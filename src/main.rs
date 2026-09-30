#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, Timelike};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowThreadProcessId, MB_ICONERROR, MB_OK, MessageBoxW, SW_RESTORE,
    SetForegroundWindow, ShowWindow,
};
use windows::core::w;

use estel::ambient;
use estel::audio::Audio;
use estel::brightness;
use estel::config::Config;
use estel::display;
use estel::overlay;
use estel::schedule::DayContext;
use estel::session;
use estel::target::{NoiseColor, Target};
use estel::tray::{Autostart, Tray, TrayAction};
use estel::update;
use estel::weather::{self, Weather, WeatherPhase};

static SCREEN_LOCK: Mutex<()> = Mutex::new(());
const SETTINGS_STARTING: u32 = 1;

fn main() -> anyhow::Result<()> {
    init_log();
    let open_settings_on_start = std::env::args_os().any(|arg| arg == "--settings");
    if let Some(camera_index) = std::env::args()
        .skip_while(|arg| arg != "--sample-ambient")
        .nth(1)
    {
        let camera_index = camera_index
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("índice de câmera inválido"))?;
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }
        println!(
            "{}",
            serde_json::to_string(
                &ambient::sample_luminance(camera_index).map_err(anyhow::Error::msg)?
            )?
        );
        return Ok(());
    }
    if std::env::args_os().any(|arg| arg == "--list-cameras") {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }
        for camera in ambient::list_cameras().map_err(anyhow::Error::msg)? {
            println!("{camera}");
        }
        return Ok(());
    }
    if std::env::args_os().any(|arg| arg == "--settings-window") {
        let (tx, _rx) = mpsc::channel();
        return estel::ui::run(Config::load_or_default(), tx)
            .map_err(|error| anyhow::anyhow!(error.to_string()));
    }

    let (settings_event, config_event, _instance_mutex) = unsafe {
        let event = CreateEventW(None, false, false, w!("Local\\EstelOpenSettings"))?;
        let config_event = CreateEventW(None, false, false, w!("Local\\EstelConfigChanged"))?;
        let instance_mutex = CreateMutexW(None, false, w!("Local\\EstelSingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            SetEvent(event)?;
            tracing::info!("Estel já está em execução");
            return Ok(());
        }
        (event, config_event, instance_mutex)
    };

    let mut cfg = Config::load_or_default();
    tracing::info!(
        config = %Config::config_path().display(),
        tick_s = cfg.tick_seconds,
        "Estel iniciando",
    );

    let autostart: Option<Autostart> = match Autostart::new() {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!("início automático indisponível: {e}");
            None
        }
    };

    let tray = Tray::new(
        autostart.as_ref().is_some_and(|a| a.is_enabled()),
        cfg.intensity,
        cfg.noise_enabled,
        cfg.ambient_enabled,
    )?;
    let (update_tx, update_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = update_tx.send(update::check_latest());
    });
    tray.set_weather_status(if cfg.weather_enabled && !cfg.preserve_colors() {
        "Clima: consultando..."
    } else {
        "Clima: desligado"
    });
    if cfg.weather_enabled && !cfg.preserve_colors() {
        weather::publish_status(&cfg, WeatherPhase::Consulting);
    }
    if cfg.preserve_colors() {
        tray.set_ambient_status("Luz ambiente: pausada para preservar cores");
    }
    let overlay_hwnd = overlay::create()?;

    let mut screen_initialized = false;
    if session::is_dirty() || (cfg.display_enabled && !cfg.preserve_colors()) {
        let _ = display::init();
        let _ = brightness::init();
        screen_initialized = true;
        if cfg.display_enabled && !cfg.preserve_colors() {
            session::mark_dirty();
        } else {
            park_screen();
        }
    }

    let running = Arc::new(AtomicBool::new(true));
    let panic_running = running.clone();
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "falha interna no Estel");
        panic_running.store(false, Ordering::SeqCst);
        if let Ok(_screen_guard) = SCREEN_LOCK.try_lock() {
            restore_screen_unlocked();
        }
        orig_hook(info);
    }));

    let mut audio: Option<Audio> = None;

    let (cfg_tx, cfg_rx) = mpsc::channel::<Config>();
    let (ambient_cfg_tx, ambient_factor_rx) = ambient::start(cfg.clone());
    let (weather_cfg_tx, weather_rx) = start_weather(cfg.clone());
    let settings_open = Arc::new(AtomicU32::new(0));
    if open_settings_on_start {
        open_settings(cfg.clone(), cfg_tx.clone(), settings_open.clone());
    }

    {
        let running = running.clone();
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })?;
    }

    let mut paused = false;
    let mut user_quit = false;
    let mut preview_until: Option<Instant> = None;
    let mut ambient_brightness = None;
    let mut ambient_last_ok: Option<Instant> = None;
    let mut ambient_failed = false;
    let mut current_weather: Option<Weather> = None;
    let mut effects_active = false;
    let mut last_display_brightness: Option<(f32, Instant)> = None;
    let mut last_display_cct: Option<(f32, Instant)> = None;

    while running.load(Ordering::SeqCst) {
        if let Ok(Ok(Some(release))) = update_rx.try_recv() {
            tray.set_update_available(&release.version);
        }
        if let Some(incoming) = pending_config(config_event, &cfg_rx) {
            if brightness_controls_changed(&cfg, &incoming) {
                last_display_brightness = None;
                last_display_cct = None;
            }
            let weather_changed = apply_config_change(
                incoming,
                &mut cfg,
                &tray,
                &ambient_cfg_tx,
                &weather_cfg_tx,
                &mut ambient_brightness,
                &mut ambient_failed,
            );
            if ambient_brightness.is_none() {
                ambient_last_ok = None;
            }
            if weather_changed {
                current_weather = None;
                if cfg.weather_enabled && !cfg.preserve_colors() {
                    weather::publish_status(&cfg, WeatherPhase::Consulting);
                    tray.set_weather_status("Clima: consultando...");
                } else {
                    tray.set_weather_status("Clima: desligado");
                }
            }
        }
        while let Ok((latitude, longitude, result)) = weather_rx.try_recv() {
            if !cfg.weather_enabled
                || cfg.preserve_colors()
                || (latitude, longitude) != (cfg.latitude, cfg.longitude)
            {
                continue;
            }
            match result {
                Ok(value) => {
                    tracing::info!(cloud_pct = value.cloud_cover as u32, "clima atualizado");
                    current_weather = Some(value);
                    tray.set_weather_status("Clima: atualizado — apoio sem câmera");
                    weather::publish_status(
                        &cfg,
                        WeatherPhase::Ready(value.cloud_cover.round() as u8),
                    );
                }
                Err(error) => {
                    tracing::warn!(%error, "clima indisponível; mantendo ajuste por horário");
                    current_weather = None;
                    tray.set_weather_status("Clima: indisponível — brilho por horário");
                    weather::publish_status(&cfg, WeatherPhase::Unavailable);
                }
            }
        }
        while let Ok(reading) = ambient_factor_rx.try_recv() {
            if cfg.preserve_colors() {
                continue;
            }
            apply_ambient_reading(
                reading,
                &mut ambient_brightness,
                &mut ambient_failed,
                &mut ambient_last_ok,
            );
        }

        if cfg.preserve_colors() {
            tray.set_ambient_status("Luz ambiente: pausada para preservar cores");
        } else if cfg.ambient_enabled && !cfg.camera_is_calibrated() {
            tray.set_ambient_status("Luz ambiente: calibre a câmera no painel");
        } else if cfg.ambient_enabled && ambient_last_ok.is_none() && !ambient_failed {
            tray.set_ambient_status("Luz ambiente: aguardando câmera");
        } else {
            update_ambient_status(
                &tray,
                cfg.ambient_enabled,
                ambient_failed,
                cfg.weather_enabled && current_weather.is_some(),
            );
        }

        let now = Local::now();
        let now_min = now.hour() as f64 * 60.0 + now.minute() as f64 + now.second() as f64 / 60.0;

        let (sr_min, ss_min) = solar_times(cfg.latitude, cfg.longitude, now.date_naive());
        let ctx = DayContext {
            sunrise_min: sr_min,
            sunset_min: ss_min,
            wake_min: cfg.wake_min(),
            bed_min: cfg.bed_min(),
        };

        let scheduled = cfg.schedule.target_at(now_min, &ctx);
        let scheduled = estel::comfort::prepare_for_rest(scheduled, now_min, &ctx, &cfg);
        let mut target = scheduled.attenuate(cfg.intensity.factor());
        let brightness_ceiling = estel::comfort::brightness_ceiling(now_min, &ctx, &cfg);
        let preview = preview_until.is_some_and(|t| Instant::now() < t);
        if preview_until.is_some_and(|t| Instant::now() >= t) {
            preview_until = None;
            tracing::info!("prévia encerrada");
        }
        if preview {
            target = Target {
                cct_kelvin: 2400.0,
                brightness: 0.28,
                noise_gain: 1.0,
                noise: Some(NoiseColor::Pink),
            };
        } else {
            let fallback = current_weather
                .filter(|_| cfg.weather_enabled)
                .map(|value| weather::fallback_brightness(target.brightness, value, &cfg, now))
                .unwrap_or(target.brightness);
            target.brightness = brightness_with_sources(
                target.brightness,
                fallback,
                cfg.ambient_enabled,
                fresh_camera_brightness(ambient_brightness, ambient_last_ok, Instant::now()),
                now_min < sr_min
                    || now_min >= ss_min
                    || estel::comfort::is_rest_period(now_min, ctx.wake_min, ctx.bed_min),
            );
        }
        target.brightness = target
            .brightness
            .clamp(cfg.min_brightness, brightness_ceiling);

        if cfg.preserve_colors() || !cfg.display_enabled || (paused && !preview) || preview {
            last_display_brightness = None;
            last_display_cct = None;
        } else {
            let now = Instant::now();
            target.brightness =
                limit_brightness_change(last_display_brightness, target.brightness, now)
                    .max(cfg.min_brightness);
            target.cct_kelvin = limit_color_change(last_display_cct, target.cct_kelvin, now);
            last_display_brightness = Some((target.brightness, now));
            last_display_cct = Some((target.cct_kelvin, now));
        }

        if cfg.preserve_colors() {
            tray.set_tooltip("Estel · cores preservadas");
        } else if paused && !preview {
            tray.set_tooltip("Estel · pausada");
        } else if preview {
            tray.set_tooltip("Estel · prévia noturna");
        } else if cfg.ambient_enabled && ambient_failed {
            tray.set_tooltip("Estel · câmera indisponível");
        } else {
            tray.set_tooltip(&format!(
                "Estel · {} K · {}",
                target.cct_kelvin as u32,
                cfg.intensity.label(),
            ));
        }

        tracing::info!(
            cct = target.cct_kelvin as u32,
            brilho_pct = (target.brightness * 100.0) as u32,
            ruido = ?target.noise,
            preview,
            "tick"
        );

        if cfg.preserve_colors() || !cfg.display_enabled {
            retry_park(&mut effects_active, session::is_dirty(), park_screen);
            overlay::hide(overlay_hwnd);
        } else if paused && !preview {
            retry_park(&mut effects_active, session::is_dirty(), park_screen);
        } else {
            let _screen_guard = SCREEN_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if !running.load(Ordering::SeqCst) {
                break;
            }
            if !screen_initialized {
                let _ = display::init();
                let _ = brightness::init();
                screen_initialized = true;
            }
            if !effects_active {
                session::mark_dirty();
            }
            let display_started = Instant::now();
            let gamma_active =
                match display::apply(&target, cfg.gamma_warm_floor_k, cfg.min_brightness) {
                    Ok(true) => {
                        tracing::debug!(cct = target.cct_kelvin as u32, "gamma ok");
                        true
                    }
                    Ok(false) => {
                        tracing::debug!(
                            cct = target.cct_kelvin as u32,
                            "gamma recusada ou ausente"
                        );
                        false
                    }
                    Err(e) => {
                        tracing::error!("display::apply: {e}");
                        false
                    }
                };
            if display_started.elapsed() > Duration::from_millis(250) {
                tracing::warn!(
                    elapsed_ms = display_started.elapsed().as_millis(),
                    "ajuste de cor demorou"
                );
            }
            let brightness_started = Instant::now();
            let ddc_active = brightness::apply(target.brightness);
            overlay::update(
                overlay_hwnd,
                target.cct_kelvin,
                target.brightness,
                ddc_active,
                gamma_active,
            );
            effects_active = true;
            if brightness_started.elapsed() > Duration::from_millis(250) {
                tracing::warn!(
                    elapsed_ms = brightness_started.elapsed().as_millis(),
                    "ajuste de brilho demorou"
                );
            }
        }

        let wants_audio = requested_audio_target(&target, &cfg, paused, preview).is_some();
        if wants_audio && audio.is_none() {
            audio = Audio::try_new();
        }
        tray.set_noise_status(if !cfg.noise_enabled {
            "Ruído: desligado"
        } else if paused {
            "Ruído: pausado"
        } else if cfg.max_volume <= 0.0 {
            "Ruído: nível em 0%"
        } else if wants_audio && audio.is_none() {
            "Ruído: sem saída de áudio; confira o Windows"
        } else if wants_audio && preview {
            "Ruído: prévia em reprodução"
        } else if wants_audio {
            "Ruído: reproduzindo à noite"
        } else {
            "Ruído: programado para a noite"
        });
        tick_audio(&mut audio, &target, &cfg, paused, preview);

        let tick = Duration::from_secs(cfg.tick_seconds.max(5));
        let step = Duration::from_millis(50);
        let mut elapsed = Duration::ZERO;
        let mut kick = false;

        while elapsed < tick && running.load(Ordering::SeqCst) && !kick {
            overlay::pump_messages();

            if unsafe { WaitForSingleObject(settings_event, 0) } == WAIT_OBJECT_0 {
                open_settings(cfg.clone(), cfg_tx.clone(), settings_open.clone());
            }

            if let Some(action) = tray.poll() {
                match action {
                    TrayAction::Quit => {
                        user_quit = true;
                        running.store(false, Ordering::SeqCst);
                    }
                    TrayAction::TogglePause => {
                        paused = !paused;
                        tray.set_paused(paused);
                        if paused {
                            effects_active = !park_screen();
                            overlay::hide(overlay_hwnd);
                            if let Some(ref mut aud) = audio {
                                aud.silence();
                            }
                            tracing::info!("pausada");
                        } else {
                            tracing::info!("retomada");
                        }
                        kick = true;
                    }
                    TrayAction::ToggleAutostart => {
                        if let Some(ref a) = autostart {
                            match a.toggle() {
                                Ok(enabled) => {
                                    tray.set_autostart(enabled);
                                    tracing::info!(enabled, "início automático");
                                }
                                Err(e) => {
                                    tray.set_autostart(a.is_enabled());
                                    tracing::error!(
                                        "não foi possível alterar o início automático: {e}"
                                    );
                                    show_error(w!(
                                        "O Windows não permitiu alterar o início automático."
                                    ));
                                }
                            }
                        }
                    }
                    TrayAction::ToggleNoise => {
                        cfg.noise_enabled = !cfg.noise_enabled;
                        tray.set_noise(cfg.noise_enabled);
                        persist(&cfg);
                        kick = true;
                    }
                    TrayAction::PreviewNight => {
                        preview_until = Some(Instant::now() + Duration::from_secs(20));
                        tracing::info!("prévia noturna — 20 s de tela quente e som, se ativado");
                        kick = true;
                    }
                    TrayAction::OpenSettings => {
                        open_settings(cfg.clone(), cfg_tx.clone(), settings_open.clone());
                    }
                    TrayAction::CheckUpdates => {
                        open_settings(cfg.clone(), cfg_tx.clone(), settings_open.clone());
                    }
                    TrayAction::SetIntensity(level) => {
                        cfg.intensity = level;
                        last_display_brightness = None;
                        last_display_cct = None;
                        tray.set_intensity(level);
                        persist(&cfg);
                        tracing::info!(level = level.label(), "intensidade");
                        kick = true;
                    }
                }
            }

            if let Some(incoming) = pending_config(config_event, &cfg_rx) {
                if brightness_controls_changed(&cfg, &incoming) {
                    last_display_brightness = None;
                    last_display_cct = None;
                }
                let weather_changed = apply_config_change(
                    incoming,
                    &mut cfg,
                    &tray,
                    &ambient_cfg_tx,
                    &weather_cfg_tx,
                    &mut ambient_brightness,
                    &mut ambient_failed,
                );
                if ambient_brightness.is_none() {
                    ambient_last_ok = None;
                }
                if weather_changed {
                    current_weather = None;
                    tray.set_weather_status(if cfg.weather_enabled && !cfg.preserve_colors() {
                        "Clima: consultando..."
                    } else {
                        "Clima: desligado"
                    });
                    if cfg.weather_enabled && !cfg.preserve_colors() {
                        weather::publish_status(&cfg, WeatherPhase::Consulting);
                    }
                }
                kick = true;
            }
            while let Ok((latitude, longitude, result)) = weather_rx.try_recv() {
                if !cfg.weather_enabled
                    || cfg.preserve_colors()
                    || (latitude, longitude) != (cfg.latitude, cfg.longitude)
                {
                    continue;
                }
                match result {
                    Ok(value) => {
                        tracing::info!(cloud_pct = value.cloud_cover as u32, "clima atualizado");
                        current_weather = Some(value);
                        tray.set_weather_status("Clima: atualizado — apoio sem câmera");
                        weather::publish_status(
                            &cfg,
                            WeatherPhase::Ready(value.cloud_cover.round() as u8),
                        );
                    }
                    Err(error) => {
                        tracing::warn!(%error, "clima indisponível; mantendo ajuste por horário");
                        current_weather = None;
                        tray.set_weather_status("Clima: indisponível — brilho por horário");
                        weather::publish_status(&cfg, WeatherPhase::Unavailable);
                    }
                }
                kick = true;
            }

            while let Ok(reading) = ambient_factor_rx.try_recv() {
                if cfg.preserve_colors() {
                    continue;
                }
                apply_ambient_reading(
                    reading,
                    &mut ambient_brightness,
                    &mut ambient_failed,
                    &mut ambient_last_ok,
                );
                kick = true;
            }

            tick_audio(&mut audio, &target, &cfg, paused, preview);

            std::thread::sleep(step);
            elapsed += step;
        }
    }

    if restore_screen() {
        tracing::info!("Estel encerrado — monitor restaurado");
    } else {
        tracing::warn!("Estel encerrado — restauração do monitor pendente");
        if user_quit {
            show_error(w!(
                "Não foi possível restaurar completamente a tela. Se o brilho estiver alterado, ajuste-o pelos botões do monitor. Ao abrir o Estel novamente, ele tentará recuperar o ajuste anterior."
            ));
        }
    }
    Ok(())
}

