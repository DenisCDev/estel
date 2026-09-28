#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, Timelike};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
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
use estel::weather::{self, Weather, WeatherPhase};

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
            ambient::sample_luminance(camera_index).map_err(anyhow::Error::msg)?
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
    tray.set_weather_status(if cfg.weather_enabled {
        "Clima: consultando..."
    } else {
        "Clima: desligado"
    });
    if cfg.weather_enabled {
        weather::publish_status(&cfg, WeatherPhase::Consulting);
    }
    let overlay_hwnd = overlay::create()?;

    let _gamma_ok = display::init();
    let ddc_ok = if cfg.display_enabled {
        brightness::init()
    } else {
        false
    };
    session::mark_dirty();

    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        display::restore();
        brightness::restore();
        orig_hook(info);
    }));

    let mut audio: Option<Audio> = None;

    let (cfg_tx, cfg_rx) = mpsc::channel::<Config>();
    let (ambient_cfg_tx, ambient_factor_rx) = ambient::start(cfg.clone());
    let (weather_cfg_tx, weather_rx) = start_weather(cfg.clone());
    let settings_open = Arc::new(AtomicBool::new(false));
    if open_settings_on_start {
        open_settings(cfg.clone(), cfg_tx.clone(), settings_open.clone());
    }

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = running.clone();
        ctrlc::set_handler(move || {
            display::restore();
            brightness::restore();
            running.store(false, Ordering::SeqCst);
        })?;
    }

    let mut paused = false;
    let mut preview_until: Option<Instant> = None;
    let mut ambient_brightness = None;
    let mut ambient_failed = false;
    let mut current_weather: Option<Weather> = None;

    while running.load(Ordering::SeqCst) {
        if let Some(incoming) = pending_config(config_event, &cfg_rx) {
            apply_config_change(
                incoming,
                &mut cfg,
                &tray,
                &ambient_cfg_tx,
                &weather_cfg_tx,
                &mut ambient_brightness,
                &mut ambient_failed,
            );
            current_weather = None;
            if cfg.weather_enabled {
                weather::publish_status(&cfg, WeatherPhase::Consulting);
                tray.set_weather_status("Clima: consultando...");
            } else {
                tray.set_weather_status("Clima: desligado");
            }
        }
        while let Ok((latitude, longitude, result)) = weather_rx.try_recv() {
            if !cfg.weather_enabled || (latitude, longitude) != (cfg.latitude, cfg.longitude) {
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
            update_ambient_status(
                &tray,
                cfg.ambient_enabled,
                ambient_failed,
                cfg.weather_enabled && current_weather.is_some(),
            );
        }
        while let Ok(reading) = ambient_factor_rx.try_recv() {
            apply_ambient_reading(reading, &mut ambient_brightness, &mut ambient_failed);
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
        let mut target = scheduled.attenuate(cfg.intensity.factor());
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
            target.brightness =
                brightness_with_ambient(fallback, cfg.ambient_enabled, ambient_brightness);
        }

        if paused && !preview {
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

        if paused && !preview {
            // parked at the moment of pause
        } else if cfg.display_enabled {
            let display_started = Instant::now();
            match display::apply(&target, cfg.gamma_warm_floor_k, cfg.min_brightness) {
                Ok(true) => tracing::debug!(cct = target.cct_kelvin as u32, "gamma ok"),
                Ok(false) => {
                    tracing::debug!(cct = target.cct_kelvin as u32, "gamma recusada ou ausente")
                }
                Err(e) => tracing::error!("display::apply: {e}"),
            }
            if display_started.elapsed() > Duration::from_millis(250) {
                tracing::warn!(
                    elapsed_ms = display_started.elapsed().as_millis(),
                    "ajuste de cor demorou"
                );
            }
            overlay::update(
                overlay_hwnd,
                target.cct_kelvin,
                target.brightness,
                ddc_ok && brightness::is_active(),
            );
            let brightness_started = Instant::now();
            brightness::apply(target.brightness);
            if brightness_started.elapsed() > Duration::from_millis(250) {
                tracing::warn!(
                    elapsed_ms = brightness_started.elapsed().as_millis(),
                    "ajuste de brilho demorou"
                );
            }
        } else {
            overlay::hide(overlay_hwnd);
        }

        if should_open_audio(paused, preview, cfg.noise_enabled, target.noise) && audio.is_none() {
            audio = Audio::try_new();
        }
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
                        running.store(false, Ordering::SeqCst);
                    }
                    TrayAction::TogglePause => {
                        paused = !paused;
                        tray.set_paused(paused);
                        if paused {
                            display::park();
                            brightness::park();
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
                        tracing::info!("prévia noturna — 20 s de tela quente e ruído");
                        kick = true;
                    }
                    TrayAction::OpenSettings => {
                        open_settings(cfg.clone(), cfg_tx.clone(), settings_open.clone());
                    }
                    TrayAction::CheckUpdates => {
                        if let Err(e) =
                            webbrowser::open("https://github.com/DenisCDev/estel/releases/latest")
                        {
                            tracing::error!("não foi possível abrir a página de atualização: {e}");
                            show_error(w!("Não foi possível abrir a página de atualização."));
                        }
                    }
                    TrayAction::SetIntensity(level) => {
                        cfg.intensity = level;
                        tray.set_intensity(level);
                        persist(&cfg);
                        tracing::info!(level = level.label(), "intensidade");
                        kick = true;
                    }
                }
            }

            if let Some(incoming) = pending_config(config_event, &cfg_rx) {
                apply_config_change(
                    incoming,
                    &mut cfg,
                    &tray,
                    &ambient_cfg_tx,
                    &weather_cfg_tx,
                    &mut ambient_brightness,
                    &mut ambient_failed,
                );
                current_weather = None;
                tray.set_weather_status(if cfg.weather_enabled {
                    "Clima: consultando..."
                } else {
                    "Clima: desligado"
                });
                if cfg.weather_enabled {
                    weather::publish_status(&cfg, WeatherPhase::Consulting);
                }
                kick = true;
            }
            while let Ok((latitude, longitude, result)) = weather_rx.try_recv() {
                if !cfg.weather_enabled || (latitude, longitude) != (cfg.latitude, cfg.longitude) {
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
                update_ambient_status(
                    &tray,
                    cfg.ambient_enabled,
                    ambient_failed,
                    cfg.weather_enabled && current_weather.is_some(),
                );
                kick = true;
            }

            while let Ok(reading) = ambient_factor_rx.try_recv() {
                apply_ambient_reading(reading, &mut ambient_brightness, &mut ambient_failed);
                update_ambient_status(
                    &tray,
                    cfg.ambient_enabled,
                    ambient_failed,
                    cfg.weather_enabled && current_weather.is_some(),
                );
                kick = true;
            }

            tick_audio(&mut audio, &target, &cfg, paused, preview);

            std::thread::sleep(step);
            elapsed += step;
        }
    }

    display::restore();
    brightness::restore();
    tracing::info!("Estel encerrado — monitor restaurado");
    Ok(())
}

