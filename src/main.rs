#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{
    Arc,
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
use estel::config::Config;
use estel::hardware_worker::{HardwareClient, HardwareStatus};
use estel::overlay;
use estel::runtime::{ActivityMonitor, WakeSignal, wait_for_work};
use estel::schedule::DayContext;
use estel::target::{NoiseColor, Target};
use estel::tray::{Autostart, Tray, TrayAction};
use estel::update;
use estel::weather::{self, Weather, WeatherPhase};

const SETTINGS_STARTING: u32 = 1;

fn main() -> anyhow::Result<()> {
    let result = run();
    if let Err(error) = &result {
        tracing::error!(%error, "Estel não conseguiu iniciar");
        if !std::env::args_os().any(|arg| {
            arg == "--list-cameras"
                || arg == "--list-camera-devices"
                || arg == "--sample-ambient"
                || arg == "--diagnostics"
                || arg == "--display-worker"
                || arg == "--display-diagnostics"
                || arg == "--sample-light-sensor"
                || arg == "--quit"
        }) {
            show_error(w!(
                "O Estel não conseguiu iniciar. Suas configurações foram preservadas. Confira estel.log na pasta de configuração e tente abrir novamente."
            ));
        }
    }
    result
}

fn run() -> anyhow::Result<()> {
    init_log();
    if std::env::args_os().any(|arg| arg == "--display-worker") {
        return estel::hardware_worker::run_stdio();
    }
    if std::env::args_os().any(|arg| arg == "--display-diagnostics") {
        println!(
            "{}",
            serde_json::to_string(&estel::hardware_worker::inspect()?)?
        );
        return Ok(());
    }
    if std::env::args_os().any(|arg| arg == "--quit") {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW};
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\EstelQuit"))? };
        let result = unsafe { SetEvent(event) };
        unsafe { CloseHandle(event)? };
        result?;
        return Ok(());
    }
    if std::env::args_os().any(|arg| arg == "--sample-light-sensor") {
        println!(
            "{}",
            serde_json::to_string(&ambient::sample_light_sensor().map_err(anyhow::Error::msg)?)?
        );
        return Ok(());
    }
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
        let camera_id = std::env::args()
            .skip_while(|arg| arg != "--camera-id")
            .nth(1);
        if camera_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 4096)
        {
            anyhow::bail!("identificador de câmera inválido");
        }
        println!(
            "{}",
            serde_json::to_string(
                &ambient::sample_luminance(camera_index, camera_id.as_deref())
                    .map_err(anyhow::Error::msg)?
            )?
        );
        return Ok(());
    }
    if std::env::args_os().any(|arg| arg == "--list-cameras" || arg == "--list-camera-devices") {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }
        if std::env::args_os().any(|arg| arg == "--list-camera-devices") {
            println!(
                "{}",
                serde_json::to_string(
                    &ambient::list_camera_devices().map_err(anyhow::Error::msg)?
                )?
            );
            return Ok(());
        }
        for camera in ambient::list_cameras().map_err(anyhow::Error::msg)? {
            println!("{camera}");
        }
        return Ok(());
    }
    if std::env::args_os().any(|arg| arg == "--settings-window") {
        let (tx, _rx) = mpsc::channel();
        return estel::ui::run(Config::load_or_default()?, tx)
            .map_err(|error| anyhow::anyhow!(error.to_string()));
    }

    if std::env::args_os().any(|arg| arg == "--diagnostics") {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "executable": std::env::current_exe()?,
                "config_path": Config::config_path(),
                "config": Config::load_or_default()?,
                "autostart_enabled": Autostart::new()?.is_enabled(),
                "cameras": ambient::list_cameras().map_err(anyhow::Error::msg)?,
            }))?
        );
        return Ok(());
    }

    let (settings_event, config_event, quit_event, _instance_mutex) = unsafe {
        let event = CreateEventW(None, false, false, w!("Local\\EstelOpenSettings"))?;
        let config_event = CreateEventW(None, false, false, w!("Local\\EstelConfigChanged"))?;
        let instance_mutex = CreateMutexW(None, false, w!("Local\\EstelSingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            SetEvent(event)?;
            tracing::info!("Estel já está em execução");
            return Ok(());
        }
        let quit_event = CreateEventW(None, false, false, w!("Local\\EstelQuit"))?;
        (event, config_event, quit_event, instance_mutex)
    };

    let wake = WakeSignal::new()?;
    let mut activity = ActivityMonitor::new()?;

    let mut cfg = Config::load_or_default()?;
    tracing::info!(
        config = %Config::config_path().display(),
        tick_s = cfg.tick_seconds,
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        executable = %std::env::current_exe()?.display(),
        "Estel iniciando",
    );

    let autostart: Option<Autostart> = match Autostart::new() {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!("início automático indisponível: {e}");
            None
        }
    };

    if let Some(autostart) = &autostart {
        match autostart.synchronize(cfg.start_with_windows) {
            Ok(enabled) => {
                if cfg.start_with_windows != Some(enabled) {
                    let previous = cfg.clone();
                    cfg.start_with_windows = Some(enabled);
                    cfg = cfg.save_changes(&previous, &Config::config_path())?;
                }
                tracing::info!(
                    enabled,
                    startup = std::env::args_os().any(|arg| arg == "--startup"),
                    "início automático verificado"
                );
            }
            Err(error) => tracing::error!(%error, "não foi possível reparar o início automático"),
        }
    }

    let tray = Tray::new(
        autostart.as_ref().is_some_and(|a| a.is_enabled()),
        cfg.intensity,
        cfg.noise_enabled,
        cfg.ambient_enabled,
    )?;
    let (update_tx, update_rx) = mpsc::channel();
    let update_wake = wake.clone();
    std::thread::spawn(move || {
        let _ = update_tx.send(update::check_latest());
        update_wake.notify();
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

    let mut hardware = HardwareClient::start(wake.clone())?;
    let mut display_control = DisplayControl::new();
    drive_hardware(
        &mut display_control,
        &mut hardware,
        false,
        &Target::neutral(),
        &cfg,
        Instant::now(),
    );

    let running = Arc::new(AtomicBool::new(true));
    let panic_running = running.clone();
    let panic_wake = wake.clone();
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "falha interna no Estel");
        panic_running.store(false, Ordering::SeqCst);
        panic_wake.notify();
        orig_hook(info);
    }));

    let mut audio: Option<Audio> = None;

    let (cfg_tx, cfg_rx) = mpsc::channel::<Config>();
    let (ambient_cfg_tx, ambient_factor_rx) = ambient::start(cfg.clone(), wake.clone());
    ambient_cfg_tx.send(ambient::AmbientCommand::Suspended(!activity.available()))?;
    let (weather_cfg_tx, weather_rx) = start_weather(cfg.clone(), wake.clone());
    let settings_open = Arc::new(AtomicU32::new(0));
    if open_settings_on_start {
        open_settings(
            cfg.clone(),
            cfg_tx.clone(),
            settings_open.clone(),
            wake.clone(),
        );
    }

    {
        let running = running.clone();
        let wake = wake.clone();
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
            wake.notify();
        })?;
    }

    let mut paused = false;
    let mut user_quit = false;
    let mut preview_until: Option<Instant> = None;
    let mut ambient_brightness = None;
    let mut ambient_last_ok: Option<Instant> = None;
    let mut ambient_failed = false;
    let mut current_weather: Option<Weather> = None;
    let mut screen_available = activity.available();
    let mut ambient_source = None;
    let mut last_display_brightness: Option<(f32, Instant)> = None;
    let mut last_display_cct: Option<(f32, Instant)> = None;

    while running.load(Ordering::SeqCst) {
        while let Some(result) = hardware.poll() {
            handle_hardware_result(
                result,
                overlay_hwnd,
                &mut display_control,
                display_requested(
                    &cfg,
                    screen_available,
                    paused,
                    preview_until,
                    Instant::now(),
                ),
            );
        }
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
            if cfg.preserve_colors() || !screen_available {
                continue;
            }
            ambient_source = reading.as_ref().ok().map(|reading| reading.source);
            apply_ambient_reading(
                reading.map(|reading| reading.brightness),
                &mut ambient_brightness,
                &mut ambient_failed,
                &mut ambient_last_ok,
            );
        }

        if cfg.preserve_colors() {
            tray.set_ambient_status("Luz ambiente: pausada para preservar cores");
        } else if cfg.ambient_enabled && ambient_source == Some(ambient::AmbientSource::LightSensor)
        {
            tray.set_ambient_status("Luz ambiente: sensor do Windows ativo");
        } else if cfg.ambient_enabled
            && !cfg.camera_is_calibrated()
            && !cfg.ambient_prefer_light_sensor
        {
            tray.set_ambient_status("Luz ambiente: calibre a câmera no painel");
        } else if cfg.ambient_enabled && ambient_last_ok.is_none() && !ambient_failed {
            tray.set_ambient_status("Luz ambiente: buscando uma leitura válida");
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

        if cfg.preserve_colors() || !cfg.display_enabled || paused || preview {
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

        tracing::debug!(
            cct = target.cct_kelvin as u32,
            brilho_pct = (target.brightness * 100.0) as u32,
            ruido = ?target.noise,
            preview,
            "tick"
        );

        let display_enabled = display_requested(
            &cfg,
            screen_available,
            paused,
            preview_until,
            Instant::now(),
        );
        if !display_enabled {
            overlay::hide(overlay_hwnd);
        }
        drive_hardware(
            &mut display_control,
            &mut hardware,
            display_enabled,
            &target,
            &cfg,
            Instant::now(),
        );

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
        let deadline = preview_until.map_or(Instant::now() + tick, |preview_end| {
            preview_end.min(Instant::now() + tick)
        });
        let mut kick = false;

        while Instant::now() < deadline && running.load(Ordering::SeqCst) && !kick {
            overlay::pump_messages();

            if activity.closing() {
                running.store(false, Ordering::SeqCst);
                break;
            }
            let available = activity.available();
            if available != screen_available {
                screen_available = available;
                if ambient_cfg_tx
                    .send(ambient::AmbientCommand::Suspended(!available))
                    .is_err()
                {
                    tracing::error!("não foi possível atualizar a pausa do sensor de luz");
                }
                if !available {
                    ambient_brightness = None;
                    ambient_last_ok = None;
                    ambient_source = None;
                    overlay::hide(overlay_hwnd);
                    display_control.set_requested(false, Instant::now());
                } else {
                    display_control.restart(Instant::now());
                }
                kick = true;
            }
            if activity.take_refresh() {
                display_control.restart(Instant::now());
                kick = true;
            }
            while let Some(result) = hardware.poll() {
                handle_hardware_result(
                    result,
                    overlay_hwnd,
                    &mut display_control,
                    display_requested(
                        &cfg,
                        screen_available,
                        paused,
                        preview_until,
                        Instant::now(),
                    ),
                );
            }

            if unsafe { WaitForSingleObject(settings_event, 0) } == WAIT_OBJECT_0 {
                open_settings(
                    cfg.clone(),
                    cfg_tx.clone(),
                    settings_open.clone(),
                    wake.clone(),
                );
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
                            preview_until = None;
                            display_control.set_requested(false, Instant::now());
                            overlay::hide(overlay_hwnd);
                            if let Some(ref mut aud) = audio {
                                aud.silence();
                            }
                            tracing::info!("pausada");
                        } else {
                            display_control.restart(Instant::now());
                            tracing::info!("retomada");
                        }
                        kick = true;
                    }
                    TrayAction::ToggleAutostart => {
                        if let Some(ref a) = autostart {
                            match a.toggle() {
                                Ok(enabled) => {
                                    let previous = cfg.clone();
                                    cfg.start_with_windows = Some(enabled);
                                    if !persist(&mut cfg, &previous, config_event)
                                        && let Err(error) = a.synchronize(Some(!enabled))
                                    {
                                        tracing::error!(%error, "não foi possível restaurar o início automático após falha ao salvar");
                                    }
                                    tray.set_autostart(a.is_enabled());
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
                        let previous = cfg.clone();
                        cfg.noise_enabled = !cfg.noise_enabled;
                        persist(&mut cfg, &previous, config_event);
                        tray.set_noise(cfg.noise_enabled);
                        kick = true;
                    }
                    TrayAction::PreviewNight => {
                        preview_until = Some(Instant::now() + Duration::from_secs(20));
                        tracing::info!("prévia noturna — 20 s de tela quente e som, se ativado");
                        kick = true;
                    }
                    TrayAction::OpenSettings => {
                        open_settings(
                            cfg.clone(),
                            cfg_tx.clone(),
                            settings_open.clone(),
                            wake.clone(),
                        );
                    }
                    TrayAction::CheckUpdates => {
                        open_settings(
                            cfg.clone(),
                            cfg_tx.clone(),
                            settings_open.clone(),
                            wake.clone(),
                        );
                    }
                    TrayAction::SetIntensity(level) => {
                        let previous = cfg.clone();
                        cfg.intensity = level;
                        last_display_brightness = None;
                        last_display_cct = None;
                        persist(&mut cfg, &previous, config_event);
                        tray.set_intensity(cfg.intensity);
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
                if cfg.preserve_colors() || !screen_available {
                    continue;
                }
                ambient_source = reading.as_ref().ok().map(|reading| reading.source);
                apply_ambient_reading(
                    reading.map(|reading| reading.brightness),
                    &mut ambient_brightness,
                    &mut ambient_failed,
                    &mut ambient_last_ok,
                );
                kick = true;
            }

            tick_audio(&mut audio, &target, &cfg, paused, preview);

            if display_control.followup_ready(Instant::now()) {
                kick = true;
            }
            if kick || !running.load(Ordering::SeqCst) {
                break;
            }
            let wait_until = display_control
                .retry_at()
                .map_or(deadline, |retry| retry.min(deadline));
            let remaining = wait_until.saturating_duration_since(Instant::now());
            let timeout = if audio.as_ref().is_some_and(Audio::needs_tick) {
                remaining.min(Duration::from_millis(50))
            } else {
                remaining
            };
            match wait_for_work(
                &[wake.handle(), settings_event, config_event, quit_event],
                timeout,
            )? {
                1 => unsafe { SetEvent(settings_event)? },
                2 => unsafe { SetEvent(config_event)? },
                3 => {
                    user_quit = true;
                    running.store(false, Ordering::SeqCst);
                }
                _ => {}
            }
        }
    }

    overlay::hide(overlay_hwnd);
    drop(audio);
    estel::status::publish_stopping();
    drop(tray);
    let restored = hardware.shutdown();
    estel::status::publish_stopped(restored);
    if restored {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayAction {
    Target,
    Park,
    Refresh,
}

struct DisplayControl {
    requested: bool,
    restored: bool,
    restore_needed: bool,
    pending: Option<DisplayAction>,
    park_attempts: u8,
    retry: Option<Instant>,
    refresh_needed: bool,
    target_due: bool,
    park_replied: bool,
    failed: bool,
}

impl DisplayControl {
    fn new() -> Self {
        Self {
            requested: false,
            restored: false,
            restore_needed: true,
            pending: None,
            park_attempts: 0,
            retry: None,
            refresh_needed: false,
            target_due: true,
            park_replied: false,
            failed: false,
        }
    }

    fn set_requested(&mut self, requested: bool, now: Instant) {
        if self.requested == requested {
            return;
        }
        self.requested = requested;
        self.park_attempts = u8::from(self.pending == Some(DisplayAction::Park));
        self.retry = Some(now);
        self.target_due = true;
        if requested && self.failed {
            self.refresh_needed = true;
            self.failed = false;
        }
        if !requested {
            self.restore_needed = !self.restored || self.pending.is_some();
        }
    }

    fn restart(&mut self, now: Instant) {
        self.park_attempts = u8::from(self.pending == Some(DisplayAction::Park));
        self.retry = Some(now);
        self.target_due = true;
        self.refresh_needed = true;
        self.failed = false;
        if !self.requested {
            self.restored = false;
            self.restore_needed = true;
        }
    }

    fn next_action(&mut self, requested: bool, now: Instant) -> Option<DisplayAction> {
        self.set_requested(requested, now);
        if self.pending.is_some() {
            self.target_due |= requested;
            return None;
        }
        if requested && self.failed {
            return None;
        }
        // A disconnected monitor can keep its recovery snapshot pending while
        // the worker safely applies adjustments to the other outputs.
        let action = if !self.park_replied || (!self.requested && self.restore_needed) {
            if self.park_attempts >= 3 || self.retry.is_some_and(|retry| retry > now) {
                return None;
            }
            self.park_attempts += 1;
            self.retry = None;
            DisplayAction::Park
        } else if !self.requested {
            return None;
        } else if self.refresh_needed {
            self.refresh_needed = false;
            DisplayAction::Refresh
        } else {
            self.target_due = false;
            self.restored = false;
            DisplayAction::Target
        };
        self.pending = Some(action);
        Some(action)
    }

    fn observe(&mut self, result: &Result<HardwareStatus, String>, now: Instant) -> bool {
        let Some(action) = self.pending.take() else {
            return false;
        };
        if action == DisplayAction::Park {
            self.park_replied = true;
            self.failed = !result
                .as_ref()
                .is_ok_and(|status| status.applied_target.is_none());
            self.restored = result
                .as_ref()
                .is_ok_and(|status| status.restored && status.applied_target.is_none());
            self.restore_needed = !self.restored;
            self.retry = (!self.requested && !self.restored && self.park_attempts < 3)
                .then(|| now + Duration::from_secs(5 * u64::from(self.park_attempts)));
            if self.restore_needed && self.park_attempts >= 3 {
                tracing::warn!(
                    "restauração não confirmada após três tentativas; aguardando retomada ou reconexão"
                );
            }
            return false;
        }
        if result.is_err() {
            self.failed = true;
            self.restored = false;
            self.restore_needed = true;
            self.retry = (!self.requested).then_some(now);
            return false;
        }
        self.failed = false;
        self.requested && !self.refresh_needed
    }

    fn retry_at(&self) -> Option<Instant> {
        if !self.requested
            && self.pending.is_none()
            && self.restore_needed
            && self.park_attempts < 3
        {
            self.retry
        } else {
            None
        }
    }

    fn followup_ready(&self, now: Instant) -> bool {
        if self.pending.is_some() {
            return false;
        }
        if !self.requested && self.restore_needed {
            self.park_attempts < 3 && self.retry.is_none_or(|retry| retry <= now)
        } else {
            self.requested && !self.failed && (self.target_due || self.refresh_needed)
        }
    }
}

fn display_requested(
    cfg: &Config,
    screen_available: bool,
    paused: bool,
    preview: Option<Instant>,
    now: Instant,
) -> bool {
    screen_available
        && cfg.display_enabled
        && !cfg.preserve_colors()
        && (!paused || preview.is_some_and(|until| now < until))
}

fn drive_hardware(
    control: &mut DisplayControl,
    hardware: &mut HardwareClient,
    requested: bool,
    target: &Target,
    cfg: &Config,
    now: Instant,
) {
    match control.next_action(requested, now) {
        Some(DisplayAction::Target) => {
            hardware.set_target(target, cfg.gamma_warm_floor_k, cfg.min_brightness)
        }
        Some(DisplayAction::Park) => hardware.park(),
        Some(DisplayAction::Refresh) => hardware.refresh(),
        None => {}
    }
}

fn handle_hardware_result(
    result: Result<HardwareStatus, String>,
    hwnd: windows::Win32::Foundation::HWND,
    control: &mut DisplayControl,
    visible: bool,
) {
    let now = Instant::now();
    control.set_requested(visible, now);
    let allow_overlay = control.observe(&result, now);
    match result {
        Ok(status) => {
            estel::status::publish_hardware(&status);
            if allow_overlay && let Some(target) = status.applied_target {
                overlay::update_outputs(
                    hwnd,
                    target.cct_kelvin,
                    target.brightness,
                    &status.outputs,
                );
            } else {
                overlay::hide(hwnd);
            }
        }
        Err(error) => {
            tracing::error!(%error, "ajuste do monitor indisponível");
            estel::status::publish_error();
            overlay::hide(hwnd);
        }
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
        match Config::load_or_default() {
            Ok(config) => latest = Some(config),
            Err(error) => {
                tracing::error!(%error, "não foi possível reler a configuração; mantendo preferências em uso")
            }
        }
    }
    latest
}

fn apply_config_change(
    incoming: Config,
    cfg: &mut Config,
    tray: &Tray,
    ambient_cfg_tx: &mpsc::Sender<ambient::AmbientCommand>,
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
    if camera_worker_changed
        && ambient_cfg_tx
            .send(ambient::AmbientCommand::Configure(Box::new(cfg.clone())))
            .is_err()
    {
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
        || old.ambient_prefer_light_sensor != new.ambient_prefer_light_sensor
        || old.ambient_camera_index != new.ambient_camera_index
        || old.ambient_camera_id != new.ambient_camera_id
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
        || old.ambient_prefer_light_sensor != new.ambient_prefer_light_sensor
        || old.ambient_calibration != new.ambient_calibration
        || old.ambient_camera_index != new.ambient_camera_index
        || old.ambient_camera_id != new.ambient_camera_id
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

fn start_weather(
    initial: Config,
    wake: WakeSignal,
) -> (mpsc::Sender<Config>, mpsc::Receiver<WeatherReading>) {
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
                    wake.notify();
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

fn persist(cfg: &mut Config, previous: &Config, config_event: HANDLE) -> bool {
    match cfg.save_changes(previous, &Config::config_path()) {
        Ok(_) => {
            if let Err(error) = unsafe { SetEvent(config_event) } {
                tracing::error!(%error, "não foi possível avisar os ajustes de tela sobre a configuração salva");
            }
            tracing::info!("configuração salva");
            true
        }
        Err(e) => {
            *cfg = previous.clone();
            tracing::error!("não foi possível salvar a configuração: {e}");
            show_error(w!(
                "Não foi possível salvar. Suas preferências anteriores foram mantidas; confira a pasta de configuração e tente novamente."
            ));
            false
        }
    }
}

fn open_settings(_cfg: Config, tx: mpsc::Sender<Config>, state: Arc<AtomicU32>, wake: WakeSignal) {
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
                    .env("ESTEL_UI_PARENT_PID", std::process::id().to_string())
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
        match Config::load_or_default() {
            Ok(config) => {
                if tx.send(config).is_ok() {
                    wake.notify();
                }
            }
            Err(error) => {
                tracing::error!(%error, "não foi possível reler a configuração após fechar o painel")
            }
        }
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
        Err(error) => {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(env)
                .init();
            tracing::warn!(%error, "não foi possível abrir o arquivo de log");
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
        DisplayAction, DisplayControl, ambient_source_changed, ambient_status_text,
        ambient_worker_changed, apply_ambient_reading, brightness_controls_changed,
        brightness_with_ambient, brightness_with_sources, display_requested,
        fresh_camera_brightness, limit_brightness_change, limit_color_change,
        requested_audio_target, weather_source_changed,
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
    fn changing_only_camera_identity_updates_the_worker_and_brightness_source() {
        let current = Config {
            ambient_camera_id: Some("camera-a".into()),
            ..Config::default()
        };
        let next = Config {
            ambient_camera_id: Some("camera-b".into()),
            ..current.clone()
        };
        assert!(ambient_source_changed(&current, &next));
        assert!(ambient_worker_changed(&current, &next));
        assert!(brightness_controls_changed(&current, &next));
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
    fn failed_screen_restore_retries_three_times_with_bounded_backoff() {
        let now = Instant::now();
        let mut control = DisplayControl::new();
        assert_eq!(control.next_action(false, now), Some(DisplayAction::Park));
        assert!(!control.observe(&Err("driver indisponível".into()), now));
        assert!(!control.restored);
        assert_eq!(control.retry_at(), Some(now + Duration::from_secs(5)));
        assert_eq!(
            control.next_action(false, now + Duration::from_secs(4)),
            None
        );
        assert_eq!(
            control.next_action(false, now + Duration::from_secs(5)),
            Some(DisplayAction::Park)
        );
        assert!(!control.observe(
            &Ok(hardware_status(false, None)),
            now + Duration::from_secs(5)
        ));
        assert_eq!(control.retry_at(), Some(now + Duration::from_secs(15)));
        assert_eq!(
            control.next_action(false, now + Duration::from_secs(14)),
            None
        );
        assert_eq!(
            control.next_action(false, now + Duration::from_secs(15)),
            Some(DisplayAction::Park)
        );
        control.observe(
            &Err("driver indisponível".into()),
            now + Duration::from_secs(15),
        );
        assert_eq!(
            control.next_action(false, now + Duration::from_secs(3600)),
            None
        );
        assert_eq!(control.retry_at(), None);
        assert!(!control.followup_ready(now + Duration::from_secs(3600)));
        assert!(!control.restored);
    }

    fn hardware_status(
        restored: bool,
        applied_target: Option<Target>,
    ) -> estel::hardware_worker::HardwareStatus {
        estel::hardware_worker::HardwareStatus {
            outputs: Vec::new(),
            restored,
            applied_target,
            recovery_warning: None,
        }
    }

    fn active_display(now: Instant) -> DisplayControl {
        let mut control = DisplayControl::new();
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Park));
        control.observe(&Ok(hardware_status(true, None)), now);
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Target));
        assert!(control.observe(&Ok(hardware_status(false, Some(Target::neutral()))), now));
        control
    }

    #[test]
    fn pause_waits_for_target_reply_and_resume_waits_for_park_reply() {
        let now = Instant::now();
        let mut control = active_display(now);
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Target));
        assert_eq!(control.next_action(false, now), None);
        assert!(!control.observe(&Ok(hardware_status(false, Some(Target::neutral()))), now));
        assert!(!control.restored);
        assert_eq!(control.next_action(false, now), Some(DisplayAction::Park));
        assert_eq!(control.next_action(true, now), None);
        assert!(!control.observe(&Ok(hardware_status(true, None)), now));
        assert!(control.restored);
        assert!(control.followup_ready(now));
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Target));
    }

    #[test]
    fn applied_target_is_never_accepted_as_confirmation_of_restoration() {
        let now = Instant::now();
        let mut control = DisplayControl::new();
        assert_eq!(control.next_action(false, now), Some(DisplayAction::Park));
        assert!(!control.observe(&Ok(hardware_status(true, Some(Target::neutral()))), now));
        assert!(!control.restored);
        assert_eq!(control.next_action(false, now), None);
        assert_eq!(
            control.next_action(false, now + Duration::from_secs(5)),
            Some(DisplayAction::Park)
        );
    }

    #[test]
    fn every_display_disable_path_requests_restoration() {
        let now = Instant::now();
        for mode in 0..5 {
            let mut control = active_display(now);
            let mut config = Config::default();
            match mode {
                0 => config.display_enabled = false,
                1 => config.color_critical_work = true,
                2 => config.color_vision_deficiency = true,
                _ => {}
            }
            let requested = display_requested(&config, mode != 3, mode == 4, None, now);
            assert!(!requested);
            assert_eq!(
                control.next_action(requested, now),
                Some(DisplayAction::Park)
            );
            assert!(!control.restored);
            control.observe(&Ok(hardware_status(true, None)), now);
            assert!(control.restored);
            assert_eq!(control.next_action(requested, now), None);
        }
    }

    #[test]
    fn reconnect_or_resume_restarts_exhausted_restoration_without_overlapping_requests() {
        let now = Instant::now();
        for resume in [false, true] {
            let mut control = DisplayControl::new();
            for seconds in [0, 5, 15] {
                let at = now + Duration::from_secs(seconds);
                assert_eq!(control.next_action(false, at), Some(DisplayAction::Park));
                control.observe(&Err("driver indisponível".into()), at);
            }
            let at = now + Duration::from_secs(20);
            if !resume {
                control.restart(at);
            }
            let expected = if resume {
                DisplayAction::Refresh
            } else {
                DisplayAction::Park
            };
            assert_eq!(control.next_action(resume, at), Some(expected));
            assert_eq!(control.next_action(resume, at), None);
            control.observe(&Ok(hardware_status(!resume, None)), at);
            if resume {
                assert_eq!(control.next_action(true, at), Some(DisplayAction::Target));
            } else {
                assert!(control.restored);
                assert_eq!(control.next_action(false, at), None);
            }
        }
    }

    #[test]
    fn partial_startup_recovery_allows_safe_outputs_and_their_overlay_to_continue() {
        let now = Instant::now();
        let mut control = DisplayControl::new();
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Park));
        let mut partial = hardware_status(false, None);
        partial.recovery_warning =
            Some("Uma tela desconectada ainda precisa ser restaurada.".into());
        assert!(!control.observe(&Ok(partial), now));
        assert!(!control.restored);
        assert!(control.followup_ready(now));
        assert_eq!(control.retry_at(), None);
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Target));
        assert_eq!(control.next_action(true, now), None);
        assert!(control.observe(&Ok(hardware_status(false, Some(Target::neutral()))), now));
    }

    #[test]
    fn resume_during_park_waits_for_reply_but_not_for_a_disconnected_monitor() {
        let now = Instant::now();
        let mut control = active_display(now);
        assert_eq!(control.next_action(false, now), Some(DisplayAction::Park));
        assert_eq!(control.next_action(true, now), None);
        assert!(!control.observe(&Ok(hardware_status(false, None)), now));
        assert!(!control.restored);
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Target));
        assert!(control.observe(&Ok(hardware_status(false, Some(Target::neutral()))), now));
    }

    #[test]
    fn disconnected_monitor_does_not_restart_exhausted_paused_retry_cycles() {
        let now = Instant::now();
        let mut control = DisplayControl::new();
        for seconds in [0, 5, 15] {
            let at = now + Duration::from_secs(seconds);
            assert_eq!(control.next_action(false, at), Some(DisplayAction::Park));
            control.observe(&Ok(hardware_status(false, None)), at);
        }
        for seconds in [16, 30, 120, 3600] {
            let at = now + Duration::from_secs(seconds);
            assert_eq!(control.next_action(false, at), None);
            assert!(!control.followup_ready(at));
        }
        assert_eq!(
            control.next_action(true, now + Duration::from_secs(3601)),
            Some(DisplayAction::Target)
        );
    }

    #[test]
    fn failed_worker_requires_explicit_refresh_before_any_new_target() {
        let now = Instant::now();
        let mut control = DisplayControl::new();
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Park));
        control.observe(&Err("worker indisponível".into()), now);
        assert!(!control.followup_ready(now + Duration::from_secs(60)));
        assert_eq!(
            control.next_action(true, now + Duration::from_secs(60)),
            None
        );
        control.restart(now);
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Refresh));
        control.observe(&Err("worker indisponível".into()), now);
        assert_eq!(control.next_action(true, now), None);
        control.restart(now);
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Refresh));
        control.observe(&Ok(hardware_status(false, None)), now);
        assert_eq!(control.next_action(true, now), Some(DisplayAction::Target));
    }
}
