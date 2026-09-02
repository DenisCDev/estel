//! Local, low-frequency ambient-light sampling through an opt-in camera.

use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ccap::{PixelFormat, PropertyName, Provider};

use crate::config::Config;

const FRAME_TIMEOUT_MS: u32 = 1_000;
const MAX_PIXEL_SAMPLES: usize = 8_000;
const SMOOTHING: f32 = 0.20;

/// Lists names reported by Windows without opening a video stream.
pub fn list_cameras() -> Result<Vec<String>, String> {
    let provider = Provider::new().map_err(|error| error.to_string())?;
    provider.list_devices().map_err(|error| error.to_string())
}

/// Starts the isolated ambient sampler and returns its configuration input and
/// latest brightness-factor output. Frames never leave this thread or disk.
pub fn start(initial: Config) -> (Sender<Config>, Receiver<f32>) {
    let (config_tx, config_rx) = mpsc::channel();
    let (factor_tx, factor_rx) = mpsc::channel();

    thread::Builder::new()
        .name("estel-ambient".into())
        .spawn(move || run(initial, config_rx, factor_tx))
        .expect("não foi possível iniciar o sensor de luz ambiente");

    (config_tx, factor_rx)
}

fn run(mut config: Config, config_rx: Receiver<Config>, factor_tx: Sender<f32>) {
    let mut smoothed = 1.0;
    let mut last_error: Option<String> = None;

    loop {
        if !config.ambient_enabled {
            if factor_tx.send(1.0).is_err() {
                return;
            }
            match config_rx.recv() {
                Ok(next) => config = next,
                Err(_) => return,
            }
            continue;
        }

        match sample_luminance_in_helper(config.ambient_camera_index) {
            Ok(luminance) => {
                let measured = factor_for_luminance(luminance, &config);
                smoothed += (measured - smoothed) * SMOOTHING;
                if factor_tx.send(smoothed).is_err() {
                    return;
                }
                last_error = None;
            }
            Err(error) => {
                if last_error.as_deref() != Some(error.as_str()) {
                    tracing::warn!(%error, "sensor de luz ambiente indisponível");
                    last_error = Some(error);
                }
            }
        }

        match config_rx.recv_timeout(Duration::from_secs(config.ambient_sample_interval_seconds)) {
            Ok(next) => config = next,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn sample_luminance_in_helper(camera_index: usize) -> Result<f32, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("não foi possível localizar o Estel ({error})"))?;
    let mut child = Command::new(executable)
        .arg("--sample-ambient")
        .arg(camera_index.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("não foi possível iniciar a leitura da câmera ({error})"))?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        if child
            .try_wait()
            .map_err(|error| format!("não foi possível ler a câmera ({error})"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("não foi possível ler a câmera ({error})"))?;
            if !output.status.success() {
                return Err("o driver da câmera não respondeu com segurança".to_owned());
            }
            let output = String::from_utf8(output.stdout)
                .map_err(|_| "o Windows retornou uma leitura de câmera inválida".to_owned())?;
            return output
                .trim()
                .parse::<f32>()
                .map(|value| value.clamp(0.0, 1.0))
                .map_err(|_| "a câmera retornou uma medida de luz inválida".to_owned());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "a leitura da câmera demorou mais de 5 segundos e foi cancelada".to_owned(),
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub fn sample_luminance(camera_index: usize) -> Result<f32, String> {
    let camera_index =
        i32::try_from(camera_index).map_err(|_| "índice de câmera inválido".to_owned())?;
    let mut provider = Provider::with_device(camera_index).map_err(|error| error.to_string())?;

    if let Err(error) = provider.set_property(
        PropertyName::PixelFormatOutput,
        PixelFormat::Bgra32 as u32 as f64,
    ) {
        tracing::debug!(%error, "câmera não aceitou BGRA; usando formato padrão");
    }
    provider.open().map_err(|error| error.to_string())?;
    provider.start().map_err(|error| error.to_string())?;

    let frame = provider
        .grab_frame(FRAME_TIMEOUT_MS)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "a câmera não entregou um quadro a tempo".to_owned())?;
    if frame.pixel_format() != PixelFormat::Bgra32 {
        return Err("a câmera não forneceu um quadro BGRA compatível".to_owned());
    }
    frame_luminance(frame.data().map_err(|error| error.to_string())?)
        .ok_or_else(|| "quadro de câmera vazio".to_owned())
}

fn frame_luminance(data: &[u8]) -> Option<f32> {
    let pixels = data.len() / 4;
    if pixels == 0 {
        return None;
    }

    let stride = (pixels / MAX_PIXEL_SAMPLES).max(1);
    let mut total = 0.0;
    let mut count = 0usize;
    for pixel in data.chunks_exact(4).step_by(stride) {
        let blue = pixel[0] as f32;
        let green = pixel[1] as f32;
        let red = pixel[2] as f32;
        total += 0.2126 * red + 0.7152 * green + 0.0722 * blue;
        count += 1;
    }
    Some((total / count as f32 / 255.0).clamp(0.0, 1.0))
}

fn factor_for_luminance(luminance: f32, config: &Config) -> f32 {
    let normalized = luminance.clamp(0.0, 1.0);
    config.ambient_brightness_min
        + (config.ambient_brightness_max - config.ambient_brightness_min) * normalized
}

#[cfg(test)]
mod tests {
    use super::{factor_for_luminance, frame_luminance};
    use crate::config::Config;

    #[test]
    fn measures_bgra_luminance() {
        let black = [0, 0, 0, 255];
        let white = [255, 255, 255, 255];
        assert_eq!(frame_luminance(&black), Some(0.0));
        assert_eq!(frame_luminance(&white), Some(1.0));
    }

    #[test]
    fn maps_dark_and_bright_rooms_to_user_limits() {
        let config = Config {
            ambient_brightness_min: 0.70,
            ambient_brightness_max: 1.10,
            ..Config::default()
        };
        assert!((factor_for_luminance(0.0, &config) - 0.70).abs() < f32::EPSILON);
        assert!((factor_for_luminance(1.0, &config) - 1.10).abs() < f32::EPSILON);
    }
}
