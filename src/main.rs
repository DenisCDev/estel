#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, Timelike};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

use estel::audio::Audio;
use estel::brightness;
use estel::config::Config;
use estel::display;
use estel::overlay;
use estel::schedule::DayContext;
use estel::session;
use estel::target::{NoiseColor, Target};
use estel::tray::{Autostart, Tray, TrayAction};

fn main() -> anyhow::Result<()> {
    init_log();

    unsafe {
        CreateMutexW(None, false, w!("Local\\EstelSingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            tracing::info!("Estel já está em execução");
            return Ok(());
        }
    }

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
    )?;
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

    let mut audio = Audio::try_new();
    if audio.is_none() {
        tracing::info!("sem dispositivo de áudio — ruído desligado");
    }

    let (cfg_tx, cfg_rx) = mpsc::channel::<Config>();
    let settings_open = Arc::new(AtomicBool::new(false));

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

    while running.load(Ordering::SeqCst) {
        while let Ok(incoming) = cfg_rx.try_recv() {
            cfg = incoming;
            tray.set_intensity(cfg.intensity);
            tray.set_noise(cfg.noise_enabled);
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
        }

        if paused && !preview {
            tray.set_tooltip("Estel · pausada");
        } else if preview {
            tray.set_tooltip("Estel · prévia noturna");
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
            match display::apply(&target, cfg.gamma_warm_floor_k, cfg.min_brightness) {
                Ok(true) => tracing::debug!(cct = target.cct_kelvin as u32, "gamma ok"),
                Ok(false) => {
                    tracing::debug!(cct = target.cct_kelvin as u32, "gamma recusada ou ausente")
                }
                Err(e) => tracing::error!("display::apply: {e}"),
            }
            overlay::update(
                overlay_hwnd,
                target.cct_kelvin,
                target.brightness,
                ddc_ok && brightness::is_active(),
            );
            brightness::apply(target.brightness);
        } else {
            overlay::hide(overlay_hwnd);
        }

        if let Some(ref mut aud) = audio {
            if preview {
                aud.tick(target.noise, 1.0, 0.55);
            } else if paused || !cfg.noise_enabled {
                aud.tick(None, 0.0, cfg.max_volume);
            } else {
                aud.tick(target.noise, target.noise_gain, cfg.max_volume);
            }
        }

        let tick = Duration::from_secs(cfg.tick_seconds.max(5));
        let step = Duration::from_millis(50);
        let mut elapsed = Duration::ZERO;
        let mut kick = false;

        while elapsed < tick && running.load(Ordering::SeqCst) && !kick {
            overlay::pump_messages();

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
                            let enabled = a.toggle();
                            tray.set_autostart(enabled);
                            tracing::info!(enabled, "início automático");
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
                    TrayAction::SetIntensity(level) => {
                        cfg.intensity = level;
                        tray.set_intensity(level);
                        persist(&cfg);
                        tracing::info!(level = level.label(), "intensidade");
                        kick = true;
                    }
                }
            }

            while let Ok(incoming) = cfg_rx.try_recv() {
                cfg = incoming;
                tray.set_intensity(cfg.intensity);
                tray.set_noise(cfg.noise_enabled);
                kick = true;
            }

            if let Some(ref mut aud) = audio {
                if preview {
                    aud.tick(target.noise, 1.0, 0.55);
                } else if paused || !cfg.noise_enabled {
                    aud.tick(None, 0.0, cfg.max_volume);
                } else {
                    aud.tick(target.noise, target.noise_gain, cfg.max_volume);
                }
            }

            std::thread::sleep(step);
            elapsed += step;
        }
    }

    display::restore();
    brightness::restore();
    tracing::info!("Estel encerrado — monitor restaurado");
    Ok(())
}

fn persist(cfg: &Config) {
    match cfg.save(&Config::config_path()) {
        Ok(()) => tracing::info!("configuração salva"),
        Err(e) => tracing::error!("não foi possível salvar a configuração: {e}"),
    }
}

fn open_settings(cfg: Config, tx: mpsc::Sender<Config>, flag: Arc<AtomicBool>) {
    if flag.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }
        if let Err(e) = estel::ui::run(cfg, tx) {
            tracing::error!("janela de configurações: {e}");
        }
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
