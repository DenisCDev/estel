//! Local, low-frequency ambient-light sampling through an opt-in camera.

use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFMediaSource, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
    MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromMediaSource,
    MFEnumDeviceSources, MFMediaType_Video, MFSTARTUP_LITE, MFShutdown, MFStartup,
    MFVideoFormat_YUY2,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::PWSTR;

use crate::config::Config;

const MAX_PIXEL_SAMPLES: usize = 8_000;
const SMOOTHING: f32 = 0.20;

/// Lists camera names through Media Foundation without opening a video stream.
pub fn list_cameras() -> Result<Vec<String>, String> {
    let session = MediaFoundationSession::start()?;
    let devices = video_devices()?;
    let names = devices
        .iter()
        .map(camera_name)
        .collect::<Result<Vec<_>, _>>();
    drop(session);
    names
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
                let measured = factor_for_luminance(luminance);
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
    let session = MediaFoundationSession::start()?;
    let devices = video_devices()?;
    let device = devices
        .get(camera_index)
        .ok_or_else(|| "índice de câmera indisponível".to_owned())?;
    let source = unsafe {
        device
            .ActivateObject::<IMFMediaSource>()
            .map_err(|error| error.to_string())?
    };
    let reader = unsafe {
        MFCreateSourceReaderFromMediaSource(&source, None).map_err(|error| error.to_string())?
    };
    let media_type = unsafe { MFCreateMediaType().map_err(|error| error.to_string())? };
    unsafe {
        media_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|error| error.to_string())?;
        media_type
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_YUY2)
            .map_err(|error| error.to_string())?;
        reader
            .SetCurrentMediaType(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                None,
                &media_type,
            )
            .map_err(|error| error.to_string())?;
    }
    let mut sample = None;
    let mut stream_flags = 0u32;
    unsafe {
        reader
            .ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut stream_flags),
                None,
                Some(&mut sample),
            )
            .map_err(|error| error.to_string())?;
    }
    let sample = sample.ok_or_else(|| "a câmera não entregou um quadro".to_owned())?;
    let buffer = unsafe {
        sample
            .ConvertToContiguousBuffer()
            .map_err(|error| error.to_string())?
    };
    let mut data = std::ptr::null_mut();
    let mut max_length = 0;
    let mut length = 0;
    unsafe {
        buffer
            .Lock(&mut data, Some(&mut max_length), Some(&mut length))
            .map_err(|error| error.to_string())?;
    }
    let luminance = unsafe { yuy2_luminance(std::slice::from_raw_parts(data, length as usize)) };
    unsafe {
        let _ = buffer.Unlock();
    }
    drop(session);
    luminance.ok_or_else(|| "quadro de câmera vazio".to_owned())
}

struct MediaFoundationSession;

impl MediaFoundationSession {
    fn start() -> Result<Self, String> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }.map_err(|error| error.to_string())?;
        Ok(Self)
    }
}

impl Drop for MediaFoundationSession {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

fn video_devices() -> Result<Vec<IMFActivate>, String> {
    let mut attributes = None;
    unsafe { MFCreateAttributes(&mut attributes, 1) }.map_err(|error| error.to_string())?;
    let attributes =
        attributes.ok_or_else(|| "o Windows não criou os atributos da câmera".to_owned())?;
    unsafe {
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|error| error.to_string())?;
    }
    let mut raw_devices = std::ptr::null_mut();
    let mut count = 0;
    unsafe {
        MFEnumDeviceSources(&attributes, &mut raw_devices, &mut count)
            .map_err(|error| error.to_string())?;
    }
    let devices = unsafe {
        std::slice::from_raw_parts(raw_devices, count as usize)
            .iter()
            .filter_map(Clone::clone)
            .collect::<Vec<_>>()
    };
    unsafe {
        CoTaskMemFree(Some(raw_devices.cast()));
    }
    if devices.is_empty() {
        return Err("nenhuma câmera foi encontrada pelo Windows".to_owned());
    }
    Ok(devices)
}

fn camera_name(device: &IMFActivate) -> Result<String, String> {
    let mut name = PWSTR::null();
    let mut length = 0;
    unsafe {
        device
            .GetAllocatedString(
                &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
                &mut name,
                &mut length,
            )
            .map_err(|error| error.to_string())?;
    }
    let value = unsafe { name.to_string() }.map_err(|error| error.to_string());
    unsafe {
        CoTaskMemFree(Some(name.0.cast()));
    }
    value
}

#[cfg(test)]
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

fn yuy2_luminance(data: &[u8]) -> Option<f32> {
    let pixels = data.len() / 2;
    if pixels == 0 {
        return None;
    }
    let stride = (pixels / MAX_PIXEL_SAMPLES).max(1);
    let mut total = 0.0;
    let mut count = 0usize;
    for pixel in data.chunks_exact(2).step_by(stride) {
        total += pixel[0] as f32;
        count += 1;
    }
    Some((total / count as f32 / 255.0).clamp(0.0, 1.0))
}

fn factor_for_luminance(luminance: f32) -> f32 {
    const MIN_FACTOR: f32 = 0.65;
    const MAX_FACTOR: f32 = 1.25;

    let normalized = luminance.clamp(0.0, 1.0);
    let response = normalized.powf(0.55);
    MIN_FACTOR + (MAX_FACTOR - MIN_FACTOR) * response
}

#[cfg(test)]
mod tests {
    use super::{factor_for_luminance, frame_luminance, yuy2_luminance};

    #[test]
    fn measures_bgra_luminance() {
        let black = [0, 0, 0, 255];
        let white = [255, 255, 255, 255];
        assert_eq!(frame_luminance(&black), Some(0.0));
        assert_eq!(frame_luminance(&white), Some(1.0));
    }

    #[test]
    fn measures_yuy2_luminance() {
        assert_eq!(yuy2_luminance(&[0, 128, 255, 128]), Some(0.5));
    }

    #[test]
    fn maps_dark_and_very_bright_rooms_continuously() {
        assert!((factor_for_luminance(0.0) - 0.65).abs() < f32::EPSILON);
        assert!((factor_for_luminance(1.0) - 1.25).abs() < f32::EPSILON);
        assert!(factor_for_luminance(0.25) < factor_for_luminance(0.50));
        assert!(factor_for_luminance(0.50) < factor_for_luminance(0.75));
    }
}