fn park_screen() -> bool {
    let _screen_guard = SCREEN_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let gamma = display::park();
    let backlight = brightness::park();
    if gamma && backlight {
        session::mark_clean();
        true
    } else {
        tracing::warn!("restauração da tela incompleta; mantendo dados para recuperação");
        false
    }
}

fn retry_park(active: &mut bool, dirty: bool, park: impl FnOnce() -> bool) {
    if *active || dirty {
        *active = !park();
    }
}

fn restore_screen() -> bool {
    let _screen_guard = SCREEN_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    restore_screen_unlocked()
}

fn restore_screen_unlocked() -> bool {
    let gamma = display::restore();
    let backlight = brightness::restore();
    if gamma && backlight {
        session::mark_clean();
        true
    } else {
        tracing::warn!("restauração da tela incompleta; mantendo dados para recuperação");
        false
    }
}

fn apply_ambient_reading(
    reading: Result<f32, String>,
    brightness: &mut Option<f32>,
    failed: &mut bool,
    last_ok: &mut Option<Instant>,
) {
    match reading {
        Ok(value) if value.is_finite() && (0.0..=1.0).contains(&value) => {
            *brightness = Some(value.clamp(0.0, 1.0));
            *failed = false;
            *last_ok = Some(Instant::now());
        }
        Ok(_) | Err(_) => {
            *brightness = None;
            *last_ok = None;
            *failed = true;
        }
    }
}

