//! Poster-inspired settings window for the Windows tray app.

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Stroke, Vec2,
};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};
use windows::core::w;
#[cfg(windows)]
use winit::platform::windows::EventLoopBuilderExtWindows;

use crate::ambient::{self, AmbientSample};
use crate::config::{AmbientCalibration, Config, Intensity, ScreenWindowRelation, SupportCountry};
use crate::location;
use crate::update::{self, DownloadedInstaller, Release};
use crate::weather::{self, Place};

const PAPER: Color32 = Color32::from_rgb(255, 250, 238);
const INK: Color32 = Color32::from_rgb(32, 49, 55);
const MUTED: Color32 = Color32::from_rgb(73, 85, 88);
const LINE: Color32 = Color32::from_rgb(32, 49, 55);
const AMBER: Color32 = Color32::from_rgb(17, 110, 70);
const MINT: Color32 = Color32::from_rgb(189, 237, 181);
const PINK: Color32 = Color32::from_rgb(255, 181, 207);
const YELLOW: Color32 = Color32::from_rgb(255, 233, 119);
const BLUE: Color32 = Color32::from_rgb(159, 222, 251);
const PEACH: Color32 = Color32::from_rgb(255, 204, 148);
const LILAC: Color32 = Color32::from_rgb(222, 194, 255);
const HOT_PINK: Color32 = Color32::from_rgb(229, 58, 115);
const DEEP_GREEN: Color32 = Color32::from_rgb(18, 105, 62);

enum UpdateState {
    Checking(Receiver<Result<Option<Release>, String>>),
    Current,
    Available(Release),
    Downloading {
        rx: Receiver<Result<DownloadedInstaller, String>>,
        downloaded: Arc<AtomicU64>,
        total: u64,
    },
    Launched,
    Error(String),
}

fn check_updates() -> UpdateState {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(update::check_latest());
    });
    UpdateState::Checking(rx)
}

fn poster_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("poster".into()))
}

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
            .with_inner_size([1020.0, 850.0])
            .with_min_inner_size([820.0, 650.0])
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
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "fredoka".into(),
                Arc::new(egui::FontData::from_static(include_bytes!(
                    "../assets/Fredoka[wdth,wght].ttf"
                ))),
            );
            fonts.font_data.insert(
                "lilita".into(),
                Arc::new(egui::FontData::from_static(include_bytes!(
                    "../assets/LilitaOne-Regular.ttf"
                ))),
            );
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "fredoka".into());
            fonts.families.insert(
                FontFamily::Name("poster".into()),
                vec!["lilita".into(), "fredoka".into()],
            );
            cc.egui_ctx.set_fonts(fonts);
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = PAPER;
            visuals.window_fill = PAPER;
            visuals.override_text_color = Some(INK);
            visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
            visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
            visuals.widgets.active.corner_radius = CornerRadius::same(6);
            visuals.widgets.inactive.bg_fill = Color32::WHITE;
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.5_f32, LINE);
            visuals.selection.bg_fill = HOT_PINK;
            cc.egui_ctx.set_visuals(visuals);

            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = Vec2::new(10.0, 10.0);
            style.spacing.window_margin = Margin::same(18);
            cc.egui_ctx.set_style(style);

            Ok(Box::new(SettingsApp::new(
                initial,
                tx,
                load_mascot_sheet(&cc.egui_ctx),
                load_poster_collage(&cc.egui_ctx),
            )))
        }),
    )
}

struct CalibrationCapture {
    dark: bool,
    camera_index: usize,
    rx: Receiver<Result<AmbientSample, String>>,
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
    calibration_capture: Option<CalibrationCapture>,
    calibration_dark: Option<AmbientSample>,
    calibration_message: Option<String>,
    location_request: bool,
    location_scan: Option<Receiver<Result<(f64, f64), String>>>,
    location_error: Option<String>,
    location_success: bool,
    place_query: String,
    place_search: Option<Receiver<Result<Vec<Place>, String>>>,
    place_results: Vec<Place>,
    place_error: Option<String>,
    selected_place: Option<String>,
    update_state: UpdateState,
    mascot_sheet: Option<egui::TextureHandle>,
    poster_collage: egui::TextureHandle,
    eye_break_until: Option<Instant>,
    eye_break_complete: bool,
    grounding_open: bool,
}

