//! Small settings window. Light, sparse, no animation.

use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Vec2};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};
use windows::core::w;
#[cfg(windows)]
use winit::platform::windows::EventLoopBuilderExtWindows;

use crate::config::{Config, Intensity, ScreenWindowRelation};
use crate::location;
use crate::weather::{self, Place};

const PAPER: Color32 = Color32::from_rgb(255, 251, 244);
const INK: Color32 = Color32::from_rgb(28, 35, 32);
const MUTED: Color32 = Color32::from_rgb(84, 93, 88);
const LINE: Color32 = Color32::from_rgb(55, 67, 59);
const AMBER: Color32 = Color32::from_rgb(20, 113, 63);
const MINT: Color32 = Color32::from_rgb(220, 239, 218);
const PINK: Color32 = Color32::from_rgb(255, 227, 235);
const YELLOW: Color32 = Color32::from_rgb(255, 241, 185);
const BLUE: Color32 = Color32::from_rgb(221, 239, 251);
const PEACH: Color32 = Color32::from_rgb(255, 229, 207);
const LILAC: Color32 = Color32::from_rgb(239, 229, 255);

pub fn run(initial: Config, tx: Sender<Config>) -> eframe::Result {
    let avatar = image::load_from_memory_with_format(
        include_bytes!("../assets/avatar-icon.png"),
        image::ImageFormat::Png,
    )
    .expect("O ícone do Estel está inválido")
    .resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
    .into_rgba8();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Estel")
            .with_icon(egui::IconData {
                rgba: avatar.into_raw(),
                width: 64,
                height: 64,
            })
            .with_inner_size([760.0, 850.0])
            .with_min_inner_size([700.0, 650.0])
            .with_resizable(true)
            .with_maximize_button(false),
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(windows)]
            builder.with_any_thread(true);
        })),
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
            visuals.widgets.inactive.corner_radius = CornerRadius::same(12);
            visuals.widgets.hovered.corner_radius = CornerRadius::same(12);
            visuals.widgets.active.corner_radius = CornerRadius::same(12);
            visuals.widgets.inactive.bg_fill = Color32::WHITE;
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
            visuals.selection.bg_fill = AMBER;
            cc.egui_ctx.set_visuals(visuals);

            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = Vec2::new(10.0, 12.0);
            style.spacing.window_margin = Margin::same(18);
            cc.egui_ctx.set_style(style);

            Ok(Box::new(SettingsApp::new(
                initial,
                tx,
                load_mascot_sheet(&cc.egui_ctx),
            )))
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
    camera_names: Vec<String>,
    camera_error: Option<String>,
    camera_scan_pending: bool,
    camera_scan: Receiver<Result<Vec<String>, String>>,
    location_request: bool,
    location_scan: Option<Receiver<Result<(f64, f64), String>>>,
    location_error: Option<String>,
    place_query: String,
    place_search: Option<Receiver<Result<Vec<Place>, String>>>,
    place_results: Vec<Place>,
    place_error: Option<String>,
    selected_place: Option<String>,
    mascot_sheet: Option<egui::TextureHandle>,
}

impl SettingsApp {
    fn new(cfg: Config, tx: Sender<Config>, mascot_sheet: Option<egui::TextureHandle>) -> Self {
        let location_request = cfg.location_auto;
        let (wake_h, wake_m) = split_hhmm(&cfg.wake);
        let (bed_h, bed_m) = split_hhmm(&cfg.bed);
        let (camera_tx, camera_scan) = mpsc::channel();
        std::thread::spawn(move || {
            let result = list_cameras_in_helper();
            let _ = camera_tx.send(result);
        });
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
            camera_names: Vec::new(),
            camera_error: None,
            camera_scan_pending: true,
            camera_scan,
            location_request,
            location_scan: None,
            location_error: None,
            place_query: String::new(),
            place_search: None,
            place_results: Vec::new(),
            place_error: None,
            selected_place: None,
            mascot_sheet,
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
                self.status = if signal_config_changed() {
                    "Salvo e aplicado".into()
                } else {
                    "Salvo; reinicie o Estel para aplicar".into()
                };
                self.save_error = None;
            }
            Err(e) => {
                self.save_error = Some(format!(
                    "Não foi possível salvar a configuração ({e}). Verifique a pasta do app em AppData."
                ));
            }
        }
    }

    fn start_location_request(&mut self) {
        self.location_request = false;
        self.location_error = None;
        match location::request_access() {
            Ok(access) => {
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(location::resolve(access));
                });
                self.location_scan = Some(rx);
            }
            Err(error) => self.location_error = Some(error),
        }
    }

    fn start_place_search(&mut self) {
        self.place_error = None;
        self.place_results.clear();
        let query = self.place_query.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(weather::search_places(&query));
        });
        self.place_search = Some(rx);
    }
}