fn fresh_camera_brightness(
    brightness: Option<f32>,
    last_ok: Option<Instant>,
    now: Instant,
) -> Option<f32> {
    last_ok
        .filter(|last| now.saturating_duration_since(*last) <= Duration::from_secs(5 * 60))
        .and(brightness)
}

fn limit_brightness_change(previous: Option<(f32, Instant)>, desired: f32, now: Instant) -> f32 {
    let Some((last, at)) = previous else {
        return desired;
    };
    let max_change = (now.saturating_duration_since(at).as_secs_f32() * 0.003).min(0.06);
    last + (desired - last).clamp(-max_change, max_change)
}

fn limit_color_change(previous: Option<(f32, Instant)>, desired: f32, now: Instant) -> f32 {
    let Some((last, at)) = previous else {
        return desired;
    };
    let max_mired_change = (now.saturating_duration_since(at).as_secs_f32() * 0.4).min(10.0);
    let last_mired = 1_000_000.0 / last.max(1_000.0);
    let desired_mired = 1_000_000.0 / desired.max(1_000.0);
    let next_mired =
        last_mired + (desired_mired - last_mired).clamp(-max_mired_change, max_mired_change);
    1_000_000.0 / next_mired
}

fn brightness_with_ambient(scheduled: f32, enabled: bool, measured: Option<f32>) -> f32 {
    if !enabled {
        return scheduled;
    }
    measured
        .map(|value| scheduled + (value - scheduled) * 0.35)
        .unwrap_or(scheduled)
}