impl SettingsApp {
    fn new(
        cfg: Config,
        tx: Sender<Config>,
        mascot_sheet: Option<egui::TextureHandle>,
        poster_collage: egui::TextureHandle,
    ) -> Self {
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
            calibration_capture: None,
            calibration_dark: None,
            calibration_message: None,
            location_request,
            location_scan: None,
            location_error: None,
            location_success: false,
            place_query: String::new(),
            place_search: None,
            place_results: Vec::new(),
            place_error: None,
            selected_place: None,
            update_state: check_updates(),
            mascot_sheet,
            poster_collage,
            eye_break_until: None,
            eye_break_complete: false,
            grounding_open: false,
        }
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
        self.status.clear();
        self.save_error = None;
    }

    fn start_calibration_capture(&mut self, dark: bool) {
        self.calibration_message = None;
        if dark {
            self.calibration_dark = None;
        }
        let camera_index = self.cfg.ambient_camera_index;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(ambient::sample_luminance_in_helper(camera_index));
        });
        self.calibration_capture = Some(CalibrationCapture {
            dark,
            camera_index,
            rx,
        });
    }

    fn cancel_calibration_capture(&mut self) {
        self.calibration_capture = None;
        self.calibration_message = Some("Leitura descartada; referências anteriores preservadas. A captura encerra em até 5 segundos.".into());
    }

    fn poll_calibration(&mut self) {
        let Some(capture) = self.calibration_capture.as_ref() else {
            return;
        };
        let result = match capture.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("A leitura foi interrompida. Tente novamente.".into())
            }
        };
        let capture = self
            .calibration_capture
            .take()
            .expect("capture checked above");
        if capture.camera_index != self.cfg.ambient_camera_index
            || !self.cfg.ambient_enabled
            || self.cfg.preserve_colors()
        {
            self.calibration_message =
                Some("Leitura descartada porque a câmera ou a opção mudou.".into());
            return;
        }
        match result {
            Err(error) => self.calibration_message = Some(error),
            Ok(sample) if capture.dark => {
                self.calibration_dark = Some(sample);
                self.calibration_message = Some("Referência escura capturada. Com iluminação ambiente mais clara, capture a segunda referência.".into());
            }
            Ok(sample) => {
                let Some(dark) = self.calibration_dark.as_ref() else {
                    return;
                };
                let calibration = AmbientCalibration {
                    device_id: sample.device_id.clone(),
                    camera_index: capture.camera_index,
                    dark: dark.luminance,
                    bright: sample.luminance,
                };
                if sample.device_id != dark.device_id || !calibration.is_valid() {
                    self.calibration_message = Some("As referências não distinguem bem claro de escuro ou são de câmeras diferentes. A exposição automática pode esconder a diferença. Refaça com iluminação ambiente distinta e o mesmo enquadramento; se persistir, use o ajuste por horário.".into());
                    return;
                }
                self.cfg.ambient_calibration = Some(calibration);
                self.calibration_message = Some("Referências aceitas. A câmera pode ajustar o brilho dentro dessa faixa; confira o conforto e refaça se mudar o enquadramento.".into());
                self.touch();
            }
        }
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
        self.location_success = false;
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

fn load_poster_collage(ctx: &egui::Context) -> egui::TextureHandle {
    let image = image::load_from_memory_with_format(
        include_bytes!("../assets/estel-poster-collage.png"),
        image::ImageFormat::Png,
    )
    .expect("A arte do Estel está inválida")
    .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    ctx.load_texture("estel-poster-collage", color, egui::TextureOptions::LINEAR)
}

fn poster_button(label: &str, fill: Color32, selected: bool) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(label.to_owned())
            .size(16.0)
            .color(if selected { Color32::WHITE } else { INK })
            .strong(),
    )
    .fill(if selected { DEEP_GREEN } else { fill })
    .stroke(Stroke::new(2.0_f32, INK))
    .corner_radius(CornerRadius::same(6))
}

fn poster_cover(
    ui: &mut egui::Ui,
    collage: &egui::TextureHandle,
    cfg: &mut Config,
    width: f32,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 375.0), egui::Sense::hover());
    {
        let painter = ui.painter();
        painter.rect_filled(
            rect.translate(Vec2::new(6.0, 6.0)),
            CornerRadius::same(8),
            INK,
        );
        painter.rect_filled(rect, CornerRadius::same(8), MINT);
        painter.rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(3.0_f32, INK),
            egui::StrokeKind::Inside,
        );
        painter.circle_filled(rect.min + Vec2::new(35.0, 34.0), 7.0, HOT_PINK);
        painter.circle_filled(rect.min + Vec2::new(55.0, 34.0), 7.0, BLUE);
        painter.circle_filled(rect.min + Vec2::new(75.0, 34.0), 7.0, MINT);
        painter.text(
            rect.min + Vec2::new(98.0, 24.0),
            egui::Align2::LEFT_TOP,
            format!("01 / ESTEL {}", env!("CARGO_PKG_VERSION")),
            poster_font(16.0),
            INK,
        );
        let art = egui::Rect::from_min_size(
            rect.min + Vec2::new(width * 0.30, 30.0),
            Vec2::new(width * 0.68, 315.0),
        );
        painter.image(
            collage.id(),
            art,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.text(
            rect.min + Vec2::new(30.0, 65.0),
            egui::Align2::LEFT_TOP,
            "ESTEL!",
            poster_font(65.0),
            Color32::WHITE,
        );
        painter.text(
            rect.min + Vec2::new(27.0, 61.0),
            egui::Align2::LEFT_TOP,
            "ESTEL!",
            poster_font(65.0),
            DEEP_GREEN,
        );
        painter.text(
            rect.min + Vec2::new(32.0, 132.0),
            egui::Align2::LEFT_TOP,
            "SUA LUZ, SUAS REGRAS",
            poster_font(22.0),
            INK,
        );
        painter.text(
            rect.min + Vec2::new(32.0, 165.0),
            egui::Align2::LEFT_TOP,
            "Brilho suave no seu ritmo.",
            FontId::proportional(16.0),
            INK,
        );
        painter.text(
            rect.min + Vec2::new(32.0, 186.0),
            egui::Align2::LEFT_TOP,
            "Clima e câmera são opcionais.",
            FontId::proportional(16.0),
            INK,
        );
        painter.text(
            rect.min + Vec2::new(31.0, 289.0),
            egui::Align2::LEFT_TOP,
            "ESCOLHA A FORÇA DA COR",
            poster_font(17.0),
            INK,
        );
    }
    let mut changed = false;
    let camera_label = if cfg.ambient_enabled && !cfg.camera_is_calibrated() {
        "CÂMERA: AGUARDANDO"
    } else if cfg.ambient_enabled {
        "USAR CÂMERA: SIM"
    } else {
        "USAR CÂMERA: NÃO"
    };
    if ui
        .place(
            egui::Rect::from_min_size(rect.min + Vec2::new(30.0, 220.0), Vec2::new(222.0, 48.0)),
            poster_button(camera_label, MINT, cfg.ambient_enabled),
        )
        .clicked()
    {
        cfg.ambient_enabled = !cfg.ambient_enabled;
        changed = true;
    }
    for (index, (intensity, label)) in [
        (Intensity::Alta, "ALTA"),
        (Intensity::Media, "MÉDIA"),
        (Intensity::Suave, "SUAVE"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = [318.0, 325.0, 312.0][index];
        let selected = cfg.intensity == intensity;
        if ui
            .place(
                egui::Rect::from_min_size(
                    rect.min + Vec2::new(30.0 + index as f32 * 119.0, y),
                    Vec2::new(108.0, 42.0),
                ),
                poster_button(label, PINK, selected),
            )
            .clicked()
            && !selected
        {
            cfg.intensity = intensity;
            changed = true;
        }
    }
    let weather_label = if cfg.weather_enabled {
        "USAR CLIMA: SIM"
    } else {
        "USAR CLIMA: NÃO"
    };
    if ui
        .place(
            egui::Rect::from_min_size(
                rect.min + Vec2::new(width - 212.0, 315.0),
                Vec2::new(182.0, 42.0),
            ),
            poster_button(weather_label, BLUE, cfg.weather_enabled),
        )
        .clicked()
    {
        cfg.weather_enabled = !cfg.weather_enabled;
        changed = true;
    }
    changed
}