fn load_mascot_sheet(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let image = image::load_from_memory_with_format(
        include_bytes!("../assets/mascote-kawaii.png"),
        image::ImageFormat::Png,
    )
    .ok()?
    .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture(
        "mascote-denisdev-kawaii",
        color,
        egui::TextureOptions::LINEAR,
    ))
}

fn poster(
    ui: &mut egui::Ui,
    sheet: &egui::TextureHandle,
    index: usize,
    fill: Color32,
    title: &str,
    caption: &str,
    width: f32,
) {
    let x = (index % 2) as f32 * 0.5;
    let y = (index / 2) as f32 * 0.5;
    let uv = egui::Rect::from_min_max(
        egui::pos2(x + 0.005, y + 0.005),
        egui::pos2(x + 0.495, y + 0.495),
    );
    let offsets = [0.0, 20.0, 5.0, 26.0];
    let scales = [0.94, 0.82, 1.0, 0.85];
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.add_space(offsets[index]);
        ui.label(
            RichText::new(format!("0{} / ESTEL", index + 1))
                .size(11.0)
                .color(AMBER)
                .strong(),
        );
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, width), egui::Sense::hover());
        let painter = ui.painter();
        let center = rect.center() + Vec2::new(4.0, 5.0);
        painter.circle_filled(center, width * 0.40, fill);
        painter.circle_filled(rect.min + Vec2::new(width * 0.18, width * 0.15), 4.0, AMBER);
        painter.circle_filled(rect.max - Vec2::new(width * 0.12, width * 0.24), 3.0, AMBER);
        let side = width * scales[index];
        let art = egui::Rect::from_center_size(center, Vec2::splat(side));
        painter.image(sheet.id(), art, uv, Color32::WHITE);
        ui.label(RichText::new(title).size(17.0).color(INK).strong());
        ui.label(RichText::new(caption).size(11.0).color(MUTED));
    });
}

fn card<R>(ui: &mut egui::Ui, fill: Color32, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let width = ui.available_width();
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.add_space(8.0);
        let (accent, _) = ui.allocate_exact_size(Vec2::new(52.0, 6.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(accent, CornerRadius::same(3), fill);
        ui.add_space(7.0);
        let result = body(ui);
        ui.add_space(23.0);
        ui.separator();
        result
    })
    .inner
}

fn signal_config_changed() -> bool {
    let event =
        match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\EstelConfigChanged")) } {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!(%error, "processo principal não recebeu a configuração");
                return false;
            }
        };
    let signaled = unsafe { SetEvent(event) };
    if let Err(error) = unsafe { CloseHandle(event) } {
        tracing::warn!(%error, "não foi possível liberar o aviso de configuração");
    }
    if let Err(error) = signaled {
        tracing::warn!(%error, "processo principal não recebeu a configuração");
        return false;
    }
    true
}