fn brightness_with_sources(
    scheduled: f32,
    weather: f32,
    camera_enabled: bool,
    camera: Option<f32>,
    night: bool,
) -> f32 {
    if camera_enabled && camera.is_some() {
        let adjusted = brightness_with_ambient(scheduled, true, camera);
        if night {
            adjusted.min(scheduled)
        } else {
            adjusted
        }
    } else {
        if night {
            weather.min(scheduled)
        } else {
            weather
        }
    }
}

fn pending_config(event: HANDLE, rx: &mpsc::Receiver<Config>) -> Option<Config> {
    let mut latest = rx.try_iter().last();
    if unsafe { WaitForSingleObject(event, 0) } == WAIT_OBJECT_0 {
        latest = Some(Config::load_or_default());
    }
    latest
}

fn apply_config_change(
    incoming: Config,
    cfg: &mut Config,
    tray: &Tray,
    ambient_cfg_tx: &mpsc::Sender<Config>,
    weather_cfg_tx: &mpsc::Sender<Config>,
    ambient_brightness: &mut Option<f32>,
    ambient_failed: &mut bool,
) -> bool {
    let camera_changed = ambient_source_changed(cfg, &incoming);
    let camera_worker_changed = ambient_worker_changed(cfg, &incoming);
    let weather_changed = weather_source_changed(cfg, &incoming);
    *cfg = incoming;
    tray.set_intensity(cfg.intensity);
    tray.set_noise(cfg.noise_enabled);
    if camera_changed {
        *ambient_brightness = None;
        *ambient_failed = false;
        tray.set_ambient_status(if cfg.preserve_colors() {
            "Luz ambiente: pausada para preservar cores"
        } else if cfg.ambient_enabled {
            "Luz ambiente: aguardando câmera"
        } else {
            "Luz ambiente: desligada"
        });
    }
    if camera_worker_changed && ambient_cfg_tx.send(cfg.clone()).is_err() {
        tracing::error!("sensor de luz ambiente encerrou inesperadamente");
    }
    if weather_changed && weather_cfg_tx.send(cfg.clone()).is_err() {
        tracing::error!("consulta de clima encerrou inesperadamente");
    }
    tracing::info!(
        ambient_enabled = cfg.ambient_enabled,
        interval_s = cfg.ambient_sample_interval_seconds,
        "configuração aplicada"
    );
    weather_changed
}

