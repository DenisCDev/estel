//! Small settings window. Light, sparse, no animation.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Vec2};

use crate::config::{Config, Intensity};

const PAPER: Color32 = Color32::from_rgb(250, 250, 248);
const INK: Color32 = Color32::from_rgb(26, 26, 26);
const MUTED: Color32 = Color32::from_rgb(110, 108, 104);
const LINE: Color32 = Color32::from_rgb(232, 230, 224);
const AMBER: Color32 = Color32::from_rgb(184, 122, 62);

pub fn run(initial: Config, tx: Sender<Config>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Estel")
            .with_inner_size([420.0, 620.0])
            .with_min_inner_size([380.0, 520.0])
            .with_resizable(true)
            .with_maximize_button(false),
        persist_window: false,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Estel",
        options,
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = PAPER;
            visuals.window_fill = PAPER;
            visuals.override_text_color = Some(INK);
            visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
            visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
            visuals.widgets.active.corner_radius = CornerRadius::same(6);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(244, 243, 239);
            visuals.selection.bg_fill = AMBER;
            cc.egui_ctx.set_visuals(visuals);

            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = Vec2::new(10.0, 10.0);
            style.spacing.window_margin = Margin::same(24);
            cc.egui_ctx.set_style(style);

            Ok(Box::new(SettingsApp::new(initial, tx)))
        }),
    )
}

struct SettingsApp {
    cfg: Config,
    tx: Sender<Config>,
    wake_h: u32,
    wake_m: u32,
    bed_h: u32,
    bed_m: u32,
    dirty: bool,
    last_edit: Instant,
    status: String,
    save_error: Option<String>,
}

impl SettingsApp {
    fn new(cfg: Config, tx: Sender<Config>) -> Self {
        let (wake_h, wake_m) = split_hhmm(&cfg.wake);
        let (bed_h, bed_m) = split_hhmm(&cfg.bed);
        SettingsApp {
            cfg,
            tx,
            wake_h,
            wake_m,
            bed_h,
            bed_m,
            dirty: false,
            last_edit: Instant::now(),
            status: String::new(),
            save_error: None,
        }
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
        self.status.clear();
        self.save_error = None;
    }

    fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.cfg.wake = format!("{:02}:{:02}", self.wake_h.min(23), self.wake_m.min(59));
        self.cfg.bed = format!("{:02}:{:02}", self.bed_h.min(23), self.bed_m.min(59));
        self.cfg.sanitize();
        match self.cfg.save(&Config::config_path()) {
            Ok(()) => {
                let _ = self.tx.send(self.cfg.clone());
                self.dirty = false;
                self.status = "Salvo".into();
                self.save_error = None;
            }
            Err(e) => {
                self.save_error = Some(format!(
                    "Não foi possível salvar a configuração ({e}). Verifique a pasta do app em AppData."
                ));
            }
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.dirty && self.last_edit.elapsed() > Duration::from_millis(400) {
            self.flush();
        }
        if self.dirty {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(PAPER).inner_margin(Margin::same(28)))
            .show(ctx, |ui| {
                ui.label(RichText::new("Estel").size(26.0).color(INK).strong());
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Luz e som ao longo do dia.")
                        .size(13.0)
                        .color(MUTED),
                );
                ui.add_space(22.0);

                section(ui, "INTENSIDADE");
                let mut intensity_changed = false;
                ui.horizontal(|ui| {
                    intensity_changed |= intensity_chip(ui, &mut self.cfg.intensity, Intensity::Alta, "Alta");
                    intensity_changed |= intensity_chip(ui, &mut self.cfg.intensity, Intensity::Media, "Média");
                    intensity_changed |= intensity_chip(ui, &mut self.cfg.intensity, Intensity::Suave, "Suave");
                });
                if intensity_changed {
                    self.touch();
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Suave deixa a cor quase neutra — útil em jogo ou filme.")
                        .size(12.0)
                        .color(MUTED),
                );
                ui.add_space(18.0);

                section(ui, "HORÁRIOS");
                if time_row(ui, "Acordar", &mut self.wake_h, &mut self.wake_m) {
                    self.touch();
                }
                if time_row(ui, "Dormir", &mut self.bed_h, &mut self.bed_m) {
                    self.touch();
                }
                ui.add_space(18.0);

                section(ui, "RUÍDO");
                if ui
                    .checkbox(&mut self.cfg.noise_enabled, "Ruído noturno (rosa / marrom)")
                    .changed()
                {
                    self.touch();
                }
                ui.add_space(4.0);
                ui.label(RichText::new("Volume").size(13.0).color(INK));
                let vol = ui.add(
                    egui::Slider::new(&mut self.cfg.max_volume, 0.0..=0.70)
                        .show_value(false)
                        .trailing_fill(true),
                );
                if vol.changed() {
                    self.touch();
                }
                ui.label(
                    RichText::new("O teto é baixo de propósito. Estel não toca alto.")
                        .size(12.0)
                        .color(MUTED),
                );
                ui.add_space(18.0);

                section(ui, "LOCALIZAÇÃO");
                ui.horizontal(|ui| {
                    ui.label("Latitude");
                    if ui
                        .add(egui::DragValue::new(&mut self.cfg.latitude).speed(0.1).range(-90.0..=90.0))
                        .changed()
                    {
                        self.touch();
                    }
                    ui.add_space(12.0);
                    ui.label("Longitude");
                    if ui
                        .add(egui::DragValue::new(&mut self.cfg.longitude).speed(0.1).range(-180.0..=180.0))
                        .changed()
                    {
                        self.touch();
                    }
                });
                ui.label(
                    RichText::new("Usado só para nascer e pôr do sol. Padrão: São Paulo.")
                        .size(12.0)
                        .color(MUTED),
                );

                ui.add_space(24.0);
                ui.separator();
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "Estel não é um tratamento. Só reduz o brilho, o azul da tela e o volume do próprio ruído.",
                    )
                    .size(12.0)
                    .color(MUTED),
                );

                if let Some(err) = &self.save_error {
                    ui.add_space(10.0);
                    ui.label(RichText::new(err).size(12.0).color(Color32::from_rgb(160, 40, 30)));
                } else if !self.status.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new(&self.status).size(12.0).color(AMBER));
                }
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush();
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(11.0).color(MUTED).strong());
    ui.add_space(8.0);
}

fn intensity_chip(
    ui: &mut egui::Ui,
    current: &mut Intensity,
    value: Intensity,
    label: &str,
) -> bool {
    let selected = *current == value;
    let fill = if selected {
        AMBER
    } else {
        Color32::from_rgb(244, 243, 239)
    };
    let text = if selected { Color32::WHITE } else { INK };
    let btn = egui::Button::new(RichText::new(label).color(text).size(13.0))
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(CornerRadius::same(8))
        .min_size(Vec2::new(96.0, 32.0));
    if ui.add(btn).clicked() && !selected {
        *current = value;
        true
    } else {
        false
    }
}

fn time_row(ui: &mut egui::Ui, label: &str, h: &mut u32, m: &mut u32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(13.0).color(INK));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed |= ui
                .add(egui::DragValue::new(m).range(0..=59).suffix(" min"))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(h).range(0..=23).suffix(" h"))
                .changed();
        });
    });
    changed
}

fn split_hhmm(s: &str) -> (u32, u32) {
    let mut it = s.split(':');
    let h = it.next().and_then(|x| x.parse().ok()).unwrap_or(7);
    let m = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    (h.min(23), m.min(59))
}