fn list_cameras_in_helper() -> Result<Vec<String>, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Não foi possível localizar o Estel ({error})."))?;
    let mut child = Command::new(executable)
        .arg("--list-cameras")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Não foi possível consultar as câmeras ({error})."))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while child
        .try_wait()
        .map_err(|error| format!("Não foi possível consultar as câmeras ({error})."))?
        .is_none()
    {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("A busca por câmeras demorou demais.".to_owned());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("Não foi possível consultar as câmeras ({error})."))?;
    if !output.status.success() {
        return Err("O Windows não permitiu listar as câmeras conectadas.".to_owned());
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|_| "O Windows retornou uma câmera com nome inválido.".to_owned())?;
    let cameras = output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if cameras.is_empty() {
        return Err("Nenhuma câmera habilitada foi encontrada pelo Windows.".to_owned());
    }
    Ok(cameras)
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.cfg.location_auto && self.location_request {
            self.start_location_request();
        }
        let location_result = self.location_scan.as_ref().map(Receiver::try_recv);
        match location_result {
            Some(Ok(Ok((latitude, longitude)))) => {
                self.location_scan = None;
                if self.cfg.location_auto {
                    self.cfg.latitude = latitude;
                    self.cfg.longitude = longitude;
                    self.selected_place = None;
                    self.touch();
                    self.flush();
                }
            }
            Some(Ok(Err(error))) => {
                self.location_scan = None;
                self.location_error = Some(error);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.location_scan = None;
                self.location_error = Some("A busca pela localização foi interrompida.".into());
            }
            _ => {}
        }
        let place_result = self.place_search.as_ref().map(Receiver::try_recv);
        match place_result {
            Some(Ok(Ok(places))) => {
                self.place_search = None;
                if places.is_empty() {
                    self.place_error =
                        Some("Nenhum lugar encontrado. Tente cidade e estado.".into());
                } else {
                    self.place_results = places;
                }
            }
            Some(Ok(Err(error))) => {
                self.place_search = None;
                self.place_error = Some(error);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.place_search = None;
                self.place_error = Some("A busca online foi interrompida. Tente novamente.".into());
            }
            _ => {}
        }
        if self.camera_scan_pending {
            match self.camera_scan.try_recv() {
                Ok(result) => {
                    self.camera_scan_pending = false;
                    match result {
                        Ok(cameras) => {
                            self.camera_names = cameras;
                            self.camera_error = None;
                        }
                        Err(error) => self.camera_error = Some(error),
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.camera_scan_pending = false;
                    self.camera_error = Some("A busca por câmeras foi interrompida.".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.dirty && self.last_edit.elapsed() > Duration::from_millis(400) {
            self.flush();
        }
        if self.dirty
            || self.camera_scan_pending
            || self.location_scan.is_some()
            || self.place_search.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        egui::TopBottomPanel::bottom("save-status")
            .frame(
                Frame::new()
                    .fill(MINT)
                    .inner_margin(Margin::symmetric(22, 12))
                    .stroke(Stroke::new(1.0_f32, LINE)),
            )
            .show(ctx, |ui| {
                if let Some(error) = &self.save_error {
                    ui.label(
                        RichText::new(error)
                            .size(12.0)
                            .color(Color32::from_rgb(160, 40, 30)),
                    );
                } else if self.dirty {
                    ui.label(RichText::new("Salvando ajustes...").size(12.0).color(INK));
                } else if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).size(12.0).color(AMBER).strong());
                } else {
                    ui.label(
                        RichText::new("Ajustes salvos automaticamente.")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
            });

        egui::CentralPanel::default()
            .frame(Frame::new().fill(PAPER).inner_margin(Margin::same(28)))
            .show(ctx, |ui| {
                let content_width = (ui.available_width() - 16.0).max(0.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.set_width(content_width);
                ui.label(RichText::new("Estel: seu cantinho de luz").size(32.0).color(INK).strong());
                ui.label(RichText::new("Brilho, cor e clima no ritmo do seu ambiente.").size(16.0).color(MUTED));
                ui.add_space(18.0);
                if let Some(sheet) = &self.mascot_sheet {
                    let width = (content_width - 36.0) / 4.0;
                    ui.horizontal(|ui| {
                        poster(ui, sheet, 0, YELLOW, "Claridade", "a luz do seu dia", width);
                        poster(ui, sheet, 1, PINK, "Seu lugar", "sol na sua cidade", width);
                        poster(ui, sheet, 2, PEACH, "Janela", "reflexos sob cuidado", width);
                        poster(ui, sheet, 3, BLUE, "Câmera", "o ambiente decide", width);
                    });
                }
                ui.add_space(20.0);
                ui.label(RichText::new("Personalize cada detalhe logo abaixo.").size(13.0).color(INK).strong());
                ui.add_space(14.0);

                card(ui, PINK, |ui| {
                section(ui, "01  /  INTENSIDADE");
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
                });
                ui.add_space(18.0);

                card(ui, YELLOW, |ui| {
                section(ui, "02  /  SEU DIA");
                if time_row(ui, "Acordar", &mut self.wake_h, &mut self.wake_m) {
                    self.touch();
                }
                if time_row(ui, "Dormir", &mut self.bed_h, &mut self.bed_m) {
                    self.touch();
                }
                ui.label(RichText::new("O sol da sua cidade ajuda a escolher quando a tela fica mais quentinha.").size(12.0).color(MUTED));
                });
                ui.add_space(18.0);

                card(ui, BLUE, |ui| {
                section(ui, "03  /  SOM DE FUNDO");
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
                });
                ui.add_space(18.0);

                card(ui, MINT, |ui| {
                section(ui, "04  /  SEU LUGAR");
                if ui
                    .checkbox(&mut self.cfg.location_auto, "Obter localização do Windows ao abrir estas configurações")
                    .changed()
                {
                    self.location_request = self.cfg.location_auto;
                    self.selected_place = None;
                    if !self.cfg.location_auto {
                        self.location_scan = None;
                        self.location_error = None;
                    }
                    self.touch();
                }
                if self.location_scan.is_some() || self.location_request {
                    ui.label(RichText::new("Buscando localização...").size(12.0).color(MUTED));
                } else if let Some(error) = &self.location_error {
                    ui.label(RichText::new(error).size(12.0).color(Color32::from_rgb(160, 40, 30)));
                    ui.hyperlink_to("Permissões de localização do Windows", "ms-settings:privacy-location");
                }
                ui.add_space(8.0);
                ui.label("Cidade ou bairro");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.place_query).hint_text("Ex.: Campinas, São Paulo"));
                    let valid = (3..=100).contains(&self.place_query.trim().chars().count());
                    if ui.add_enabled(valid && self.place_search.is_none(), egui::Button::new("Buscar"))
                        .clicked() {
                        self.start_place_search();
                    }
                });
                if self.place_search.is_some() {
                    ui.label("Buscando lugares...");
                }
                if let Some(error) = &self.place_error {
                    ui.label(RichText::new(error).size(12.0).color(Color32::from_rgb(160, 40, 30)));
                }
                let mut selected_place = None;
                for (index, place) in self.place_results.iter().enumerate() {
                    if ui.button(place.label()).clicked() {
                        selected_place = Some(index);
                    }
                }
                if let Some(index) = selected_place {
                    let place = &self.place_results[index];
                    self.selected_place = Some(place.label());
                    self.cfg.latitude = place.latitude;
                    self.cfg.longitude = place.longitude;
                    self.cfg.location_auto = false;
                    self.location_scan = None;
                    self.place_results.clear();
                    self.touch();
                }
                if let Some(name) = &self.selected_place {
                    ui.label(RichText::new(format!("Local selecionado: {name}")).size(12.0).color(AMBER));
                }
                ui.label(RichText::new("Ao buscar, o texto é enviado ao Open-Meteo (dados do GeoNames). Escolha um resultado para usar as coordenadas.").size(12.0).color(MUTED));
                if !self.place_query.trim().is_empty() {
                    let mut search_url = url::Url::parse("https://www.openstreetmap.org/search").expect("URL fixa válida");
                    search_url.query_pairs_mut().append_pair("query", self.place_query.trim());
                    ui.hyperlink_to("Buscar o ponto exato no mapa", search_url.as_str());
                }
                ui.horizontal(|ui| {
                    ui.label("Latitude");
                    if ui
                        .add(egui::DragValue::new(&mut self.cfg.latitude).speed(0.1).range(-90.0..=90.0))
                        .changed()
                    {
                        self.cfg.location_auto = false;
                        self.location_scan = None;
                        self.selected_place = None;
                        self.touch();
                    }
                    ui.add_space(12.0);
                    ui.label("Longitude");
                    if ui
                        .add(egui::DragValue::new(&mut self.cfg.longitude).speed(0.1).range(-180.0..=180.0))
                        .changed()
                    {
                        self.cfg.location_auto = false;
                        self.location_scan = None;
                        self.selected_place = None;
                        self.touch();
                    }
                });
                ui.label(
                    RichText::new("As coordenadas definem nascer e pôr do sol e, se ativado, a consulta de clima.")
                        .size(12.0)
                        .color(MUTED),
                );
                let map_url = format!("https://www.openstreetmap.org/?mlat={:.5}&mlon={:.5}#map=14/{:.5}/{:.5}",
                    self.cfg.latitude, self.cfg.longitude, self.cfg.latitude, self.cfg.longitude);
                ui.hyperlink_to("Ver o ponto no mapa; copie as coordenadas se quiser mais precisão", map_url);
                });

                ui.add_space(18.0);
                card(ui, PEACH, |ui| {
                section(ui, "05  /  SOL E JANELA");
                if ui.checkbox(&mut self.cfg.weather_enabled, "Usar clima para ajustar brilho sem câmera").changed() {
                    self.touch();
                }
                ui.label(RichText::new(weather::status_label(&self.cfg)).size(12.0).color(MUTED));
                if self.cfg.weather_enabled {
                    ctx.request_repaint_after(Duration::from_secs(1));
                }
                ui.label(RichText::new("Envia as coordenadas ao Open-Meteo a cada 15 minutos. Serviço gratuito para uso não comercial; sem rede, mantém o horário.").size(12.0).color(MUTED));
                if ui.checkbox(&mut self.cfg.window_near, "Há uma janela perto da tela").changed() {
                    self.touch();
                }
                if self.cfg.window_near {
                    let mut direction = self.cfg.window_azimuth_deg.map(|value| (value / 45.0).round() as usize % 8);
                    let mut direction_changed = false;
                    let directions = ["Norte", "Nordeste", "Leste", "Sudeste", "Sul", "Sudoeste", "Oeste", "Noroeste"];
                    egui::ComboBox::from_label("Para onde a janela aponta")
                        .selected_text(direction.map(|index| directions[index]).unwrap_or("Não sei"))
                        .show_ui(ui, |ui| {
                            direction_changed |= ui.selectable_value(&mut direction, None, "Não sei").changed();
                            for (index, label) in directions.iter().enumerate() {
                                direction_changed |= ui.selectable_value(&mut direction, Some(index), *label).changed();
                            }
                        });
                    if direction_changed {
                        self.cfg.window_azimuth_deg = direction.map(|index| index as f32 * 45.0);
                        self.touch();
                    }
                    egui::ComboBox::from_label("A tela em relação à janela")
                        .selected_text(match self.cfg.screen_window_relation {
                            ScreenWindowRelation::Front => "De frente para a janela",
                            ScreenWindowRelation::Back => "De costas para a janela",
                            ScreenWindowRelation::Side => "De lado para a janela",
                        })
                        .show_ui(ui, |ui| {
                            for (relation, label) in [
                                (ScreenWindowRelation::Front, "De frente para a janela"),
                                (ScreenWindowRelation::Back, "De costas para a janela"),
                                (ScreenWindowRelation::Side, "De lado para a janela"),
                            ] {
                                if ui.selectable_value(&mut self.cfg.screen_window_relation, relation, label).changed() { self.touch(); }
                            }
                        });
                    ui.label(RichText::new("Use a bússola do celular para saber a direção da janela. O ajuste é aproximado; a câmera, quando disponível, tem prioridade.").size(12.0).color(MUTED));
                }
                });

                ui.add_space(18.0);
                card(ui, LILAC, |ui| {
                section(ui, "06  /  LUZ AMBIENTE");
                let ambient_changed = ui
                    .scope(|ui| {
                        ui.style_mut().visuals.widgets.inactive.bg_fill = PAPER;
                        ui.style_mut().visuals.widgets.inactive.bg_stroke =
                            Stroke::new(1.5_f32, INK);
                        ui.checkbox(
                            &mut self.cfg.ambient_enabled,
                            "Ajustar brilho pela luz do ambiente",
                        )
                        .changed()
                    })
                    .inner;
                if ambient_changed {
                    self.touch();
                }
                ui.label(
                    RichText::new(
                        "Opcional e local: o Estel mede a claridade de um quadro e o descarta. Não grava, transmite ou analisa pessoas.",
                    )
                    .size(12.0)
                    .color(MUTED),
                );
                if self.cfg.ambient_enabled {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Câmera").size(13.0).color(INK));
                    let selected = self
                        .camera_names
                        .get(self.cfg.ambient_camera_index)
                        .map(String::as_str)
                        .unwrap_or(if self.camera_scan_pending {
                            "Buscando câmeras..."
                        } else {
                            "Dispositivo salvo indisponível"
                        });
                    let mut camera_changed = false;
                    egui::ComboBox::from_id_salt("ambient-camera")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for (index, name) in self.camera_names.iter().enumerate() {
                                camera_changed |= ui
                                    .selectable_value(
                                        &mut self.cfg.ambient_camera_index,
                                        index,
                                        name,
                                    )
                                    .changed();
                            }
                        });
                    if camera_changed {
                        self.touch();
                    }
                    if self.camera_names.is_empty() {
                        let message = if self.camera_scan_pending {
                            "Buscando câmeras..."
                        } else {
                            self.camera_error.as_deref().unwrap_or(
                                "Nenhuma câmera foi encontrada pelo Windows.",
                            )
                        };
                        ui.label(
                            RichText::new(message)
                                .size(12.0)
                                .color(if self.camera_scan_pending {
                                    MUTED
                                } else {
                                    Color32::from_rgb(160, 40, 30)
                                }),
                        );
                        if !self.camera_scan_pending {
                            ui.label(RichText::new("Sem câmera disponível, o Estel usa horário, localização e clima, se estiver ativo.").size(12.0).color(MUTED));
                        }
                    }
                    ui.add_space(4.0);
                    if ui
                        .add(
                            egui::Slider::new(
                                &mut self.cfg.ambient_sample_interval_seconds,
                                10..=120,
                            )
                            .text("Leitura")
                            .suffix(" s"),
                        )
                        .changed()
                    {
                        self.touch();
                    }
                    ui.label(
                        RichText::new(
                            "A câmera define o brilho conforme a claridade, independentemente do horário.",
                        )
                        .size(12.0)
                        .color(MUTED),
                    );
                }
                });

                ui.add_space(24.0);
                ui.separator();
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "Estel não é um tratamento. Ajusta brilho e cor da tela; o som opcional afeta apenas o ruído do app.",
                    )
                    .size(12.0)
                    .color(MUTED),
                );

                    });
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush();
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(16.0).color(INK).strong());
    ui.add_space(10.0);
}

fn intensity_chip(
    ui: &mut egui::Ui,
    current: &mut Intensity,
    value: Intensity,
    label: &str,
) -> bool {
    let selected = *current == value;
    let fill = if selected { AMBER } else { Color32::WHITE };
    let text = if selected { Color32::WHITE } else { INK };
    let btn = egui::Button::new(RichText::new(label).color(text).size(13.0))
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(CornerRadius::same(14))
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