fn brightness_controls_changed(old: &Config, new: &Config) -> bool {
    old.intensity != new.intensity
        || old.display_enabled != new.display_enabled
        || old.ambient_enabled != new.ambient_enabled
        || old.ambient_camera_index != new.ambient_camera_index
        || old.ambient_brightness_min != new.ambient_brightness_min
        || old.ambient_brightness_max != new.ambient_brightness_max
        || old.min_brightness != new.min_brightness
        || old.day_brightness_max != new.day_brightness_max
        || old.rest_brightness_max != new.rest_brightness_max
        || old.wake != new.wake
        || old.bed != new.bed
        || old.weather_enabled != new.weather_enabled
        || ((old.latitude != new.latitude || old.longitude != new.longitude) && !new.location_auto)
        || old.window_near != new.window_near
        || old.window_azimuth_deg != new.window_azimuth_deg
        || old.screen_window_relation != new.screen_window_relation
        || old.preserve_colors() != new.preserve_colors()
}

fn ambient_source_changed(old: &Config, new: &Config) -> bool {
    old.ambient_enabled != new.ambient_enabled
        || old.ambient_calibration != new.ambient_calibration
        || old.ambient_camera_index != new.ambient_camera_index
        || old.ambient_brightness_min != new.ambient_brightness_min
        || old.ambient_brightness_max != new.ambient_brightness_max
        || old.preserve_colors() != new.preserve_colors()
}