fn card<R>(
    ui: &mut egui::Ui,
    fill: Color32,
    sheet: Option<&egui::TextureHandle>,
    index: usize,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let width = ui.available_width();
    let shown = Frame::new()
        .fill(fill)
        .stroke(Stroke::new(2.5_f32, INK))
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::same(20))
        .shadow(egui::Shadow {
            offset: [5, 5],
            blur: 0,
            spread: 0,
            color: INK,
        })
        .show(ui, |ui| {
            ui.set_width((width - 40.0).max(0.0));
            let result = body(ui);
            ui.add_space(66.0);
            result
        });
    let corner = shown.response.rect.right_bottom();
    let footer = shown.response.rect.left_bottom();
    ui.painter().text(
        footer + Vec2::new(31.0, -27.0),
        egui::Align2::LEFT_CENTER,
        "ESTEL / LUZ BOA",
        poster_font(13.0),
        INK,
    );
    ui.painter()
        .circle_filled(footer + Vec2::new(20.0, -27.0), 4.0, HOT_PINK);
    if let Some(sheet) = sheet {
        let x = (index % 2) as f32 * 0.5;
        let y = (index / 2) as f32 * 0.5;
        let uv = egui::Rect::from_min_max(
            egui::pos2(x + 0.005, y + 0.005),
            egui::pos2(x + 0.495, y + 0.495),
        );
        let art = egui::Rect::from_min_size(corner - Vec2::new(98.0, 98.0), Vec2::splat(86.0));
        ui.painter().image(sheet.id(), art, uv, Color32::WHITE);
    }
    shown.inner
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
                    self.location_success = true;
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
        let next_update = match &self.update_state {
            UpdateState::Checking(rx) => match rx.try_recv() {
                Ok(Ok(Some(release))) => Some(UpdateState::Available(release)),
                Ok(Ok(None)) => Some(UpdateState::Current),
                Ok(Err(error)) => Some(UpdateState::Error(error)),
                Err(TryRecvError::Disconnected) => Some(UpdateState::Error(
                    "A busca por atualização foi interrompida. Tente novamente.".into(),
                )),
                Err(TryRecvError::Empty) => None,
            },
            UpdateState::Downloading { rx, .. } => match rx.try_recv() {
                Ok(Ok(installer)) => Some(match update::launch_installer(installer) {
                    Ok(()) => UpdateState::Launched,
                    Err(error) => UpdateState::Error(error),
                }),
                Ok(Err(error)) => Some(UpdateState::Error(error)),
                Err(TryRecvError::Disconnected) => Some(UpdateState::Error(
                    "O download foi interrompido. Tente novamente.".into(),
                )),
                Err(TryRecvError::Empty) => None,
            },
            _ => None,
        };
        if let Some(state) = next_update {
            self.update_state = state;
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
        self.poll_calibration();
        if self.dirty && self.last_edit.elapsed() > Duration::from_millis(400) {
            self.flush();
        }
        if self.dirty
            || self.camera_scan_pending
            || self.calibration_capture.is_some()
            || self.location_scan.is_some()
            || self.place_search.is_some()
            || self
                .eye_break_until
                .is_some_and(|until| Instant::now() < until)
            || matches!(
                self.update_state,
                UpdateState::Checking(_) | UpdateState::Downloading { .. }
            )
        {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        egui::TopBottomPanel::bottom("save-status")
            .frame(
                Frame::new()
                    .fill(INK)
                    .inner_margin(Margin::symmetric(22, 12))
                    .stroke(Stroke::new(2.0_f32, DEEP_GREEN)),
            )
            .show(ctx, |ui| {
                if let Some(error) = &self.save_error {
                    ui.label(RichText::new(error).size(12.0).color(YELLOW));
                } else if self.dirty {
                    ui.label(
                        RichText::new("Salvando ajustes...")
                            .size(12.0)
                            .color(Color32::WHITE),
                    );
                } else if !self.status.is_empty() {
                    ui.label(
                        RichText::new(&self.status)
                            .size(12.0)
                            .color(YELLOW)
                            .strong(),
                    );
                } else {
                    ui.label(
                        RichText::new("ESTEL / ajustes salvos automaticamente")
                            .size(13.0)
                            .color(Color32::WHITE),
                    );
                }
            });

        egui::CentralPanel::default()
            .frame(Frame::new().fill(PAPER).inner_margin(Margin::same(25)))
            .show(ctx, |ui| {
                let content_width = (ui.available_width() - 14.0).max(0.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.set_width(content_width);
                let sticker_sheet = self.mascot_sheet.clone();
                if poster_cover(ui, &self.poster_collage, &mut self.cfg, content_width) {
                    self.touch();
                }
                ui.add_space(14.0);
                if self.cfg.ambient_enabled && !self.cfg.camera_is_calibrated() {
                    ui.label(RichText::new("Câmera aguardando referências. Role até 06 / LUZ AMBIENTE para capturar escuro e claro; enquanto isso, o brilho segue horário e clima opcional.").size(14.0).color(INK));
                }
                Frame::new()
                    .fill(BLUE)
                    .stroke(Stroke::new(2.0_f32, INK))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.set_width(content_width - 32.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("ATUALIZAÇÕES /").font(poster_font(17.0)).color(INK));
                            match &self.update_state {
                                UpdateState::Checking(_) => { ui.label("Verificando versão publicada..."); }
                                UpdateState::Current => { ui.label("Esta é a versão mais recente."); }
                                UpdateState::Available(release) => { ui.label(format!("{} disponível", release.version)); }
                                UpdateState::Downloading { downloaded, total, .. } => {
                                    let progress = downloaded.load(Ordering::Relaxed);
                                    if progress >= *total {
                                        ui.label("Download concluído. Verificando o instalador...");
                                    } else {
                                        ui.label(format!("Baixando instalador: {}%", progress * 100 / total));
                                    }
                                }
                                UpdateState::Launched => { ui.label("Instalador aberto. Conclua as etapas na janela do Windows."); }
                                UpdateState::Error(error) => { ui.label(RichText::new(error).color(Color32::from_rgb(160, 40, 30))); }
                            }
                            let action = match &self.update_state {
                                UpdateState::Current | UpdateState::Error(_) => Some("VERIFICAR DE NOVO"),
                                UpdateState::Available(_) => Some("ATUALIZAR AGORA"),
                                _ => None,
                            };
                            if let Some(label) = action
                                && ui.add(egui::Button::new(RichText::new(label).strong()).fill(YELLOW).stroke(Stroke::new(1.5_f32, INK))).clicked() {
                                    match &self.update_state {
                                        UpdateState::Available(release) => {
                                            let release = release.clone();
                                            let (tx, rx) = mpsc::channel();
                                            let downloaded = Arc::new(AtomicU64::new(0));
                                            let progress = downloaded.clone();
                                            let total = release.size;
                                            std::thread::spawn(move || {
                                                let _ = tx.send(update::download_installer(&release, &progress));
                                            });
                                            self.update_state = UpdateState::Downloading { rx, downloaded, total };
                                        }
                                        _ => self.update_state = check_updates(),
                                    }
                            }
                        });
                    });
                ui.add_space(18.0);
                ui.label(RichText::new("CORES DO SEU JEITO").font(poster_font(23.0)).color(DEEP_GREEN));
                ui.horizontal_wrapped(|ui| {
                    if toggle_sticker(ui, &mut self.cfg.color_critical_work, "TRABALHO COM CORES", YELLOW) {
                        self.touch();
                    }
                    if toggle_sticker(ui, &mut self.cfg.color_vision_deficiency, "TENHO DALTONISMO", PINK) {
                        self.touch();
                    }
                });
                if self.cfg.preserve_colors() {
                    ui.label("Cores preservadas: o Estel pausa seus ajustes de tela, inclusive brilho automático. O som pode continuar.");
                } else {
                    ui.label("Ative uma opção para preservar as cores originais do monitor. O Estel não diagnostica nem corrige daltonismo.");
                }
                ui.add_space(32.0);
                ui.label(RichText::new("SEU MUNDO / SEUS AJUSTES").font(poster_font(28.0)).color(DEEP_GREEN));
                ui.label(RichText::new("Média é o ponto de partida; Suave deixa a cor mais próxima da original. A intensidade da cor e do som não altera seus limites de brilho.").size(14.0).color(INK));
                ui.add_space(18.0);

                ui.columns(2, |columns| {
                card(&mut columns[0], YELLOW, sticker_sheet.as_ref(), 0, |ui| {
                section(ui, "02  /  SEU DIA");
                if time_row(ui, "Acordar", &mut self.wake_h, &mut self.wake_m) {
                    self.touch();
                }
                if time_row(ui, "Dormir", &mut self.bed_h, &mut self.bed_m) {
                    self.touch();
                }
                ui.label(RichText::new("O sol da sua cidade ajuda a escolher quando a tela fica mais quentinha.").size(12.0).color(MUTED));
                });

                columns[1].add_space(34.0);
                card(&mut columns[1], BLUE, sticker_sheet.as_ref(), 3, |ui| {
                section(ui, "03  /  SOM DE FUNDO");
                if toggle_sticker(ui, &mut self.cfg.noise_enabled, "RUÍDO NOTURNO", PINK) {
                    self.touch();
                }
                ui.label(RichText::new("Rosa ou marrom, só durante a noite.").size(13.0).color(INK));
                ui.add_space(4.0);
                ui.label(RichText::new(format!("NÍVEL DO SOM / {:.0}%", self.cfg.max_volume * 100.0)).font(poster_font(17.0)).color(INK));
                let slider_width = ui.available_width() * 0.78;
                let vol = ui.scope(|ui| {
                    ui.spacing_mut().slider_width = slider_width;
                    ui.add(egui::Slider::new(&mut self.cfg.max_volume, 0.0..=0.70)
                        .show_value(false)
                        .trailing_fill(true))
                }).inner;
                if vol.changed() {
                    self.touch();
                }
                ui.label(
                    RichText::new("Opcional. O limite é digital: em fones, confira o volume real e reduza ou desligue se incomodar. Não há benefício comprovado para todos.")
                        .size(12.0)
                        .color(MUTED),
                );
                });
                });
                ui.add_space(30.0);

                card(ui, MINT, sticker_sheet.as_ref(), 1, |ui| {
                section(ui, "04  /  SEU LUGAR");
                if toggle_sticker(ui, &mut self.cfg.location_auto, "LOCALIZAÇÃO DO WINDOWS", YELLOW) {
                    self.location_request = self.cfg.location_auto;
                    self.selected_place = None;
                    if !self.cfg.location_auto {
                        self.location_scan = None;
                        self.location_error = None;
                        self.location_success = false;
                    }
                    self.touch();
                }
                if self.location_scan.is_some() || self.location_request {
                    ui.label(RichText::new("Buscando localização...").size(12.0).color(MUTED));
                } else if let Some(error) = &self.location_error {
                    ui.label(RichText::new(error).size(12.0).color(Color32::from_rgb(160, 40, 30)));
                    ui.hyperlink_to("Permissões de localização do Windows", "ms-settings:privacy-location");
                } else if self.location_success {
                    let message = if self.cfg.weather_enabled {
                        "Localização obtida do Windows. O sol e o clima usam as coordenadas abaixo."
                    } else {
                        "Localização obtida do Windows. O sol usa as coordenadas abaixo; o clima está desligado."
                    };
                    ui.label(RichText::new(message).size(12.0).color(INK));
                }
                ui.add_space(8.0);
                ui.label("Cidade ou bairro");
                ui.horizontal(|ui| {
                    if ui.add_sized([420.0, 42.0], egui::TextEdit::singleline(&mut self.place_query).font(FontId::proportional(16.0)).hint_text("Ex.: Campinas, São Paulo")).changed() {
                        self.place_results.clear();
                        self.place_error = None;
                        self.place_search = None;
                    }
                    let valid = (3..=100).contains(&self.place_query.trim().chars().count());
                    if ui.add_enabled(valid && self.place_search.is_none(), egui::Button::new(RichText::new("BUSCAR").size(16.0).color(INK).strong()).fill(PINK).stroke(Stroke::new(2.0_f32, INK)).corner_radius(CornerRadius::same(5)).min_size(Vec2::new(120.0, 42.0)))
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
                    ui.horizontal(|ui| {
                        ui.add_space((index % 3) as f32 * 18.0);
                        let color = [PINK, YELLOW, BLUE][index % 3];
                        if ui.add(egui::Button::new(RichText::new(place.label()).size(15.0).color(INK)).fill(color).stroke(Stroke::new(1.5_f32, INK)).corner_radius(CornerRadius::same(5))).clicked() {
                            selected_place = Some(index);
                        }
                    });
                }
                if let Some(index) = selected_place {
                    let place = &self.place_results[index];
                    self.selected_place = Some(place.label());
                    self.cfg.latitude = place.latitude;
                    self.cfg.longitude = place.longitude;
                    self.cfg.location_auto = false;
                    self.location_scan = None;
                    self.location_success = false;
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
                        self.location_success = false;
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
                        self.location_success = false;
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

                ui.add_space(30.0);
                ui.columns(2, |columns| {
                card(&mut columns[0], PEACH, sticker_sheet.as_ref(), 2, |ui| {
                section(ui, "05  /  BRILHO, CLIMA E JANELA");
                ui.label("Brilho mínimo da tela");
                if ui.add(egui::Slider::new(&mut self.cfg.min_brightness, 0.15..=0.80).custom_formatter(|value, _| format!("{:.0}%", value * 100.0))).changed() {
                    self.touch();
                }
                ui.label(RichText::new("Ajuste até o texto ficar legível sem a tela parecer intensa demais. Esse mínimo vale para todos os ajustes da tela.").size(12.0).color(MUTED));
                if ui.add(egui::Slider::new(&mut self.cfg.day_brightness_max, self.cfg.min_brightness..=1.0).text("Máximo durante o dia").custom_formatter(|value, _| format!("{:.0}%", value * 100.0))).changed() { self.touch(); }
                self.cfg.rest_brightness_max = self.cfg.rest_brightness_max.clamp(self.cfg.min_brightness, self.cfg.day_brightness_max.max(self.cfg.min_brightness));
                if ui.add(egui::Slider::new(&mut self.cfg.rest_brightness_max, self.cfg.min_brightness..=self.cfg.day_brightness_max.max(self.cfg.min_brightness)).text("Máximo à noite / descanso").custom_formatter(|value, _| format!("{:.0}%", value * 100.0))).changed() { self.touch(); }
                ui.label(RichText::new("O limite de descanso vale após o pôr do sol e das 3 horas antes de dormir até acordar, mesmo sem câmera. Começa em 25%; ajuste pela legibilidade e pelo seu conforto. Os percentuais não medem a luz nos olhos.").size(12.0).color(MUTED));
                if toggle_sticker(ui, &mut self.cfg.weather_enabled, "USAR CLIMA", BLUE) {
                    self.touch();
                }
                ui.label(RichText::new(weather::status_label(&self.cfg)).size(12.0).color(MUTED));
                if self.cfg.weather_enabled {
                    ctx.request_repaint_after(Duration::from_secs(1));
                }
                ui.label(RichText::new("Envia as coordenadas ao Open-Meteo a cada 15 minutos. Serviço gratuito para uso não comercial; sem rede, mantém o horário.").size(12.0).color(MUTED));
                if toggle_sticker(ui, &mut self.cfg.window_near, "JANELA PERTO DA TELA", YELLOW) {
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
                    ui.label(RichText::new("Use a bússola do celular para saber a direção da janela. A estimativa é aproximada; a câmera, se ativada, tem prioridade e o clima fica de reserva.").size(12.0).color(MUTED));
                }
                });

                columns[1].add_space(38.0);
                card(&mut columns[1], LILAC, sticker_sheet.as_ref(), 3, |ui| {
                section(ui, "06  /  LUZ AMBIENTE");
                let ambient_changed = toggle_sticker(ui, &mut self.cfg.ambient_enabled, "BRILHO PELA CÂMERA", MINT);
                if ambient_changed {
                    self.touch();
                }
                ui.label(
                    RichText::new(
                        "Opcional e local: a câmera compara a claridade da imagem com duas referências suas. Não grava, transmite ou analisa pessoas. A exposição automática pode distorcer a leitura; a calibração é relativa e não mede lux.",
                    )
                    .size(12.0)
                    .color(MUTED),
                );
                ui.label(RichText::new("Se o Windows oferece brilho automático por sensor de luz, experimente essa opção primeiro. Para usá-lo sozinho, pause os ajustes de tela do Estel na bandeja; desligar só a câmera mantém o brilho por horário.").size(12.0).color(INK));
                ui.hyperlink_to("Abrir ajustes de tela do Windows", "ms-settings:display");
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
                        self.calibration_dark = None;
                        self.calibration_message = None;
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
                    ui.label(if self.cfg.camera_is_calibrated() { "Referências salvas para esta câmera" } else { "Sem referências: o brilho segue o horário e o clima opcional" });
                    ui.label(RichText::new("Mantenha a câmera na posição de uso e a mesma tela. Capture um ambiente com pouca luz e, depois, com mais luz difusa, sem apontar lâmpadas à lente nem cobri-la. Você pode fazer as etapas em momentos diferentes enquanto este painel estiver aberto. Referências anteriores só são substituídas quando as duas novas forem aceitas.").size(12.0).color(MUTED));
                    let available = self.calibration_capture.is_none() && !self.camera_scan_pending
                        && self.camera_names.get(self.cfg.ambient_camera_index).is_some() && !self.cfg.preserve_colors();
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(available, egui::Button::new("1 · CAPTURAR AMBIENTE ESCURO")).clicked() { self.start_calibration_capture(true); }
                        if ui.add_enabled(available && self.calibration_dark.is_some(), egui::Button::new("2 · CAPTURAR AMBIENTE CLARO")).clicked() { self.start_calibration_capture(false); }
                    });
                    if self.cfg.preserve_colors() { ui.label("Desative a preservação de cores para calibrar e usar os ajustes de tela."); }
                    ui.label("Faixa de brilho estimada pela câmera");
                    if ui.add(egui::Slider::new(&mut self.cfg.ambient_brightness_min, 0.15..=self.cfg.ambient_brightness_max).text("Ambiente escuro").custom_formatter(|value, _| format!("{:.0}%", value * 100.0))).changed() {
                        self.touch();
                    }
                    if ui.add(egui::Slider::new(&mut self.cfg.ambient_brightness_max, self.cfg.ambient_brightness_min..=1.0).text("Ambiente claro").custom_formatter(|value, _| format!("{:.0}%", value * 100.0))).changed() {
                        self.touch();
                    }
                    ui.label(
                        RichText::new(
                            "A câmera corrige parte do brilho por horário e respeita seus limites de descanso. Quadros instáveis, saturados, de outro dispositivo ou fora das referências são rejeitados. Sem leitura válida, usa clima opcional ou horário. Refaça as referências se mover a câmera.",
                        )
                        .size(12.0)
                        .color(MUTED),
                    );
                    ui.label(RichText::new("O estado da leitura aparece no menu do ícone do Estel, ao lado do relógio. Se a câmera ficar indisponível, feche apps que a usam e confira as permissões no Windows.").size(12.0).color(MUTED));
                }
                if self.calibration_capture.is_some() {
                    ui.spinner();
                    ui.label("Lendo a câmera… prazo máximo de 5 segundos.");
                    if ui.button("CANCELAR LEITURA").clicked() { self.cancel_calibration_capture(); }
                }
                if let Some(message) = &self.calibration_message { ui.label(RichText::new(message).size(12.0).color(INK)); }
                });
                });

                ui.add_space(24.0);
                card(ui, BLUE, sticker_sheet.as_ref(), 0, |ui| {
                    section(ui, "07  /  FOCO E CONFORTO");
                    ui.label("Luz difusa, menos reflexos e texto em tamanho confortável ajudam mais do que escurecer a tela ao máximo. Deixe a janela de lado em relação ao monitor quando puder.");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("PAUSA PARA OS OLHOS · 20 S").clicked() {
                            self.eye_break_until = Some(Instant::now() + Duration::from_secs(20));
                            self.eye_break_complete = false;
                        }
                        if let Some(until) = self.eye_break_until {
                            let remaining = until.saturating_duration_since(Instant::now()).as_secs();
                            if remaining > 0 {
                                ui.label(format!("Olhe para longe e pisque com calma · {remaining} s"));
                            } else {
                                self.eye_break_until = None;
                                self.eye_break_complete = true;
                            }
                        }
                        if self.eye_break_complete {
                            ui.label("Pausa concluída. Volte quando quiser.");
                        }
                    });
                    ui.label(RichText::new("Faça pausas no seu ritmo; o lembrete de 20 segundos é um guia, não uma regra terapêutica.").size(12.0).color(MUTED));
                });
                ui.add_space(24.0);
                card(ui, LILAC, sticker_sheet.as_ref(), 1, |ui| {
                    section(ui, "08  /  MOMENTO DIFÍCIL");
                    if ui.button(if self.grounding_open { "FECHAR EXERCÍCIO" } else { "ABRIR EXERCÍCIO DE ATERRAMENTO" }).clicked() {
                        self.grounding_open = !self.grounding_open;
                    }
                    if self.grounding_open {
                        ui.label("Sinta os pés no chão. Respire devagar, sem forçar. Observe 5 coisas que vê, 4 que ouve, 3 que toca, 2 cheiros e 1 sabor. Se for demais, escolha só uma coisa ao seu redor.");
                    }
                    ui.label("Quer conversar com alguém? Escolha o país para ver serviços de apoio emocional e atendimento.");
                    egui::ComboBox::from_label("País para apoio")
                        .selected_text(match self.cfg.support_country {
                            SupportCountry::Brazil => "Brasil",
                            SupportCountry::Portugal => "Portugal",
                            SupportCountry::UnitedStates => "Estados Unidos",
                            SupportCountry::UnitedKingdom => "Reino Unido",
                            SupportCountry::Other => "Outro país",
                        })
                        .show_ui(ui, |ui| {
                            for (country, label) in [
                                (SupportCountry::Brazil, "Brasil"),
                                (SupportCountry::Portugal, "Portugal"),
                                (SupportCountry::UnitedStates, "Estados Unidos"),
                                (SupportCountry::UnitedKingdom, "Reino Unido"),
                                (SupportCountry::Other, "Outro país"),
                            ] {
                                if ui.selectable_value(&mut self.cfg.support_country, country, label).changed() {
                                    self.touch();
                                }
                            }
                        });
                    match self.cfg.support_country {
                        SupportCountry::Brazil => {
                            ui.label("Apoio emocional: CVV 188 (24 h). Cuidado contínuo: CAPS do SUS. Em urgência: SAMU 192.");
                            ui.hyperlink_to("Conversar com o CVV", "https://cvv.org.br/o-cvv/");
                            ui.hyperlink_to("Encontrar cuidado pelo CAPS", "https://www.gov.br/saude/pt-br/composicao/saes/desmad/raps/caps/caps/");
                        }
                        SupportCountry::Portugal => {
                            ui.label("Apoio psicológico: SNS 24, 808 24 24 24, opção 4. Em emergência: 112.");
                            ui.hyperlink_to("Apoio psicológico do SNS 24", "https://portugal.gov.pt/gc23/comunicacao/noticias/linha-de-apoio-psicologico-do-sns-24-ja-atendeu-mais-de-240-mil-chamadas");
                            ui.hyperlink_to("Contatos de emergência em Portugal", "https://www.gov.pt/guias/contactos-de-emergencia-em-portugal");
                        }
                        SupportCountry::UnitedStates => {
                            ui.label("Apoio emocional: ligue ou envie mensagem para 988. Em emergência: 911.");
                            ui.hyperlink_to("Conversar com a 988 Lifeline", "https://988lifeline.org/");
                        }
                        SupportCountry::UnitedKingdom => {
                            ui.label("Escuta: Samaritans 116 123. Ajuda urgente em saúde mental: NHS 111 e escolha a opção de saúde mental. Em emergência: 999.");
                            ui.hyperlink_to("Ajuda em saúde mental do NHS", "https://www.nhs.uk/nhs-services/mental-health-services/where-to-get-urgent-help-for-mental-health/");
                        }
                        SupportCountry::Other => {
                            ui.label("Procure um serviço de escuta no seu país. Em emergência, use o número local de emergência.");
                            ui.hyperlink_to("Buscar apoio no seu país", "https://befrienders.org/");
                        }
                    }
                });
                ui.add_space(24.0);
                ui.separator();
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "O Estel ajusta a tela e oferece apoio de momento; os serviços acima podem ajudar quando você precisar de uma pessoa.",
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
    let (number, name) = title.split_once('/').unwrap_or(("00", title));
    ui.horizontal(|ui| {
        Frame::new()
            .fill(INK)
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::symmetric(9, 4))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(number.trim())
                        .font(poster_font(19.0))
                        .color(Color32::WHITE),
                );
            });
        ui.label(
            RichText::new(name.trim())
                .font(poster_font(27.0))
                .color(INK),
        );
    });
    ui.add_space(13.0);
}

