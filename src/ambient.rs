//! Local, low-frequency ambient-light sampling through an opt-in camera.

use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFMediaSource, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources, MFMediaType_Video, MFSTARTUP_LITE,
    MFShutdown, MFStartup, MFVideoFormat_YUY2,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::core::{PWSTR, w};

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbientSample {
    pub device_id: String,
    pub luminance: f32,
}

const MAX_PIXEL_SAMPLES: usize = 8_000;
const SMOOTHING: f32 = 0.20;

/// Lists camera names through Media Foundation without opening a video stream.
pub fn list_cameras() -> Result<Vec<String>, String> {
    let _session = MediaFoundationSession::start()?;
    let devices = video_devices()?;
    devices
        .iter()
        .map(camera_name)
        .collect::<Result<Vec<_>, _>>()
}

/// Starts the isolated ambient sampler and returns its configuration input and
/// latest brightness output. Frames never leave this thread or disk.
pub fn start(initial: Config) -> (Sender<Config>, Receiver<Result<f32, String>>) {
    let (config_tx, config_rx) = mpsc::channel();
    let (factor_tx, factor_rx) = mpsc::channel();

    thread::Builder::new()
        .name("estel-ambient".into())
        .spawn(move || run(initial, config_rx, factor_tx))
        .expect("não foi possível iniciar o sensor de luz ambiente");

    (config_tx, factor_rx)
}