fn ambient_worker_changed(old: &Config, new: &Config) -> bool {
    ambient_source_changed(old, new)
        || old.ambient_sample_interval_seconds != new.ambient_sample_interval_seconds
}

fn weather_source_changed(old: &Config, new: &Config) -> bool {
    old.weather_enabled != new.weather_enabled
        || old.latitude != new.latitude
        || old.longitude != new.longitude
        || old.preserve_colors() != new.preserve_colors()
}

type WeatherReading = (f64, f64, Result<Weather, String>);

fn start_weather(initial: Config) -> (mpsc::Sender<Config>, mpsc::Receiver<WeatherReading>) {
    let (config_tx, config_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("estel-weather".into())
        .spawn(move || {
            let mut config = initial;
            let mut cached: Option<(f64, f64, Instant, Result<Weather, String>)> = None;
            loop {
                if config.weather_enabled && !config.preserve_colors() {
                    let current = cached
                        .as_ref()
                        .filter(|(latitude, longitude, fetched_at, _)| {
                            (*latitude, *longitude) == (config.latitude, config.longitude)
                                && fetched_at.elapsed() < Duration::from_secs(900)
                        });
                    let result = match current {
                        Some((_, _, _, result)) => result.clone(),
                        None => {
                            let result =
                                weather::current_weather(config.latitude, config.longitude);
                            cached = Some((
                                config.latitude,
                                config.longitude,
                                Instant::now(),
                                result.clone(),
                            ));
                            result
                        }
                    };
                    if result_tx
                        .send((config.latitude, config.longitude, result))
                        .is_err()
                    {
                        return;
                    }
                }
                let delay = cached
                    .as_ref()
                    .map(|(_, _, fetched_at, _)| {
                        Duration::from_secs(900).saturating_sub(fetched_at.elapsed())
                    })
                    .filter(|_| config.weather_enabled && !config.preserve_colors())
                    .unwrap_or(Duration::from_secs(900));
                match config_rx.recv_timeout(delay) {
                    Ok(next) => config = next,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .expect("não foi possível iniciar a consulta de clima");
    (config_tx, result_rx)
}

fn requested_audio_target(
    target: &Target,
    cfg: &Config,
    paused: bool,
    preview: bool,
) -> Option<(NoiseColor, f32)> {
    if paused || !cfg.noise_enabled || cfg.max_volume <= 0.0 {
        return None;
    }
    let gain = if preview { 1.0 } else { target.noise_gain };
    target
        .noise
        .filter(|_| gain > 0.0)
        .map(|color| (color, gain))
}

fn tick_audio(
    audio: &mut Option<Audio>,
    target: &Target,
    cfg: &Config,
    paused: bool,
    preview: bool,
) {
    let requested = requested_audio_target(target, cfg, paused, preview);
    if let Some(active) = audio.as_mut() {
        match requested {
            Some((color, gain)) => active.tick(Some(color), gain, cfg.max_volume),
            None => active.tick(None, 0.0, cfg.max_volume),
        }
    }
    if requested.is_none() && audio.as_ref().is_some_and(Audio::is_silent) {
        *audio = None;
    }
}

fn ambient_status_text(enabled: bool, failed: bool, weather_active: bool) -> &'static str {
    if !enabled {
        "Luz ambiente: desligada"
    } else if failed && weather_active {
        "Câmera indisponível — clima e horário"
    } else if failed {
        "Câmera indisponível — brilho por horário"
    } else {
        "Luz ambiente: ativa"
    }
}

fn update_ambient_status(tray: &Tray, enabled: bool, failed: bool, weather_active: bool) {
    tray.set_ambient_status(ambient_status_text(enabled, failed, weather_active));
}

fn show_error(message: windows::core::PCWSTR) {
    unsafe {
        let _ = MessageBoxW(None, message, w!("Estel"), MB_OK | MB_ICONERROR);
    }
}

fn persist(cfg: &Config) {
    match cfg.save(&Config::config_path()) {
        Ok(()) => tracing::info!("configuração salva"),
        Err(e) => tracing::error!("não foi possível salvar a configuração: {e}"),
    }
}

fn open_settings(_cfg: Config, tx: mpsc::Sender<Config>, state: Arc<AtomicU32>) {
    if let Err(pid) =
        state.compare_exchange(0, SETTINGS_STARTING, Ordering::SeqCst, Ordering::SeqCst)
    {
        if pid != SETTINGS_STARTING {
            let mut previous = None;
            for _ in 0..64 {
                let Ok(window) = (unsafe { FindWindowExW(None, previous, None, w!("Estel")) })
                else {
                    break;
                };
                let mut window_pid = 0;
                unsafe { GetWindowThreadProcessId(window, Some(&mut window_pid)) };
                if window_pid == pid {
                    unsafe {
                        let _ = ShowWindow(window, SW_RESTORE);
                        if !SetForegroundWindow(window).as_bool() {
                            tracing::warn!("não foi possível trazer a janela do Estel para frente");
                        }
                    }
                    break;
                }
                previous = Some(window);
            }
        }
        return;
    }
    std::thread::spawn(move || {
        let result = std::env::current_exe()
            .and_then(|executable| {
                std::process::Command::new(executable)
                    .arg("--settings-window")
                    .spawn()
            })
            .and_then(|mut child| {
                state.store(child.id(), Ordering::SeqCst);
                child.wait()
            });
        state.store(0, Ordering::SeqCst);
        if let Err(error) = result {
            tracing::error!("janela de configurações: {error}");
            show_error(w!("Não foi possível abrir as configurações."));
        }
        let _ = tx.send(Config::load_or_default());
    });
}

fn init_log() {
    let dir = Config::config_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let log_path = dir.join("estel.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path);

    let env = tracing_subscriber::EnvFilter::from_default_env().add_directive(
        "estel=info"
            .parse()
            .unwrap_or_else(|_| "info".parse().unwrap()),
    );

    match file {
        Ok(f) => {
            tracing_subscriber::fmt()
                .with_writer(std::sync::Mutex::new(f))
                .with_env_filter(env)
                .init();
        }
        Err(_) => {
            tracing_subscriber::fmt().with_env_filter(env).init();
        }
    }
}

fn solar_times(lat: f64, lon: f64, date: NaiveDate) -> (f64, f64) {
    use sunrise::{Coordinates, SolarDay, SolarEvent};
    let fallback = (6.0 * 60.0, 18.0 * 60.0);
    let coords = match Coordinates::new(lat, lon) {
        Some(c) => c,
        None => {
            tracing::warn!(lat, lon, "coordenadas inválidas — usando 06:00/18:00");
            return fallback;
        }
    };
    let day = SolarDay::new(coords, date);
    let to_min = |dt: chrono::DateTime<chrono::Utc>| {
        let local = dt.with_timezone(&Local);
        local.hour() as f64 * 60.0 + local.minute() as f64
    };
    let sr = day
        .event_time(SolarEvent::Sunrise)
        .map(to_min)
        .unwrap_or(fallback.0);
    let ss = day
        .event_time(SolarEvent::Sunset)
        .map(to_min)
        .unwrap_or(fallback.1);
    (sr, ss)
}

#[cfg(test)]
mod tests {
    use super::{
        ambient_source_changed, ambient_status_text, ambient_worker_changed, apply_ambient_reading,
        brightness_controls_changed, brightness_with_ambient, brightness_with_sources,
        fresh_camera_brightness, limit_brightness_change, limit_color_change,
        requested_audio_target, retry_park, weather_source_changed,
    };
    use estel::Config;
    use estel::target::{NoiseColor, Target};
    use std::time::{Duration, Instant};

    #[test]
    fn rejected_camera_reading_immediately_returns_to_schedule() {
        let mut brightness = Some(0.65);
        let mut failed = false;
        let now = Instant::now();
        let mut last_ok = Some(now);
        apply_ambient_reading(
            Err("câmera indisponível".into()),
            &mut brightness,
            &mut failed,
            &mut last_ok,
        );
        assert_eq!(brightness, None);
        assert_eq!(
            brightness_with_ambient(
                0.2,
                true,
                fresh_camera_brightness(brightness, last_ok, now + Duration::from_secs(120)),
            ),
            0.2
        );
        assert_eq!(
            brightness_with_ambient(
                0.2,
                true,
                fresh_camera_brightness(brightness, last_ok, now + Duration::from_secs(301)),
            ),
            0.2
        );
        assert!(failed);

        apply_ambient_reading(Ok(0.8), &mut brightness, &mut failed, &mut last_ok);
        assert!((brightness_with_ambient(0.2, true, brightness) - 0.41).abs() < 0.0001);
        assert_eq!(brightness_with_ambient(0.2, false, brightness), 0.2);
        assert!(!failed);
        apply_ambient_reading(Ok(f32::NAN), &mut brightness, &mut failed, &mut last_ok);
        assert_eq!(brightness, None);
        assert!(failed);
    }

    #[test]
    fn camera_brightness_gently_adjusts_night_schedule() {
        assert!((brightness_with_ambient(0.16, true, Some(1.0)) - 0.454).abs() < 0.0001);
        assert!((brightness_with_ambient(0.16, true, Some(0.35)) - 0.2265).abs() < 0.0001);
    }

    #[test]
    fn weather_does_not_brighten_protected_schedule() {
        assert_eq!(brightness_with_sources(0.25, 0.5, true, None, true), 0.25);
        assert_eq!(brightness_with_sources(0.25, 0.5, false, None, true), 0.25);
    }

    #[test]
    fn camera_does_not_brighten_after_sunset() {
        assert_eq!(
            brightness_with_sources(0.22, 0.22, true, Some(1.0), true),
            0.22
        );
        assert_eq!(
            brightness_with_sources(0.50, 0.50, true, Some(0.35), true),
            0.4475
        );
    }

    #[test]
    fn weather_is_only_used_without_a_camera_reading() {
        assert!((brightness_with_sources(0.5, 0.7, true, Some(0.8), false) - 0.605).abs() < 0.0001);
        assert_eq!(brightness_with_sources(0.5, 0.7, true, None, false), 0.7);
        assert_eq!(
            brightness_with_sources(0.5, 0.7, false, Some(0.8), false),
            0.7
        );
    }

    #[test]
    fn disabled_noise_does_not_open_an_audio_device() {
        let target = Target {
            noise: Some(NoiseColor::Pink),
            noise_gain: 0.5,
            ..Target::neutral()
        };
        let mut cfg = Config::default();
        assert!(requested_audio_target(&target, &cfg, false, true).is_none());
        cfg.noise_enabled = true;
        cfg.max_volume = 0.0;
        assert!(requested_audio_target(&target, &cfg, false, true).is_none());
        cfg.max_volume = 0.35;
        assert!(requested_audio_target(&target, &cfg, true, true).is_none());
        assert_eq!(
            requested_audio_target(&target, &cfg, false, true),
            Some((NoiseColor::Pink, 1.0))
        );
        assert_eq!(
            requested_audio_target(&target, &cfg, false, false),
            Some((NoiseColor::Pink, 0.5))
        );
    }

    #[test]
    fn brightness_change_is_bounded_after_camera_discontinuity() {
        let now = Instant::now();
        assert_eq!(limit_brightness_change(None, 1.0, now), 1.0);
        let after_ten_seconds =
            limit_brightness_change(Some((0.3, now)), 1.0, now + Duration::from_secs(10));
        assert!((after_ten_seconds - 0.33).abs() < 0.001);
        let after_long_pause =
            limit_brightness_change(Some((0.3, now)), 1.0, now + Duration::from_secs(3600));
        assert!((after_long_pause - 0.36).abs() < 0.001);
    }

    #[test]
    fn automatic_color_change_is_bounded_in_mired() {
        let now = Instant::now();
        let next = limit_color_change(Some((6500.0, now)), 2400.0, now + Duration::from_secs(10));
        let mired_change: f32 = 1_000_000.0 / next - 1_000_000.0 / 6500.0;
        assert!((mired_change - 4.0).abs() < 0.01);
        assert_eq!(limit_color_change(None, 2400.0, now), 2400.0);
    }

    #[test]
    fn explicit_intensity_change_bypasses_camera_slew_limit() {
        let current = Config::default();
        let mut volume_only = current.clone();
        volume_only.max_volume = 0.1;
        assert!(!brightness_controls_changed(&current, &volume_only));

        let mut intensity = current.clone();
        intensity.intensity = estel::config::Intensity::Suave;
        assert!(brightness_controls_changed(&current, &intensity));
        intensity = current.clone();
        intensity.ambient_enabled = !current.ambient_enabled;
        assert!(brightness_controls_changed(&current, &intensity));
        intensity = current.clone();
        intensity.weather_enabled = !current.weather_enabled;
        assert!(brightness_controls_changed(&current, &intensity));
        intensity = current.clone();
        intensity.window_near = !current.window_near;
        assert!(brightness_controls_changed(&current, &intensity));

        let mut automatic_location = current.clone();
        automatic_location.location_auto = true;
        let mut refreshed_location = automatic_location.clone();
        refreshed_location.latitude += 0.0001;
        assert!(!brightness_controls_changed(
            &automatic_location,
            &refreshed_location
        ));
        refreshed_location.location_auto = false;
        assert!(brightness_controls_changed(
            &automatic_location,
            &refreshed_location
        ));
    }

    #[test]
    fn camera_status_names_weather_or_schedule_fallback() {
        assert_eq!(
            ambient_status_text(true, true, true),
            "Câmera indisponível — clima e horário"
        );
        assert_eq!(
            ambient_status_text(true, true, false),
            "Câmera indisponível — brilho por horário"
        );
    }

    #[test]
    fn volume_change_keeps_camera_and_weather_readings() {
        let current = Config::default();
        let mut next = current.clone();
        next.max_volume = 0.5;
        assert!(!ambient_source_changed(&current, &next));
        assert!(!weather_source_changed(&current, &next));

        next.ambient_camera_index = 1;
        assert!(ambient_source_changed(&current, &next));
        next.ambient_camera_index = current.ambient_camera_index;
        next.latitude += 1.0;
        assert!(weather_source_changed(&current, &next));

        next = current.clone();
        next.color_critical_work = true;
        assert!(ambient_source_changed(&current, &next));
        assert!(weather_source_changed(&current, &next));

        next = current.clone();
        next.ambient_sample_interval_seconds = 10;
        assert!(!ambient_source_changed(&current, &next));
        assert!(ambient_worker_changed(&current, &next));
    }

    #[test]
    fn failed_screen_restore_retries_while_dirty() {
        let mut active = false;
        let mut attempts = 0;
        retry_park(&mut active, true, || {
            attempts += 1;
            false
        });
        assert!(active);
        retry_park(&mut active, true, || {
            attempts += 1;
            true
        });
        assert!(!active);
        retry_park(&mut active, false, || {
            attempts += 1;
            false
        });
        assert_eq!(attempts, 2);
    }
}
