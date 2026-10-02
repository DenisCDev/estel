//! Local, opt-in ambient sampling through a dedicated sensor or camera.

#[path = "light_sensor.rs"]
mod light_sensor;
pub use light_sensor::LightSample;

use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::Media::DirectShow::{
    CameraControl_Exposure, IAMCameraControl, IAMVideoProcAmp,
};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFActivate, IMFMediaBuffer, IMFMediaSource, IMFMediaType, IMFSourceReader,
    MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_E_NO_MORE_TYPES,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_VERSION, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources, MFMediaType_Video, MFSTARTUP_LITE,
    MFShutdown, MFStartup, MFVideoFormat_MJPG, MFVideoFormat_NV12, MFVideoFormat_YUY2,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::core::{Interface, PWSTR, w};

use crate::config::Config;
use crate::runtime::WakeSignal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbientSample {
    pub device_id: String,
    pub luminance: f32,
    pub capture_profile: String,
    pub mode_description: String,
}

pub enum AmbientCommand {
    Configure(Box<Config>),
    Suspended(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientSource {
    Camera,
    LightSensor,
}

pub struct AmbientReading {
    pub brightness: f32,
    pub source: AmbientSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraInfo {
    pub name: String,
    pub device_id: String,
}

const MAX_PIXEL_SAMPLES: usize = 8_000;
const SMOOTHING: f32 = 0.20;

/// Lists camera names through Media Foundation without opening a video stream.
pub fn list_cameras() -> Result<Vec<String>, String> {
    Ok(list_camera_devices()?
        .into_iter()
        .map(|camera| camera.name)
        .collect())
}

pub fn list_camera_devices() -> Result<Vec<CameraInfo>, String> {
    let _session = MediaFoundationSession::start()?;
    let devices = video_devices()?;
    devices
        .iter()
        .map(|device| {
            Ok(CameraInfo {
                name: camera_name(device)?,
                device_id: camera_attribute(
                    device,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                )?,
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

/// Starts the isolated ambient sampler and returns its configuration input and
/// latest brightness output. Frames never leave this thread or disk.
pub fn start(
    initial: Config,
    wake: WakeSignal,
) -> (
    Sender<AmbientCommand>,
    Receiver<Result<AmbientReading, String>>,
) {
    let (config_tx, config_rx) = mpsc::channel();
    let (factor_tx, factor_rx) = mpsc::channel();
    thread::Builder::new()
        .name("estel-ambient".into())
        .spawn(move || run(initial, config_rx, factor_tx, wake))
        .expect("não foi possível iniciar o sensor de luz ambiente");
    (config_tx, factor_rx)
}

fn run(
    mut config: Config,
    config_rx: Receiver<AmbientCommand>,
    factor_tx: Sender<Result<AmbientReading, String>>,
    wake: WakeSignal,
) {
    let mut smoothed = None;
    let mut last_error: Option<String> = None;
    // The session owner must provide the initial display/lock state first.
    let mut suspended = true;
    let mut last_source = None;
    let mut sensor_retry = Instant::now();
    loop {
        if suspended || !config.ambient_enabled || config.preserve_colors() {
            smoothed = None;
            match config_rx.recv() {
                Ok(next) => {
                    if apply_command(next, &mut config, &mut suspended) {
                        sensor_retry = Instant::now();
                        last_source = None;
                    }
                    last_error = None;
                }
                Err(_) => return,
            }
            continue;
        }
        let reading = ambient_reading(&config, &mut sensor_retry);
        // Do not publish a capture completed under obsolete session preferences.
        let mut changed = false;
        while let Ok(next) = config_rx.try_recv() {
            changed |= apply_command(next, &mut config, &mut suspended);
        }
        if changed {
            smoothed = None;
            last_source = None;
            sensor_retry = Instant::now();
            continue;
        }
        let result = match reading {
            Ok(measured) => {
                if last_source != Some(measured.source) {
                    smoothed = None;
                }
                last_source = Some(measured.source);
                let next = smooth_factor(
                    smoothed,
                    measured.brightness,
                    config.ambient_brightness_min,
                    config.ambient_brightness_max,
                );
                smoothed = Some(next);
                last_error = None;
                Ok(AmbientReading {
                    brightness: next,
                    source: measured.source,
                })
            }
            Err(error) => {
                smoothed = None;
                if last_error.as_deref() != Some(error.as_str()) {
                    tracing::warn!(%error, "ambient light reading unavailable");
                }
                last_error = Some(error.clone());
                Err(error)
            }
        };
        if factor_tx.send(result).is_err() {
            return;
        }
        wake.notify();
        let waiting_since = Instant::now();
        loop {
            let interval = Duration::from_secs(config.ambient_sample_interval_seconds.max(10));
            match config_rx.recv_timeout(interval.saturating_sub(waiting_since.elapsed())) {
                Ok(next) => {
                    if apply_command(next, &mut config, &mut suspended) {
                        smoothed = None;
                        last_error = None;
                        sensor_retry = Instant::now();
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

fn apply_command(command: AmbientCommand, config: &mut Config, suspended: &mut bool) -> bool {
    match command {
        AmbientCommand::Configure(next) => {
            let changed = smoothing_source_changed(config, &next);
            *config = *next;
            changed
        }
        AmbientCommand::Suspended(next) => {
            let changed = *suspended != next;
            *suspended = next;
            changed
        }
    }
}

fn ambient_reading(config: &Config, sensor_retry: &mut Instant) -> Result<AmbientReading, String> {
    if config.ambient_prefer_light_sensor && Instant::now() >= *sensor_retry {
        match sample_light_sensor_in_helper() {
            Ok(Some(sample)) => {
                return Ok(AmbientReading {
                    brightness: sample
                        .brightness(config.ambient_brightness_min, config.ambient_brightness_max),
                    source: AmbientSource::LightSensor,
                });
            }
            Ok(None) => *sensor_retry = Instant::now() + Duration::from_secs(300),
            Err(error) => {
                tracing::warn!(%error, "dedicated light sensor unavailable; using calibrated camera");
                *sensor_retry = Instant::now() + Duration::from_secs(300);
            }
        }
    }
    if !config.camera_is_calibrated() {
        return Err("Sem sensor de luz disponível: calibre ou refaça as referências da câmera; por enquanto, o brilho segue o horário e o clima opcional.".into());
    }
    let calibration = config
        .ambient_calibration
        .as_ref()
        .ok_or_else(|| "Calibre a câmera no painel.".to_owned())?;
    let sample =
        sample_luminance_for_device(config.ambient_camera_index, Some(&calibration.device_id))?;
    if !calibration.matches_capture(&sample.capture_profile) {
        return Err(
            "O formato ou os controles da câmera mudaram. Refaça as referências no painel.".into(),
        );
    }
    let brightness = calibration.brightness(&sample.device_id, sample.luminance,
        config.ambient_brightness_min, config.ambient_brightness_max)
        .ok_or_else(|| "A câmera saiu da faixa calibrada ou mudou de dispositivo; confira a calibração no painel.".to_owned())?;
    Ok(AmbientReading {
        brightness,
        source: AmbientSource::Camera,
    })
}

fn smoothing_source_changed(old: &Config, new: &Config) -> bool {
    old.ambient_enabled != new.ambient_enabled
        || old.ambient_prefer_light_sensor != new.ambient_prefer_light_sensor
        || old.ambient_calibration != new.ambient_calibration
        || old.ambient_camera_index != new.ambient_camera_index
        || old.ambient_camera_id != new.ambient_camera_id
        || old.ambient_brightness_min != new.ambient_brightness_min
        || old.ambient_brightness_max != new.ambient_brightness_max
        || old.preserve_colors() != new.preserve_colors()
}

pub fn sample_luminance_in_helper(camera_index: usize) -> Result<AmbientSample, String> {
    sample_luminance_for_device(camera_index, None)
}

pub fn sample_luminance_for_device(
    camera_index: usize,
    device_id: Option<&str>,
) -> Result<AmbientSample, String> {
    let index = camera_index.to_string();
    let mut arguments = vec!["--sample-ambient", index.as_str()];
    if let Some(device_id) = device_id {
        arguments.extend(["--camera-id", device_id]);
    }
    let output = run_helper(&arguments, true)?;
    let sample: AmbientSample = serde_json::from_slice(&output)
        .map_err(|_| "A câmera retornou uma medida de luz inválida.".to_owned())?;
    if sample.device_id.is_empty()
        || sample.device_id.len() > 4096
        || sample.capture_profile.is_empty()
        || sample.capture_profile.len() > 4096
        || sample.mode_description.len() > 1024
        || !sample.luminance.is_finite()
        || !(0.02..=0.98).contains(&sample.luminance)
    {
        return Err("A câmera retornou uma leitura inválida ou saturada.".into());
    }
    Ok(sample)
}

pub fn sample_light_sensor() -> Result<Option<LightSample>, String> {
    light_sensor::sample()
}

pub fn sample_light_sensor_in_helper() -> Result<Option<LightSample>, String> {
    let output = run_helper(&["--sample-light-sensor"], false)?;
    let sample: Option<LightSample> = serde_json::from_slice(&output)
        .map_err(|_| "O sensor de luz retornou uma resposta inválida.".to_owned())?;
    if sample.as_ref().is_some_and(|sample| !sample.is_valid()) {
        return Err("O sensor de luz retornou uma medida inválida.".into());
    }
    Ok(sample)
}

fn run_helper(arguments: &[&str], camera: bool) -> Result<Vec<u8>, String> {
    use std::os::windows::process::CommandExt;
    let executable = std::env::current_exe()
        .map_err(|error| format!("Não foi possível localizar o Estel ({error})."))?;
    let child = Command::new(executable)
        .args(arguments)
        .creation_flags(0x0800_0000)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Não foi possível iniciar a leitura de luz ({error})."))?;
    let mut child = HelperProcess(Some(child));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let process = child
            .0
            .as_mut()
            .ok_or_else(|| "A leitura foi encerrada.".to_owned())?;
        if process
            .try_wait()
            .map_err(|error| format!("Não foi possível ler o sensor de luz ({error})."))?
            .is_some()
        {
            let output = child
                .0
                .take()
                .ok_or_else(|| "A leitura foi encerrada.".to_owned())?
                .wait_with_output()
                .map_err(|error| format!("Não foi possível ler o sensor de luz ({error})."))?;
            if !output.status.success() {
                return Err(if camera {
                    helper_error(&output.stderr)
                } else {
                    "O sensor de luz do Windows não forneceu uma leitura atual.".to_owned()
                });
            }
            if output.stdout.len() > 16_384 {
                return Err("O sensor de luz retornou uma resposta grande demais.".into());
            }
            return Ok(output.stdout);
        }
        if Instant::now() >= deadline {
            return Err("A leitura de luz demorou mais de 5 segundos e foi cancelada.".to_owned());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

struct HelperProcess(Option<std::process::Child>);

impl Drop for HelperProcess {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            if let Err(error) = child.kill() {
                tracing::warn!(%error, "ambient helper cancellation failed");
            }
            // TerminateProcess can outlive its return while driver I/O is cancelled.
            if let Err(error) = child.try_wait() {
                tracing::warn!(%error, "ambient helper cleanup failed");
            }
        }
    }
}

fn helper_error(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr);
    if message.contains("modo YUY2") {
        "A câmera não oferece um modo de captura compatível; selecione outra câmera ou use o sensor de luz do Windows.".into()
    } else if message.contains("controles da câmera") || message.contains("formato da câmera") {
        "O formato ou os controles da câmera mudaram durante a leitura. Tente novamente.".into()
    } else if message.contains("oscilou") {
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

pub fn sample_luminance(
    camera_index: usize,
    expected: Option<&str>,
) -> Result<AmbientSample, String> {
    let _capture = CameraCapture::acquire()?;
    let _session = MediaFoundationSession::start()?;
    let devices = video_devices()?;
    let selected_index = if let Some(expected) = expected {
        let identities = devices
            .iter()
            .map(|device| {
                camera_attribute(
                    device,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        select_camera_index(&identities, camera_index, Some(expected))
    } else {
        Some(camera_index)
    };
    let device = selected_index
        .and_then(|index| devices.get(index))
        .ok_or_else(|| "índice de câmera indisponível".to_owned())?;
    let source = CameraSource(unsafe {
        device
            .ActivateObject::<IMFMediaSource>()
            .map_err(|error| error.to_string())?
    });
    let reader = unsafe {
        MFCreateSourceReaderFromMediaSource(&source.0, None).map_err(|error| error.to_string())?
    };
    let format = select_capture_format(&reader)?;
    let (controls, confidence) = camera_controls(&source.0);
    let capture_profile = format!("v1:{}:{controls}", format.profile());
    let mode_description = format!(
        "{} × {} · {:.1} fps · {} · {confidence}",
        format.width,
        format.height,
        format.frames_per_second(),
        format.kind.label()
    );
    let started = Instant::now();
    let mut readings = Vec::with_capacity(5);
    for _ in 0..120 {
        let mut sample = None;
        let mut flags = 0u32;
        unsafe {
            reader
                .ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
                .map_err(|error| error.to_string())?;
        }
        if flags
            & (MF_SOURCE_READERF_ERROR.0
                | MF_SOURCE_READERF_ENDOFSTREAM.0
                | MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0) as u32
            != 0
        {
            return Err(
                "O formato da câmera mudou ou a captura foi interrompida. Tente novamente.".into(),
            );
        }
        if started.elapsed() >= Duration::from_secs(4) {
            break;
        }
        if started.elapsed() < Duration::from_millis(500) {
            continue;
        }
        let Some(sample) = sample else { continue };
        let buffer = unsafe {
            sample
                .ConvertToContiguousBuffer()
                .map_err(|error| error.to_string())?
        };
        let luminance = buffer_luminance(&buffer, format)?;
        if push_stable_reading(&mut readings, luminance).is_some() {
            break;
        }
    }
    let luminance = stable_luminance(&readings)?;
    if camera_controls(&source.0).0 != controls {
        return Err("Os controles da câmera mudaram durante a leitura. Tente novamente.".into());
    }
    Ok(AmbientSample {
        device_id: camera_attribute(
            device,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
        )?,
        luminance,
        capture_profile,
        mode_description,
    })
}

struct CameraSource(IMFMediaSource);

impl Drop for CameraSource {
    fn drop(&mut self) {
        if let Err(error) = unsafe { self.0.Shutdown() } {
            tracing::warn!(%error, "camera source shutdown failed");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PixelFormat {
    Yuy2,
    Nv12,
}

impl PixelFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Yuy2 => "YUY2",
            Self::Nv12 => "NV12",
        }
    }

    fn pixel_stride(self) -> usize {
        match self {
            Self::Yuy2 => 2,
            Self::Nv12 => 1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CaptureFormat {
    width: u32,
    height: u32,
    rate_numerator: u32,
    rate_denominator: u32,
    row_stride: usize,
    kind: PixelFormat,
    decoded_mjpeg: bool,
    nominal_range: u32,
}

impl CaptureFormat {
    fn frames_per_second(self) -> f64 {
        self.rate_numerator as f64 / self.rate_denominator as f64
    }

    fn valid(self) -> bool {
        (80..=4096).contains(&self.width)
            && self.width % 2 == 0
            && (60..=2160).contains(&self.height)
            && self.height % 2 == 0
            && self.rate_denominator != 0
            && (5.0..=60.0).contains(&self.frames_per_second())
            && self.row_stride >= self.width as usize * self.kind.pixel_stride()
            && self.row_stride <= 16_384
    }

    fn bandwidth(self) -> f64 {
        let bytes_per_pixel = match self.kind {
            PixelFormat::Yuy2 => 2.0,
            PixelFormat::Nv12 => 1.5,
        };
        self.width as f64 * self.height as f64 * self.frames_per_second() * bytes_per_pixel
    }

    fn profile(self) -> String {
        format!(
            "{}:{}x{}:{}/{}:range={}{}",
            self.kind.label(),
            self.width,
            self.height,
            self.rate_numerator,
            self.rate_denominator,
            self.nominal_range,
            if self.decoded_mjpeg { ":MJPG" } else { "" }
        )
    }

    fn normalize_luma(self, average: f32) -> f32 {
        let (black, white) = match self.nominal_range {
            2 => (16.0, 235.0),
            3 => (48.0, 208.0),
            4 => (64.0, 127.0),
            // Unknown range keeps the raw relative scale without inventing sensor precision.
            _ => (0.0, 255.0),
        };
        ((average - black) / (white - black)).clamp(0.0, 1.0)
    }
}

fn describe_media_type(media_type: &IMFMediaType) -> Result<Option<CaptureFormat>, String> {
    let subtype =
        unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }.map_err(|error| error.to_string())?;
    let kind = if subtype == MFVideoFormat_YUY2 {
        PixelFormat::Yuy2
    } else if subtype == MFVideoFormat_NV12 {
        PixelFormat::Nv12
    } else {
        return Ok(None);
    };
    let dimensions =
        unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }.map_err(|error| error.to_string())?;
    let frame_rate =
        unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) }.map_err(|error| error.to_string())?;
    let width = (dimensions >> 32) as u32;
    // Absent stride means the subtype's tightly packed default layout.
    let row_stride = unsafe { media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
        .map_or(width as usize * kind.pixel_stride(), |value| value as usize);
    // Many webcam drivers omit range metadata; the profile preserves that uncertainty.
    let nominal_range = unsafe { media_type.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE) }.unwrap_or(0);
    let format = CaptureFormat {
        width,
        height: dimensions as u32,
        rate_numerator: (frame_rate >> 32) as u32,
        rate_denominator: frame_rate as u32,
        row_stride,
        kind,
        decoded_mjpeg: false,
        nominal_range,
    };
    Ok(format.valid().then_some(format))
}

fn select_capture_format(reader: &IMFSourceReader) -> Result<CaptureFormat, String> {
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let mut candidates = Vec::new();
    for index in 0..512 {
        let media_type = match unsafe { reader.GetNativeMediaType(stream, index) } {
            Ok(value) => value,
            Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
            Err(error) => return Err(error.to_string()),
        };
        if let Some(format) = describe_media_type(&media_type)? {
            candidates.push((format, media_type, None));
        } else if unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }
            .is_ok_and(|subtype| subtype == MFVideoFormat_MJPG)
        {
            let decoded = decoded_media_type(&media_type)?;
            if let Some(format) = describe_media_type(&decoded)? {
                candidates.push((
                    CaptureFormat {
                        decoded_mjpeg: true,
                        ..format
                    },
                    media_type,
                    Some(decoded),
                ));
            }
        }
    }
    // Avoid decoding when a directly readable native mode exists.
    candidates.sort_by(|(left, _, _), (right, _, _)| {
        left.decoded_mjpeg
            .cmp(&right.decoded_mjpeg)
            .then_with(|| left.bandwidth().total_cmp(&right.bandwidth()))
    });
    unsafe {
        reader
            .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
            .map_err(|error| error.to_string())?;
        reader
            .SetStreamSelection(stream, true)
            .map_err(|error| error.to_string())?;
    }
    for (selected, media_type, decoded) in candidates {
        match unsafe { reader.SetCurrentMediaType(stream, None, &media_type) } {
            Ok(()) => {
                if let Some(decoded) = decoded
                    && let Err(error) =
                        unsafe { reader.SetCurrentMediaType(stream, None, &decoded) }
                {
                    tracing::debug!(%error, "camera decoder rejected capture mode");
                    continue;
                }
                let actual = unsafe { reader.GetCurrentMediaType(stream) }
                    .map_err(|error| error.to_string())?;
                return describe_media_type(&actual)?
                    .map(|format| CaptureFormat {
                        decoded_mjpeg: selected.decoded_mjpeg,
                        ..format
                    })
                    .ok_or_else(|| "A câmera mudou para um formato incompatível.".to_owned());
            }
            Err(error) => tracing::debug!(%error, "camera rejected advertised capture mode"),
        }
    }
    Err(
        "A câmera não oferece um modo YUY2, NV12 ou MJPG adequado para uma leitura breve de luz."
            .into(),
    )
}

fn decoded_media_type(native: &IMFMediaType) -> Result<IMFMediaType, String> {
    let decoded = unsafe { MFCreateMediaType() }.map_err(|error| error.to_string())?;
    unsafe {
        decoded
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|error| error.to_string())?;
        decoded
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_YUY2)
            .map_err(|error| error.to_string())?;
        for key in [&MF_MT_FRAME_SIZE, &MF_MT_FRAME_RATE] {
            let value = native.GetUINT64(key).map_err(|error| error.to_string())?;
            decoded
                .SetUINT64(key, value)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(decoded)
}

fn camera_controls(source: &IMFMediaSource) -> (String, &'static str) {
    // Optional driver controls are diagnostic; unsupported controls remain explicit.
    let exposure = source.cast::<IAMCameraControl>().ok().and_then(|control| {
        let (mut value, mut flags) = (0, 0);
        unsafe { control.Get(CameraControl_Exposure.0, &mut value, &mut flags) }
            .ok()
            .map(|()| (value, flags))
    });
    let mut profile = format!("exposure={}", control_profile(exposure));
    if let Ok(control) = source.cast::<IAMVideoProcAmp>() {
        for property in 0..10 {
            let (mut value, mut flags) = (0, 0);
            let setting = unsafe { control.Get(property, &mut value, &mut flags) }
                .ok()
                .map(|()| (value, flags));
            profile.push_str(&format!(
                ";processing{property}={}",
                control_profile(setting)
            ));
        }
    } else {
        profile.push_str(";processing=unknown");
    }
    let confidence = match exposure {
        Some((_, flags)) if flags & 1 == 0 => "exposição manual; leitura relativa",
        Some(_) => "exposição automática; estimativa limitada",
        None => "exposição desconhecida; estimativa limitada",
    };
    (profile, confidence)
}

fn control_profile(setting: Option<(i32, i32)>) -> String {
    match setting {
        Some((_, flags)) if flags & 1 != 0 => format!("auto:{flags}"),
        Some((value, flags)) => format!("fixed:{value}:{flags}"),
        None => "unknown".into(),
    }
}

fn buffer_luminance(buffer: &IMFMediaBuffer, format: CaptureFormat) -> Result<f32, String> {
    if let Ok(two_dimensional) = buffer.cast::<IMF2DBuffer>() {
        let length =
            unsafe { two_dimensional.GetContiguousLength() }.map_err(|error| error.to_string())?;
        let minimum = format.width as usize * format.height as usize * format.kind.pixel_stride();
        if (length as usize) < minimum || length > 32 * 1024 * 1024 {
            return Err("A câmera retornou um quadro incompleto ou grande demais.".into());
        }
        let (mut data, mut pitch) = (std::ptr::null_mut(), 0i32);
        unsafe { two_dimensional.Lock2D(&mut data, &mut pitch) }
            .map_err(|error| error.to_string())?;
        let width = format.width as usize;
        let pixels = width * format.height as usize;
        let pitch_bytes = pitch.unsigned_abs() as usize;
        let luminance = if data.is_null()
            || pitch_bytes < width * format.kind.pixel_stride()
            || pitch_bytes > 16_384
        {
            None
        } else {
            let mut total = 0u64;
            let mut count = 0usize;
            for pixel in (0..pixels).step_by(sample_stride(pixels)) {
                let row = pixel / width;
                let column = pixel % width;
                // Lock2D exposes the top row and its actual signed pitch, including padding.
                let offset =
                    row as isize * pitch as isize + (column * format.kind.pixel_stride()) as isize;
                total += unsafe { *data.offset(offset) } as u64;
                count += 1;
            }
            Some(format.normalize_luma(total as f32 / count as f32))
        };
        unsafe { two_dimensional.Unlock2D() }.map_err(|error| error.to_string())?;
        return luminance.ok_or_else(|| "A câmera retornou um quadro incompleto.".into());
    }
    let mut data = std::ptr::null_mut();
    let mut length = 0;
    unsafe { buffer.Lock(&mut data, None, Some(&mut length)) }
        .map_err(|error| error.to_string())?;
    let luminance = if data.is_null() || length == 0 || length > 32 * 1024 * 1024 {
        None
    } else {
        planar_luminance(
            unsafe { std::slice::from_raw_parts(data, length as usize) },
            format,
        )
    };
    unsafe { buffer.Unlock() }.map_err(|error| error.to_string())?;
    luminance.ok_or_else(|| "A câmera retornou um quadro incompleto.".into())
}

fn planar_luminance(data: &[u8], format: CaptureFormat) -> Option<f32> {
    let width = format.width as usize;
    let height = format.height as usize;
    let row_bytes = width.checked_mul(format.kind.pixel_stride())?;
    let required = (height.checked_sub(1)?)
        .checked_mul(format.row_stride)?
        .checked_add(row_bytes)?;
    if width == 0 || format.row_stride < row_bytes || data.len() < required {
        return None;
    }
    let pixels = width.checked_mul(height)?;
    let step = sample_stride(pixels);
    let mut total = 0u64;
    let mut count = 0usize;
    for pixel in (0..pixels).step_by(step) {
        let row = pixel / width;
        let column = pixel % width;
        total += data[row * format.row_stride + column * format.kind.pixel_stride()] as u64;
        count += 1;
    }
    Some(format.normalize_luma(total as f32 / count as f32))
}

fn select_camera_index(
    identities: &[String],
    index: usize,
    expected: Option<&str>,
) -> Option<usize> {
    match expected {
        Some(expected) => identities.iter().position(|identity| identity == expected),
        None => (index < identities.len()).then_some(index),
    }
}

fn push_stable_reading(readings: &mut Vec<f32>, reading: f32) -> Option<f32> {
    readings.push(reading);
    if readings.len() > 5 {
        readings.remove(0);
    }
    stable_luminance(readings).ok()
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
        if let Err(error) = unsafe { MFShutdown() } {
            tracing::warn!(%error, "Media Foundation shutdown failed");
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
        if !raw_devices.is_null() {
            unsafe { CoTaskMemFree(Some(raw_devices.cast())) };
        }
        return Err("nenhuma câmera foi encontrada pelo Windows".to_owned());
    }
    let devices = unsafe {
        (0..count as usize)
            .filter_map(|index| std::ptr::read(raw_devices.add(index)))
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

#[cfg(test)]
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
        AmbientCommand, CaptureFormat, PixelFormat, apply_command, control_profile,
        planar_luminance,
    };
    use super::{
        MAX_PIXEL_SAMPLES, frame_luminance, helper_error, sample_stride, smooth_factor,
        smoothing_source_changed, stable_luminance, yuy2_luminance,
    };
    use super::{push_stable_reading, select_camera_index};
    use crate::config::Config;

    fn format(width: u32, height: u32, fps: u32, kind: PixelFormat) -> CaptureFormat {
        CaptureFormat {
            width,
            height,
            rate_numerator: fps,
            rate_denominator: 1,
            row_stride: width as usize * kind.pixel_stride(),
            kind,
            decoded_mjpeg: false,
            nominal_range: 0,
        }
    }

    #[test]
    fn chooses_lower_bandwidth_without_using_a_frame_rate_that_cannot_settle() {
        let mut modes = [
            format(640, 480, 30, PixelFormat::Yuy2),
            format(160, 120, 5, PixelFormat::Yuy2),
            format(320, 240, 5, PixelFormat::Nv12),
        ];
        modes.sort_by(|left, right| left.bandwidth().total_cmp(&right.bandwidth()));
        assert_eq!(modes[0].profile(), "YUY2:160x120:5/1:range=0");
        assert!(modes.iter().all(|mode| mode.valid()));
        assert!(!format(160, 120, 1, PixelFormat::Yuy2).valid());
        assert!(
            !CaptureFormat {
                rate_denominator: 0,
                ..modes[0]
            }
            .valid()
        );
    }

    #[test]
    fn nv12_ignores_chroma_and_yuy2_ignores_padding() {
        let nv12 = format(2, 2, 5, PixelFormat::Nv12);
        assert_eq!(
            planar_luminance(&[0, 255, 0, 255, 255, 255], nv12),
            Some(0.5)
        );
        let yuy2 = CaptureFormat {
            row_stride: 6,
            ..format(2, 2, 5, PixelFormat::Yuy2)
        };
        assert_eq!(
            planar_luminance(&[0, 128, 255, 128, 255, 255, 0, 128, 255, 128], yuy2),
            Some(0.5)
        );
        assert_eq!(planar_luminance(&[128; 8], yuy2), None);
    }

    #[test]
    fn limited_range_black_and_white_are_recognized_as_saturated() {
        let limited = CaptureFormat {
            nominal_range: 2,
            ..format(2, 2, 5, PixelFormat::Nv12)
        };
        assert_eq!(planar_luminance(&[16; 4], limited), Some(0.0));
        assert_eq!(planar_luminance(&[235; 4], limited), Some(1.0));
        assert!(stable_luminance(&[planar_luminance(&[16; 4], limited).unwrap(); 5]).is_err());
        assert!(stable_luminance(&[planar_luminance(&[235; 4], limited).unwrap(); 5]).is_err());
    }

    #[test]
    fn fixed_camera_controls_invalidate_comparison_but_auto_values_do_not() {
        assert_ne!(
            control_profile(Some((-6, 2))),
            control_profile(Some((-5, 2)))
        );
        assert_ne!(
            control_profile(Some((-6, 2))),
            control_profile(Some((-6, 1)))
        );
        assert_eq!(
            control_profile(Some((-6, 1))),
            control_profile(Some((-5, 1)))
        );
        assert_eq!(control_profile(None), "unknown");
    }

    #[test]
    fn suspend_and_sensor_preference_changes_invalidate_previous_readings() {
        let mut config = Config::default();
        let mut suspended = false;
        assert!(apply_command(
            AmbientCommand::Suspended(true),
            &mut config,
            &mut suspended
        ));
        assert!(suspended);
        assert!(!apply_command(
            AmbientCommand::Suspended(true),
            &mut config,
            &mut suspended
        ));
        let next = Config {
            ambient_prefer_light_sensor: false,
            ..config.clone()
        };
        assert!(apply_command(
            AmbientCommand::Configure(Box::new(next)),
            &mut config,
            &mut suspended
        ));
        assert!(!config.ambient_prefer_light_sensor);
    }

    #[test]
    fn saved_camera_identity_survives_reordering_without_selecting_another_device() {
        let devices = vec!["camera-b".into(), "camera-a".into()];
        assert_eq!(select_camera_index(&devices, 0, Some("camera-a")), Some(1));
        assert_eq!(select_camera_index(&devices, 0, Some("unplugged")), None);
        assert_eq!(select_camera_index(&devices, 0, None), Some(0));
        assert_eq!(select_camera_index(&devices, 2, None), None);
    }

    #[test]
    fn changing_camera_identity_at_the_same_index_resets_the_previous_reading() {
        let current = Config {
            ambient_camera_id: Some("camera-a".into()),
            ..Config::default()
        };
        let next = Config {
            ambient_camera_id: Some("camera-b".into()),
            ..current.clone()
        };
        assert!(smoothing_source_changed(&current, &next));
    }

    #[test]
    fn exposure_can_settle_after_initial_unstable_or_saturated_frames() {
        let mut readings = Vec::new();
        for sample in [1.0, 0.2, 0.8, 0.3, 0.7] {
            assert!(push_stable_reading(&mut readings, sample).is_none());
        }
        for _ in 0..4 {
            assert!(push_stable_reading(&mut readings, 0.4).is_none());
        }
        assert!((push_stable_reading(&mut readings, 0.4).unwrap() - 0.4).abs() < 0.001);
        assert_eq!(readings.len(), 5);
    }

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