fn run(mut config: Config, config_rx: Receiver<Config>, factor_tx: Sender<Result<f32, String>>) {
    let mut smoothed = None;
    let mut last_error: Option<String> = None;

    loop {
        if !config.ambient_enabled || config.preserve_colors() || !config.camera_is_calibrated() {
            smoothed = None;
            if config.ambient_enabled && !config.preserve_colors() {
                let _ = factor_tx.send(Err(
                    "Calibre a câmera no painel; por enquanto, o brilho segue o horário.".into(),
                ));
            }
            match config_rx.recv() {
                Ok(next) => {
                    config = next;
                    last_error = None;
                }
                Err(_) => return,
            }
            continue;
        }

        let reading = sample_luminance_in_helper(config.ambient_camera_index).and_then(|sample| {
            config.ambient_calibration.as_ref().and_then(|calibration| calibration.brightness(
                &sample.device_id, sample.luminance, config.ambient_brightness_min, config.ambient_brightness_max,
            )).ok_or_else(|| "A câmera saiu da faixa calibrada ou mudou de dispositivo; confira a calibração no painel.".to_owned())
        });
        match reading {
            Ok(measured) => {
                let next = smooth_factor(
                    smoothed,
                    measured,
                    config.ambient_brightness_min,
                    config.ambient_brightness_max,
                );
                smoothed = Some(next);
                if factor_tx.send(Ok(next)).is_err() {
                    return;
                }
                last_error = None;
            }
            Err(error) => {
                smoothed = None;
                if last_error.as_deref() != Some(error.as_str()) {
                    tracing::warn!(%error, "sensor de luz ambiente indisponível");
                }
                last_error = Some(error.clone());
                if factor_tx.send(Err(error)).is_err() {
                    return;
                }
            }
        }

        match config_rx.recv_timeout(Duration::from_secs(config.ambient_sample_interval_seconds)) {
            Ok(next) => {
                let reset = smoothing_source_changed(&config, &next);
                config = next;
                if reset {
                    smoothed = None;
                    last_error = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn smoothing_source_changed(old: &Config, new: &Config) -> bool {
    old.ambient_enabled != new.ambient_enabled
        || old.ambient_calibration != new.ambient_calibration
        || old.ambient_camera_index != new.ambient_camera_index
        || old.ambient_brightness_min != new.ambient_brightness_min
        || old.ambient_brightness_max != new.ambient_brightness_max
        || old.preserve_colors() != new.preserve_colors()
}

pub fn sample_luminance_in_helper(camera_index: usize) -> Result<AmbientSample, String> {
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
                return Err(helper_error(&output.stderr));
            }
            let output = String::from_utf8(output.stdout)
                .map_err(|_| "o Windows retornou uma leitura de câmera inválida".to_owned())?;
            let sample: AmbientSample = serde_json::from_str(output.trim())
                .map_err(|_| "a câmera retornou uma medida de luz inválida".to_owned())?;
            if sample.device_id.is_empty()
                || sample.device_id.len() > 4096
                || !sample.luminance.is_finite()
                || !(0.02..=0.98).contains(&sample.luminance)
            {
                return Err("A câmera retornou uma leitura inválida ou saturada.".into());
            }
            return Ok(sample);
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

fn helper_error(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr);
    if message.contains("oscilou") {
        "A leitura oscilou. Mantenha o enquadramento e a iluminação estáveis e tente novamente."
            .into()
    } else if message.contains("saturação") {
        "A câmera não forneceu quadros válidos sem saturação. Tente novamente com luz difusa."
            .into()
    } else if message.contains("em andamento") {
        "Já há uma leitura em andamento. Aguarde alguns segundos e tente novamente.".into()
    } else if message.contains("0xC00D3704") {
        "A câmera não iniciou e pode estar em uso por outro aplicativo. Feche o aplicativo que a utiliza ou deixe o brilho pela câmera desligado.".to_owned()
    } else {
        "A câmera não iniciou. Confira as permissões no Windows ou escolha outra câmera.".to_owned()
    }
}

pub fn sample_luminance(camera_index: usize) -> Result<AmbientSample, String> {
    let _capture = CameraCapture::acquire()?;
    let _session = MediaFoundationSession::start()?;
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
    let started = Instant::now();
    let mut readings = Vec::with_capacity(5);
    for _ in 0..90 {
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
        if started.elapsed() < Duration::from_millis(500) {
            continue;
        }
        let Some(sample) = sample else {
            continue;
        };
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
        let luminance = if length == 0 || data.is_null() {
            None
        } else {
            unsafe { yuy2_luminance(std::slice::from_raw_parts(data, length as usize)) }
        };
        unsafe { buffer.Unlock() }.map_err(|error| error.to_string())?;
        readings.push(luminance.ok_or_else(|| "quadro de câmera vazio".to_owned())?);
        if readings.len() == 5 {
            break;
        }
    }
    let luminance = stable_luminance(&readings)?;
    Ok(AmbientSample {
        device_id: camera_attribute(
            device,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
        )?,
        luminance,
    })
}

struct CameraCapture(HANDLE);

impl CameraCapture {
    fn acquire() -> Result<Self, String> {
        let handle = unsafe { CreateMutexW(None, false, w!("Local\\EstelAmbientCapture")) }
            .map_err(|error| error.to_string())?;
        let result = unsafe { WaitForSingleObject(handle, 1000) };
        if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED {
            unsafe { CloseHandle(handle) }.map_err(|error| error.to_string())?;
            return Err(
                "Já há uma leitura em andamento. Aguarde alguns segundos e tente novamente.".into(),
            );
        }
        Ok(Self(handle))
    }
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        if let Err(error) = unsafe { ReleaseMutex(self.0) } {
            tracing::warn!(%error, "camera capture release failed");
        }
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            tracing::warn!(%error, "camera capture handle close failed");
        }
    }
}

fn stable_luminance(readings: &[f32]) -> Result<f32, String> {
    if readings.len() != 5
        || readings
            .iter()
            .any(|value| !value.is_finite() || !(0.02..=0.98).contains(value))
    {
        return Err("A câmera não forneceu cinco quadros válidos sem saturação. Tente novamente com luz difusa.".into());
    }
    let min = readings.iter().copied().fold(1.0_f32, f32::min);
    let max = readings.iter().copied().fold(0.0_f32, f32::max);
    if max - min > 0.05 {
        return Err("A leitura da câmera oscilou. Mantenha o enquadramento e a iluminação estáveis e tente novamente.".into());
    }
    Ok(readings.iter().sum::<f32>() / readings.len() as f32)
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
    if count == 0 || raw_devices.is_null() {
        return Err("nenhuma câmera foi encontrada pelo Windows".to_owned());
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
    camera_attribute(device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
}

fn camera_attribute(
    device: &IMFActivate,
    attribute: &windows::core::GUID,
) -> Result<String, String> {
    let mut name = PWSTR::null();
    let mut length = 0;
    unsafe {
        device
            .GetAllocatedString(attribute, &mut name, &mut length)
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

    let stride = sample_stride(pixels);
    let mut total = 0.0;
    let mut count = 0usize;
    for pixel in data.as_chunks::<4>().0.iter().step_by(stride) {
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
    let stride = sample_stride(pixels);
    let mut total = 0.0;
    let mut count = 0usize;
    for pixel in data.as_chunks::<2>().0.iter().step_by(stride) {
        total += pixel[0] as f32;
        count += 1;
    }
    Some((total / count as f32 / 255.0).clamp(0.0, 1.0))
}

fn sample_stride(pixels: usize) -> usize {
    pixels.div_ceil(MAX_PIXEL_SAMPLES).max(1)
}

fn smooth_factor(previous: Option<f32>, measured: f32, min_factor: f32, max_factor: f32) -> f32 {
    previous
        .map_or(measured, |value| {
            if (measured - value).abs() < 0.02 {
                value
            } else {
                value + (measured - value) * SMOOTHING
            }
        })
        .clamp(min_factor, max_factor)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PIXEL_SAMPLES, frame_luminance, helper_error, sample_stride, smooth_factor,
        smoothing_source_changed, stable_luminance, yuy2_luminance,
    };
    use crate::config::Config;

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
    fn busy_camera_error_suggests_releasing_the_device() {
        let message = helper_error(b"camera failed (0xC00D3704)");
        assert!(message.contains("pode estar em uso"));
    }

    #[test]
    fn rejects_unstable_saturated_and_nonfinite_camera_readings() {
        for readings in [[0.2, 0.2, 0.4, 0.2, 0.2], [1.0; 5], [f32::NAN; 5]] {
            assert!(stable_luminance(&readings).is_err());
        }
        assert!((stable_luminance(&[0.3, 0.31, 0.3, 0.32, 0.3]).unwrap() - 0.306).abs() < 0.001);
    }

    #[test]
    fn tiny_camera_fluctuations_do_not_move_brightness() {
        assert_eq!(smooth_factor(Some(0.5), 0.51, 0.25, 0.85), 0.5);
    }

    #[test]
    fn changing_only_camera_interval_keeps_previous_measurement() {
        let current = Config::default();
        let mut next = current.clone();
        next.ambient_sample_interval_seconds = 10;
        assert!(!smoothing_source_changed(&current, &next));
        next.ambient_camera_index = current.ambient_camera_index + 1;
        assert!(smoothing_source_changed(&current, &next));
    }

    #[test]
    fn first_camera_reading_respects_configured_limit() {
        let measured = 0.35;
        assert!(smooth_factor(None, measured, 0.35, 0.35) <= 0.35);
    }

    #[test]
    fn samples_at_most_eight_thousand_pixels() {
        let pixels: usize = 640 * 480;
        assert!(pixels.div_ceil(sample_stride(pixels)) <= MAX_PIXEL_SAMPLES);
    }
}