fn toggle_sticker(ui: &mut egui::Ui, current: &mut bool, label: &str, fill: Color32) -> bool {
    let text = format!(
        "{}  /  {}",
        label,
        if *current { "ligado" } else { "desligado" }
    );
    let button = egui::Button::new(RichText::new(text).size(15.0).color(INK).strong())
        .fill(if *current { fill } else { Color32::WHITE })
        .stroke(Stroke::new(2.0_f32, INK))
        .corner_radius(CornerRadius::same(5))
        .min_size(Vec2::new(0.0, 39.0))
        .wrap();
    if ui.add(button).clicked() {
        *current = !*current;
        true
    } else {
        false
    }
}

fn time_row(ui: &mut egui::Ui, label: &str, h: &mut u32, m: &mut u32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).font(poster_font(19.0)).color(INK));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            Frame::new()
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.5_f32, INK))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(8, 5))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(egui::DragValue::new(m).range(0..=59).suffix(" min"))
                            .changed();
                        changed |= ui
                            .add(egui::DragValue::new(h).range(0..=23).suffix(" h"))
                            .changed();
                    });
                });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> SettingsApp {
        let cfg = Config {
            ambient_enabled: true,
            ambient_calibration: Some(AmbientCalibration {
                device_id: "camera-a".into(),
                camera_index: 0,
                dark: 0.2,
                bright: 0.6,
            }),
            ..Config::default()
        };
        let context = egui::Context::default();
        let texture = context.load_texture(
            "test",
            egui::ColorImage::new([1, 1], vec![Color32::WHITE]),
            egui::TextureOptions::LINEAR,
        );
        let (tx, _) = mpsc::channel();
        let (_, camera_scan) = mpsc::channel();
        SettingsApp {
            cfg,
            tx,
            wake_h: 7,
            wake_m: 0,
            bed_h: 23,
            bed_m: 0,
            dirty: false,
            last_edit: Instant::now(),
            status: String::new(),
            save_error: None,
            camera_names: vec!["Câmera A".into(), "Câmera B".into()],
            camera_error: None,
            camera_scan_pending: false,
            camera_scan,
            calibration_capture: None,
            calibration_dark: None,
            calibration_message: None,
            location_request: false,
            location_scan: None,
            location_error: None,
            location_success: false,
            place_query: String::new(),
            place_search: None,
            place_results: Vec::new(),
            place_error: None,
            selected_place: None,
            update_state: UpdateState::Current,
            mascot_sheet: None,
            poster_collage: texture,
            eye_break_until: None,
            eye_break_complete: false,
            grounding_open: false,
        }
    }

    fn finish(app: &mut SettingsApp, dark: bool, result: Result<AmbientSample, String>) {
        let (tx, rx) = mpsc::channel();
        tx.send(result).unwrap();
        app.calibration_capture = Some(CalibrationCapture {
            dark,
            camera_index: 0,
            rx,
        });
        app.poll_calibration();
    }

    #[test]
    fn failure_cancel_and_camera_roundtrip_preserve_saved_references() {
        let mut app = app();
        let previous = app.cfg.ambient_calibration.clone();
        finish(&mut app, true, Err("Câmera ocupada".into()));
        assert_eq!(app.cfg.ambient_calibration, previous);
        app.cancel_calibration_capture();
        assert_eq!(app.cfg.ambient_calibration, previous);
        app.cfg.ambient_camera_index = 1;
        assert!(!app.cfg.camera_is_calibrated());
        app.cfg.ambient_camera_index = 0;
        assert!(app.cfg.camera_is_calibrated());
        assert_eq!(app.cfg.ambient_calibration, previous);
        assert!(!app.dirty);
    }

    #[test]
    fn only_a_valid_completed_pair_replaces_saved_references() {
        let mut app = app();
        let previous = app.cfg.ambient_calibration.clone();
        finish(
            &mut app,
            true,
            Ok(AmbientSample {
                device_id: "camera-a".into(),
                luminance: 0.3,
            }),
        );
        assert_eq!(app.cfg.ambient_calibration, previous);
        finish(
            &mut app,
            false,
            Ok(AmbientSample {
                device_id: "camera-a".into(),
                luminance: 0.32,
            }),
        );
        assert_eq!(app.cfg.ambient_calibration, previous);
        finish(
            &mut app,
            false,
            Ok(AmbientSample {
                device_id: "camera-a".into(),
                luminance: 0.8,
            }),
        );
        let accepted = app.cfg.ambient_calibration.unwrap();
        assert_eq!((accepted.dark, accepted.bright), (0.3, 0.8));
        assert!(app.dirty);
    }

    #[test]
    fn changed_disabled_or_preserved_camera_discards_inflight_result() {
        for mode in 0..3 {
            let mut app = app();
            let previous = app.cfg.ambient_calibration.clone();
            match mode {
                0 => app.cfg.ambient_enabled = false,
                1 => app.cfg.ambient_camera_index = 1,
                _ => app.cfg.color_critical_work = true,
            }
            finish(
                &mut app,
                true,
                Ok(AmbientSample {
                    device_id: "camera-a".into(),
                    luminance: 0.3,
                }),
            );
            assert_eq!(app.cfg.ambient_calibration, previous);
            assert!(app.calibration_dark.is_none());
            assert!(app.calibration_message.unwrap().contains("descartada"));
        }
    }
}