fn apply_ambient_reading(
    reading: Result<f32, String>,
    brightness: &mut Option<f32>,
    failed: &mut bool,
) {
    match reading {
        Ok(value) => {
            *brightness = Some(value.clamp(0.0, 1.0));
            *failed = false;
        }
        Err(_) => {
            *brightness = None;
            *failed = true;
        }
    }
}

fn brightness_with_ambient(scheduled: f32, enabled: bool, measured: Option<f32>) -> f32 {
    if enabled {
        measured.unwrap_or(scheduled)
    } else {
        scheduled
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
) {
    *cfg = incoming;
    tray.set_intensity(cfg.intensity);
    tray.set_noise(cfg.noise_enabled);
    *ambient_brightness = None;
    *ambient_failed = false;
    tray.set_ambient_status(if cfg.ambient_enabled {
        "Luz ambiente: aguardando câmera"
    } else {
        "Luz ambiente: desligada"
    });
    if ambient_cfg_tx.send(cfg.clone()).is_err() {
        tracing::error!("sensor de luz ambiente encerrou inesperadamente");
    }
    if weather_cfg_tx.send(cfg.clone()).is_err() {
        tracing::error!("consulta de clima encerrou inesperadamente");
    }
    tracing::info!(
        ambient_enabled = cfg.ambient_enabled,
        interval_s = cfg.ambient_sample_interval_seconds,
        "configuração aplicada"
    );
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
                if config.weather_enabled {
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
                    .filter(|_| config.weather_enabled)
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

fn should_open_audio(
    paused: bool,
    preview: bool,
    noise_enabled: bool,
    noise: Option<NoiseColor>,
) -> bool {
    !paused && (preview || (noise_enabled && noise.is_some()))
}

fn tick_audio(
    audio: &mut Option<Audio>,
    target: &Target,
    cfg: &Config,
    paused: bool,
    preview: bool,
) {
    if let Some(active) = audio.as_mut() {
        if preview && !paused {
            active.tick(target.noise, 1.0, 0.55);
        } else if paused || !cfg.noise_enabled {
            active.tick(None, 0.0, cfg.max_volume);
        } else {
            active.tick(target.noise, target.noise_gain, cfg.max_volume);
        }
    }
    if (paused || !cfg.noise_enabled || target.noise.is_none())
        && !preview
        && audio.as_ref().is_some_and(Audio::is_silent)
    {
        *audio = None;
    }
}

fn update_ambient_status(tray: &Tray, enabled: bool, failed: bool, weather_active: bool) {
    if !enabled {
        tray.set_ambient_status("Luz ambiente: desligada");
    } else if failed {
        tray.set_ambient_status(if weather_active {
            "Câmera indisponível — clima e horário"
        } else {
            "Câmera indisponível — brilho por horário"
        });
    } else {
        tray.set_ambient_status("Luz ambiente: ativa");
    }
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

fn open_settings(_cfg: Config, tx: mpsc::Sender<Config>, flag: Arc<AtomicBool>) {
    if flag.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let result = std::env::current_exe()
            .and_then(|executable| {
                std::process::Command::new(executable)
                    .arg("--settings-window")
                    .spawn()
            })
            .and_then(|mut child| child.wait());
        if let Err(error) = result {
            tracing::error!("janela de configurações: {error}");
            show_error(w!("Não foi possível abrir as configurações."));
        }
        let _ = tx.send(Config::load_or_default());
        flag.store(false, Ordering::SeqCst);
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
    use super::{apply_ambient_reading, brightness_with_ambient, should_open_audio};
    use estel::target::NoiseColor;

    #[test]
    fn camera_failure_restores_scheduled_brightness() {
        let mut brightness = Some(0.65);
        let mut failed = false;
        apply_ambient_reading(
            Err("câmera indisponível".into()),
            &mut brightness,
            &mut failed,
        );
        assert_eq!(brightness_with_ambient(0.2, true, brightness), 0.2);
        assert!(failed);

        apply_ambient_reading(Ok(0.8), &mut brightness, &mut failed);
        assert_eq!(brightness_with_ambient(0.2, true, brightness), 0.8);
        assert_eq!(brightness_with_ambient(0.2, false, brightness), 0.2);
        assert!(!failed);
    }

    #[test]
    fn camera_brightness_overrides_night_schedule() {
        assert_eq!(brightness_with_ambient(0.16, true, Some(1.0)), 1.0);
        assert_eq!(brightness_with_ambient(0.16, true, Some(0.35)), 0.35);
    }

    #[test]
    fn disabled_noise_does_not_open_an_audio_device() {
        assert!(!should_open_audio(
            false,
            false,
            false,
            Some(NoiseColor::Pink)
        ));
        assert!(!should_open_audio(false, false, true, None));
        assert!(!should_open_audio(true, true, true, Some(NoiseColor::Pink)));
        assert!(should_open_audio(false, true, false, None));
        assert!(should_open_audio(
            false,
            false,
            true,
            Some(NoiseColor::Pink)
        ));
    }
}
