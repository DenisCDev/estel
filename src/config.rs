//! User configuration: persisted as a hand-editable TOML file under
//! `%APPDATA%\Roaming\condado\estel\config\config.toml`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::schedule::{Anchor, Keypoint, Schedule};
use crate::target::NoiseColor;

/// How strongly the scheduled display adjustments are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Intensity {
    /// Full schedule.
    Alta,
    /// Moderate default, adjustable for personal comfort.
    #[default]
    Media,
    /// Lightest scheduled adjustment.
    Suave,
}

impl Intensity {
    /// Multiplier applied to color and sound, independently of brightness.
    pub fn factor(self) -> f32 {
        match self {
            Intensity::Alta => 1.0,
            Intensity::Media => 0.6,
            Intensity::Suave => 0.3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Intensity::Alta => "Alta",
            Intensity::Media => "Média",
            Intensity::Suave => "Suave",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScreenWindowRelation {
    Front,
    Back,
    #[default]
    Side,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SupportCountry {
    #[default]
    Brazil,
    Portugal,
    UnitedStates,
    UnitedKingdom,
    Other,
}

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Existing preferences remain active; newly created files open the setup guide.
    #[serde(default = "completed_setup")]
    pub setup_completed: bool,
    /// Latitude/longitude for sunrise/sunset. Default: São Paulo.
    pub latitude: f64,
    pub longitude: f64,
    /// Ask Windows for coordinates when the settings window first opens.
    /// Existing configurations without this field keep their manual coordinates.
    #[serde(default)]
    pub location_auto: bool,
    /// Weather-assisted fallback when the camera cannot provide a reading.
    #[serde(default)]
    pub weather_enabled: bool,
    /// Direction of the nearest window, clockwise from north.
    pub window_azimuth_deg: Option<f32>,
    /// Whether daylight reaches the desk through that window.
    pub window_near: bool,
    /// How the display faces the window: front, back, or side.
    pub screen_window_relation: ScreenWindowRelation,
    /// Typical wake and bed times, local `"HH:MM"`.
    pub wake: String,
    pub bed: String,

    /// Lowest comfortable software luminance (0..1) so the screen is never
    /// fully black even at deep night.
    pub min_brightness: f32,
    /// Personal brightness ceilings, independent of color intensity.
    pub day_brightness_max: f32,
    pub rest_brightness_max: f32,
    /// Gentle warm floor (Kelvin) that the gamma path is allowed to reach before
    /// the overlay takes over; staying ≳3400 K avoids the Win11 gamma clamp.
    pub gamma_warm_floor_k: f32,
    /// Recompute + reapply the target every this many seconds.
    pub tick_seconds: u64,

    /// User cap for Estel's own noise (0..1). Still bounded by a hard ceiling
    /// in the audio layer — this cannot make the app loud.
    pub max_volume: f32,
    /// Master switches.
    pub display_enabled: bool,
    pub noise_enabled: bool,
    /// Preserve the monitor's calibrated output during color-critical work.
    pub color_critical_work: bool,
    /// Avoid global color transforms when color discrimination may differ.
    pub color_vision_deficiency: bool,

    /// Color and sound intensity. Switchable at runtime via tray.
    /// "alta" = full, "media" = 60 % (default), "suave" = 30 %.
    pub intensity: Intensity,
    /// None migrates the existing Windows startup preference on first launch.
    pub start_with_windows: Option<bool>,

    /// Enable local ambient brightness adaptation.
    pub ambient_enabled: bool,
    /// Prefer a dedicated illuminance sensor before opening the camera.
    pub ambient_prefer_light_sensor: bool,
    /// Index in the webcam list used to capture ambient light.
    pub ambient_camera_index: usize,
    pub ambient_camera_id: Option<String>,
    /// Interval between samples in seconds.
    pub ambient_sample_interval_seconds: u64,
    /// Lowest screen brightness when ambient light is low.
    pub ambient_brightness_min: f32,
    /// Highest screen brightness when ambient light is high.
    pub ambient_brightness_max: f32,
    /// Relative image anchors; these do not represent lux or spectral exposure.
    pub ambient_calibration: Option<AmbientCalibration>,

    /// Country selected for local emotional support information.
    pub support_country: SupportCountry,

    /// The daily curve.
    pub schedule: Schedule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AmbientCalibration {
    pub device_id: String,
    pub camera_index: usize,
    /// Capture format and observed camera controls must match both references.
    #[serde(default)]
    pub capture_profile: Option<String>,
    pub dark: f32,
    pub bright: f32,
}

impl AmbientCalibration {
    pub fn matches_capture(&self, profile: &str) -> bool {
        !profile.is_empty()
            && profile.len() <= 4096
            && self.capture_profile.as_deref() == Some(profile)
    }

    pub fn is_valid(&self) -> bool {
        !self.device_id.is_empty()
            && self.device_id.len() <= 4096
            && self.dark.is_finite()
            && self.bright.is_finite()
            && (0.02..=0.98).contains(&self.dark)
            && (0.02..=0.98).contains(&self.bright)
            // A small contrast can be image noise or automatic exposure compensation.
            && self.bright - self.dark >= 0.10
    }

    pub fn brightness(&self, device_id: &str, signal: f32, low: f32, high: f32) -> Option<f32> {
        if !self.is_valid()
            || self.device_id != device_id
            || !signal.is_finite()
            || !(self.dark..=self.bright).contains(&signal)
        {
            return None;
        }
        let fraction = (signal - self.dark) / (self.bright - self.dark);
        Some(low + (high - low) * fraction)
    }
}

fn finite_clamp(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() { value } else { fallback }.clamp(min, max)
}

impl Default for Config {
    fn default() -> Self {
        Config {
            setup_completed: true,
            latitude: -23.5505,
            longitude: -46.6333,
            location_auto: true,
            weather_enabled: false,
            window_azimuth_deg: None,
            window_near: false,
            screen_window_relation: ScreenWindowRelation::Side,
            wake: "07:00".to_string(),
            bed: "23:00".to_string(),
            min_brightness: 0.25,
            day_brightness_max: 0.85,
            rest_brightness_max: 0.25,
            gamma_warm_floor_k: 3400.0,
            tick_seconds: 30,
            max_volume: 0.35,
            display_enabled: true,
            noise_enabled: false,
            color_critical_work: false,
            color_vision_deficiency: false,
            intensity: Intensity::Media,
            start_with_windows: None,
            ambient_enabled: false,
            ambient_prefer_light_sensor: true,
            ambient_camera_index: 0,
            ambient_camera_id: None,
            ambient_sample_interval_seconds: 30,
            ambient_brightness_min: 0.25,
            ambient_brightness_max: 0.85,
            ambient_calibration: None,
            support_country: SupportCountry::Brazil,
            schedule: default_schedule(),
        }
    }
}

/// The default circadian curve (see the spec table).
fn default_schedule() -> Schedule {
    Schedule {
        keypoints: vec![
            Keypoint {
                anchor: Anchor::BedOffset(120),
                cct_kelvin: 1900.0,
                brightness: 0.0,
                noise_gain: 0.55,
                noise: Some(NoiseColor::Brown),
            },
            Keypoint {
                anchor: Anchor::WakeOffset(0),
                cct_kelvin: 3400.0,
                brightness: 0.50,
                noise_gain: 0.0,
                noise: None,
            },
            Keypoint {
                anchor: Anchor::WakeOffset(90),
                cct_kelvin: 6500.0,
                brightness: 0.90,
                noise_gain: 0.0,
                noise: None,
            },
            Keypoint {
                anchor: Anchor::SunsetOffset(-15),
                cct_kelvin: 6500.0,
                brightness: 0.85,
                noise_gain: 0.0,
                noise: None,
            },
            Keypoint {
                anchor: Anchor::SunsetOffset(45),
                cct_kelvin: 3800.0,
                brightness: 0.55,
                noise_gain: 0.45,
                noise: Some(NoiseColor::Pink),
            },
            Keypoint {
                anchor: Anchor::SunsetOffset(120),
                cct_kelvin: 2800.0,
                brightness: 0.38,
                noise_gain: 0.70,
                noise: Some(NoiseColor::Pink),
            },
            Keypoint {
                anchor: Anchor::BedOffset(-30),
                cct_kelvin: 2300.0,
                brightness: 0.22,
                noise_gain: 0.80,
                noise: Some(NoiseColor::Brown),
            },
            Keypoint {
                anchor: Anchor::BedOffset(0),
                cct_kelvin: 2100.0,
                brightness: 0.16,
                noise_gain: 0.55,
                noise: Some(NoiseColor::Brown),
            },
        ],
    }
}

impl Config {
    pub fn camera_is_calibrated(&self) -> bool {
        self.ambient_calibration.as_ref().is_some_and(|value| {
            self.ambient_camera_id
                .as_ref()
                .map_or(value.camera_index == self.ambient_camera_index, |id| {
                    id == &value.device_id
                })
                && value.is_valid()
                && value
                    .capture_profile
                    .as_ref()
                    .is_some_and(|profile| !profile.is_empty() && profile.len() <= 4096)
        })
    }

    pub fn preserve_colors(&self) -> bool {
        self.color_critical_work || self.color_vision_deficiency
    }
    /// `%APPDATA%\Roaming\condado\estel\config\config.toml` (or a CWD fallback).
    pub fn config_path() -> PathBuf {
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            return root.join("condado/estel/config/config.toml");
        }
        if let Some(dirs) = directories::ProjectDirs::from("studio", "condado", "estel") {
            dirs.config_dir().join("config.toml")
        } else {
            PathBuf::from("estel-config.toml")
        }
    }

    /// Load the config from the default path, writing defaults on first run.
    ///
    /// A broken file is recovered from a valid backup, preserving its original
    /// bytes. Without a valid backup, fail instead of replacing preferences.
    pub fn load_or_default() -> io::Result<Self> {
        Self::load_from(&Self::config_path())
    }

    pub fn load_from(path: &Path) -> io::Result<Self> {
        let _access = ConfigAccess::acquire()?;
        Self::load_unlocked(path)
    }

    fn load_unlocked(path: &Path) -> io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => match parse_config(&text) {
                Ok(mut cfg) => {
                    cfg.sanitize();
                    if cfg.migrate_schedule_to_sunset() {
                        if let Err(e) = cfg.save_unlocked(path) {
                            tracing::warn!("não foi possível gravar a curva nova: {e}");
                        } else {
                            tracing::info!("curva atualizada: a noite agora segue o pôr do sol");
                        }
                    }
                    Ok(cfg)
                }
                Err(e) => {
                    tracing::error!(%e, "configuração inválida; tentando recuperar o backup");
                    Self::recover_backup(path, Some(text))
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if path.with_extension("toml.bak").exists() {
                    return Self::recover_backup(path, None);
                }
                let mut cfg = Config {
                    setup_completed: false,
                    location_auto: false,
                    ..Config::default()
                };
                cfg.sanitize();
                cfg.save_unlocked(path)?;
                Ok(cfg)
            }
            Err(error) => Err(error),
        }
    }

    fn recover_backup(path: &Path, invalid: Option<String>) -> io::Result<Self> {
        let text = std::fs::read_to_string(path.with_extension("toml.bak"))?;
        let mut cfg = parse_config(&text)?;
        cfg.sanitize();
        if let Some(invalid) = invalid {
            atomic_write(&path.with_extension("toml.invalid"), invalid.as_bytes())?;
        }
        atomic_write(path, text.as_bytes())?;
        tracing::warn!("preferências recuperadas do backup; arquivo danificado preservado");
        Ok(cfg)
    }

    /// Clamp every user-facing field. Call after deserialize and before save.
    pub fn sanitize(&mut self) {
        self.latitude = if self.latitude.is_finite() {
            self.latitude.clamp(-90.0, 90.0)
        } else {
            Config::default().latitude
        };
        self.longitude = if self.longitude.is_finite() {
            self.longitude.clamp(-180.0, 180.0)
        } else {
            Config::default().longitude
        };
        self.window_azimuth_deg = self
            .window_azimuth_deg
            .filter(|value| value.is_finite())
            .map(|value| value.rem_euclid(360.0));
        self.min_brightness = finite_clamp(self.min_brightness, 0.25, 0.15, 0.80);
        self.day_brightness_max =
            finite_clamp(self.day_brightness_max, 0.85, self.min_brightness, 1.0);
        self.rest_brightness_max = finite_clamp(
            self.rest_brightness_max,
            0.25,
            self.min_brightness,
            self.day_brightness_max,
        );
        self.gamma_warm_floor_k = self.gamma_warm_floor_k.clamp(3000.0, 4500.0);
        self.tick_seconds = self.tick_seconds.clamp(5, 120);
        self.max_volume = self.max_volume.clamp(0.0, 0.70);
        self.ambient_sample_interval_seconds = self.ambient_sample_interval_seconds.clamp(10, 120);
        self.ambient_camera_id = self
            .ambient_camera_id
            .take()
            .filter(|id| !id.is_empty() && id.len() <= 4096);
        self.ambient_brightness_min = finite_clamp(self.ambient_brightness_min, 0.25, 0.15, 1.0);
        self.ambient_brightness_max = finite_clamp(self.ambient_brightness_max, 0.85, 0.15, 1.0);
        if self
            .ambient_calibration
            .as_ref()
            .is_some_and(|value| !value.is_valid())
        {
            self.ambient_calibration = None;
        }
        if self.ambient_brightness_max < self.ambient_brightness_min {
            std::mem::swap(
                &mut self.ambient_brightness_min,
                &mut self.ambient_brightness_max,
            );
        }
        if self.schedule.keypoints.is_empty() {
            self.schedule = default_schedule();
        }
        for k in &mut self.schedule.keypoints {
            k.cct_kelvin = k.cct_kelvin.clamp(1000.0, 10000.0);
            k.brightness = k.brightness.clamp(0.0, 1.0);
            k.noise_gain = k.noise_gain.clamp(0.0, 1.0);
        }
    }

    /// Old configs only anchored on wake/bed, so evening waited until 5 h
    /// before bedtime (20:00 if you sleep at 01:00). Sunset keypoints make
    /// the warm + noise start when the sun actually goes down.
    fn migrate_schedule_to_sunset(&mut self) -> bool {
        let has_solar = self.schedule.keypoints.iter().any(|k| {
            matches!(
                k.anchor,
                crate::schedule::Anchor::SunsetOffset(_)
                    | crate::schedule::Anchor::SunriseOffset(_)
            )
        });
        if has_solar {
            return false;
        }
        self.schedule = default_schedule();
        true
    }

    /// Persist to `path`, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let _access = ConfigAccess::acquire()?;
        self.save_unlocked(path)
    }

    fn save_unlocked(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        match std::fs::read_to_string(path) {
            Ok(previous) => {
                parse_config(&previous)?;
                atomic_write(&path.with_extension("toml.bak"), previous.as_bytes())?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        atomic_write(path, text.as_bytes())
    }

    /// Only edited fields replace the latest preferences saved by the tray or panel.
    pub fn save_changes(&self, baseline: &Self, path: &Path) -> io::Result<Self> {
        let _access = ConfigAccess::acquire()?;
        let mut current =
            toml::Value::try_from(Self::load_unlocked(path)?).map_err(io::Error::other)?;
        let edited = toml::Value::try_from(self).map_err(io::Error::other)?;
        let baseline = toml::Value::try_from(baseline).map_err(io::Error::other)?;
        let edited = edited
            .as_table()
            .ok_or_else(|| io::Error::other("configuração inválida"))?;
        let baseline = baseline
            .as_table()
            .ok_or_else(|| io::Error::other("configuração inválida"))?;
        let current = current
            .as_table_mut()
            .ok_or_else(|| io::Error::other("configuração inválida"))?;
        for (key, value) in edited {
            if baseline.get(key) != Some(value) {
                current.insert(key.clone(), value.clone());
            }
        }
        for key in baseline.keys() {
            if !edited.contains_key(key) {
                current.remove(key);
            }
        }
        let mut merged: Self = toml::Value::Table(current.clone())
            .try_into()
            .map_err(io::Error::other)?;
        merged.sanitize();
        merged.save_unlocked(path)?;
        Ok(merged)
    }

    /// Wake time as minutes since local midnight.
    pub fn wake_min(&self) -> f64 {
        parse_hhmm(&self.wake)
    }

    /// Bed time as minutes since local midnight.
    pub fn bed_min(&self) -> f64 {
        parse_hhmm(&self.bed)
    }
}

fn completed_setup() -> bool {
    true
}

fn parse_config(text: &str) -> io::Result<Config> {
    let value: toml::Value = toml::from_str(text).map_err(io::Error::other)?;
    if value.as_table().is_none_or(|table| table.is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "arquivo de configuração vazio",
        ));
    }
    value.try_into().map_err(io::Error::other)
}

struct ConfigAccess {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

impl ConfigAccess {
    fn acquire() -> io::Result<Self> {
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0};
            use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
            let handle = unsafe {
                CreateMutexW(None, false, windows::core::w!("Local\\EstelConfiguration"))
            }
            .map_err(io::Error::other)?;
            let wait = unsafe { WaitForSingleObject(handle, 2000) };
            if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                if let Err(error) = unsafe { CloseHandle(handle) } {
                    tracing::warn!(%error, "não foi possível liberar o acesso à configuração");
                }
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "outra janela está salvando a configuração; tente novamente",
                ));
            }
            Ok(Self { handle })
        }
        #[cfg(not(windows))]
        Ok(Self {})
    }
}

impl Drop for ConfigAccess {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::Threading::ReleaseMutex;
            if let Err(error) = unsafe { ReleaseMutex(self.handle) } {
                tracing::warn!(%error, "não foi possível liberar o bloqueio da configuração");
            }
            if let Err(error) = unsafe { CloseHandle(self.handle) } {
                tracing::warn!(%error, "não foi possível fechar o acesso à configuração");
            }
        }
    }
}

pub(crate) fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
            let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                MoveFileExW(
                    windows::core::PCWSTR(source.as_ptr()),
                    windows::core::PCWSTR(destination.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
            .map_err(io::Error::other)
        }
        #[cfg(not(windows))]
        std::fs::rename(&temporary, path)
    })();
    if result.is_err()
        && let Err(error) = std::fs::remove_file(&temporary)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(%error, "não foi possível remover o arquivo temporário da configuração");
    }
    result
}

fn parse_hhmm(s: &str) -> f64 {
    let mut it = s.split(':');
    let h: f64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0.0);
    let m: f64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0.0);
    (h * 60.0 + m).rem_euclid(crate::schedule::DAY_MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConfigDirectory(PathBuf);

    impl ConfigDirectory {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "estel-config-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn config(&self) -> PathBuf {
            self.0.join("config.toml")
        }
    }

    impl Drop for ConfigDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove test configuration");
        }
    }

    #[test]
    fn saving_keeps_the_previous_preferences_as_a_valid_backup() {
        let directory = ConfigDirectory::new();
        let path = directory.config();
        let mut cfg = Config {
            wake: "08:00".into(),
            ambient_enabled: true,
            ..Config::default()
        };
        cfg.save(&path).unwrap();
        cfg.wake = "09:00".into();
        cfg.save(&path).unwrap();
        let backup: Config =
            toml::from_str(&std::fs::read_to_string(path.with_extension("toml.bak")).unwrap())
                .unwrap();
        assert_eq!(backup.wake, "08:00");
        assert!(backup.ambient_enabled);
        let saved: Config = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.wake, "09:00");
    }

    #[test]
    fn saving_does_not_replace_an_unreadable_or_corrupt_config() {
        let directory = ConfigDirectory::new();
        let path = directory.config();
        let corrupt = "wake = \"08:";
        std::fs::write(&path, corrupt).unwrap();
        assert!(Config::default().save(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), corrupt);
    }

    #[test]
    fn corrupt_or_missing_primary_recovers_preferences_from_backup() {
        for missing in [false, true] {
            let directory = ConfigDirectory::new();
            let path = directory.config();
            let cfg = Config {
                wake: "08:00".into(),
                ambient_enabled: true,
                intensity: Intensity::Alta,
                ..Config::default()
            };
            cfg.save(&path).unwrap();
            cfg.save(&path).unwrap();
            if missing {
                std::fs::remove_file(&path).unwrap();
            } else {
                std::fs::write(&path, "wake = \"08:").unwrap();
            }
            let recovered = Config::load_from(&path).unwrap();
            assert_eq!(recovered.wake, "08:00");
            assert!(recovered.ambient_enabled);
            assert_eq!(recovered.intensity, Intensity::Alta);
            assert_eq!(Config::load_from(&path).unwrap().wake, "08:00");
            if !missing {
                assert_eq!(
                    std::fs::read_to_string(path.with_extension("toml.invalid")).unwrap(),
                    "wake = \"08:"
                );
            }
        }
    }

    #[test]
    fn loading_corrupt_without_backup_or_directory_never_creates_defaults() {
        let directory = ConfigDirectory::new();
        let path = directory.config();
        std::fs::write(&path, "wake = \"08:").unwrap();
        assert!(Config::load_from(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "wake = \"08:");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(Config::load_from(&path).is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn empty_primary_recovers_the_backup_instead_of_resetting_preferences() {
        for empty in ["", "   \n", "# interrupted save\n"] {
            let directory = ConfigDirectory::new();
            let path = directory.config();
            let config = Config {
                wake: "08:00".into(),
                ..Config::default()
            };
            config.save(&path).unwrap();
            config.save(&path).unwrap();
            std::fs::write(&path, empty).unwrap();
            assert_eq!(Config::load_from(&path).unwrap().wake, "08:00");
            assert_eq!(
                std::fs::read_to_string(path.with_extension("toml.invalid")).unwrap(),
                empty
            );
        }
    }

    #[test]
    fn an_open_panel_does_not_overwrite_newer_tray_preferences() {
        let directory = ConfigDirectory::new();
        let path = directory.config();
        let initial = Config::default();
        initial.save(&path).unwrap();
        let mut tray = initial.clone();
        tray.intensity = Intensity::Alta;
        tray.start_with_windows = Some(false);
        tray.save_changes(&initial, &path).unwrap();
        let mut panel = initial.clone();
        panel.wake = "08:00".into();
        let merged = panel.save_changes(&initial, &path).unwrap();
        assert_eq!(merged.wake, "08:00");
        assert_eq!(merged.intensity, Intensity::Alta);
        assert_eq!(merged.start_with_windows, Some(false));
    }

    #[cfg(windows)]
    #[test]
    fn simultaneous_panel_and_tray_saves_preserve_both_edits() {
        let directory = ConfigDirectory::new();
        let path = directory.config();
        let baseline = Config::default();
        baseline.save(&path).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            for field in 0..2 {
                let baseline = &baseline;
                let path = &path;
                let barrier = barrier.clone();
                scope.spawn(move || {
                    let mut config = baseline.clone();
                    if field == 0 {
                        config.wake = "08:00".into();
                    } else {
                        config.intensity = Intensity::Alta;
                    }
                    barrier.wait();
                    config.save_changes(baseline, path).unwrap();
                });
            }
        });
        let saved = Config::load_from(&path).unwrap();
        assert_eq!(saved.wake, "08:00");
        assert_eq!(saved.intensity, Intensity::Alta);
    }

    #[test]
    fn calibration_rejects_wrong_device_missing_contrast_and_extrapolation() {
        let calibration = AmbientCalibration {
            device_id: "camera-a".into(),
            camera_index: 0,
            capture_profile: Some("YUY2:320x240:5/1:exposure=-6".into()),
            dark: 0.2,
            bright: 0.6,
        };
        assert!(
            (calibration.brightness("camera-a", 0.4, 0.25, 0.85).unwrap() - 0.55).abs() < 0.001
        );
        for (device, signal) in [
            ("camera-b", 0.4),
            ("camera-a", 0.1),
            ("camera-a", 0.7),
            ("camera-a", f32::NAN),
        ] {
            assert!(calibration.brightness(device, signal, 0.25, 0.85).is_none());
        }
        for (dark, bright) in [(0.4, 0.41), (0.6, 0.2), (f32::NAN, 0.8), (0.2, 1.0)] {
            assert!(
                !AmbientCalibration {
                    dark,
                    bright,
                    ..calibration.clone()
                }
                .is_valid()
            );
        }
    }

    #[test]
    fn camera_references_require_the_same_capture_profile_and_legacy_anchors_are_preserved() {
        let calibration = AmbientCalibration {
            device_id: "camera-a".into(),
            camera_index: 0,
            capture_profile: Some("YUY2:160x120:5/1".into()),
            dark: 0.2,
            bright: 0.6,
        };
        assert!(calibration.matches_capture("YUY2:160x120:5/1"));
        assert!(!calibration.matches_capture("YUY2:640x480:30/1"));
        let mut config = Config {
            ambient_calibration: Some(calibration),
            ..Config::default()
        };
        assert!(config.camera_is_calibrated());
        config.ambient_calibration.as_mut().unwrap().capture_profile = None;
        config.sanitize();
        assert!(!config.camera_is_calibrated());
        assert!(config.ambient_calibration.is_some());
    }

    #[test]
    fn personal_brightness_bounds_remain_ordered_with_legacy_and_invalid_values() {
        let mut cfg: Config = toml::from_str("min_brightness = 0.6").unwrap();
        cfg.sanitize();
        assert_eq!(cfg.rest_brightness_max, 0.6);
        cfg.min_brightness = f32::NAN;
        cfg.day_brightness_max = f32::NEG_INFINITY;
        cfg.rest_brightness_max = f32::NAN;
        cfg.sanitize();
        assert_eq!(cfg.min_brightness, 0.25);
        assert_eq!(cfg.day_brightness_max, 0.85);
        assert_eq!(cfg.rest_brightness_max, 0.25);
    }

    #[test]
    fn default_config_roundtrips_through_toml() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).expect("serialize");
        let back: Config = toml::from_str(&text).expect("deserialize");
        assert_eq!(cfg.schedule.keypoints.len(), back.schedule.keypoints.len());
        assert_eq!(back.wake, "07:00");
        assert_eq!(back.intensity, Intensity::Media);
        assert_eq!(back.min_brightness, 0.25);
        assert!(!back.ambient_enabled);
        assert_eq!(back.support_country, SupportCountry::Brazil);
    }

    #[test]
    fn hhmm_parsing() {
        assert_eq!(parse_hhmm("07:00"), 420.0);
        assert_eq!(parse_hhmm("23:30"), 1410.0);
        assert_eq!(parse_hhmm("00:00"), 0.0);
    }

    #[test]
    fn intensity_factors() {
        assert_eq!(Intensity::Alta.factor(), 1.0);
        assert_eq!(Intensity::Media.factor(), 0.6);
        assert_eq!(Intensity::Suave.factor(), 0.3);
    }

    #[test]
    fn sanitize_clamps_volume_and_tick() {
        let mut cfg = Config {
            max_volume: 10.0,
            tick_seconds: 0,
            latitude: 200.0,
            ..Config::default()
        };
        cfg.schedule.keypoints.clear();
        cfg.sanitize();
        assert!(cfg.max_volume <= 0.70);
        assert!(cfg.tick_seconds >= 5);
        assert!(cfg.latitude <= 90.0);
        assert!(!cfg.schedule.keypoints.is_empty());
    }

    #[test]
    fn legacy_notify_volume_alias_still_loads() {
        let text = r#"
latitude = -23.55
longitude = -46.63
wake = "07:00"
bed = "23:00"
min_brightness = 0.3
gamma_warm_floor_k = 3400.0
tick_seconds = 30
max_volume = 0.35
display_enabled = true
noise_enabled = false
intensity = "alta"

[[schedule.keypoints]]
anchor = "bed"
cct_kelvin = 2300.0
brightness = 0.18
notify_volume = 0.15
noise = "pink"
"#;
        let cfg: Config = toml::from_str(text).expect("legacy alias");
        assert!((cfg.schedule.keypoints[0].noise_gain - 0.15).abs() < 1e-6);
    }

    #[test]
    fn ambient_values_are_sanitized() {
        let mut cfg = Config {
            ambient_enabled: true,
            ambient_sample_interval_seconds: 0,
            ambient_brightness_min: 2.0,
            ambient_brightness_max: 0.0,
            ambient_camera_index: 10,
            ..Config::default()
        };
        cfg.sanitize();
        assert!(cfg.ambient_sample_interval_seconds >= 10);
        assert!(cfg.ambient_brightness_min <= cfg.ambient_brightness_max);
        assert!((cfg.ambient_brightness_min - 0.15).abs() < f32::EPSILON);
        assert!((cfg.ambient_brightness_max - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn older_config_keeps_manual_location() {
        let text = toml::to_string(&Config::default()).unwrap();
        let legacy = text
            .lines()
            .filter(|line| {
                !line.starts_with("location_auto =") && !line.starts_with("support_country =")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let config: Config = toml::from_str(&legacy).unwrap();
        assert!(!config.location_auto);
        assert!(Config::default().location_auto);
        assert!(!config.weather_enabled);
        assert_eq!(config.support_country, SupportCountry::Brazil);
    }

    #[test]
    fn fresh_install_waits_for_setup_without_requesting_location_or_camera() {
        let directory = ConfigDirectory::new();
        let config = Config::load_from(&directory.config()).unwrap();
        assert!(!config.setup_completed);
        assert!(!config.location_auto);
        assert!(!config.ambient_enabled);
        assert!(
            !Config::load_from(&directory.config())
                .unwrap()
                .setup_completed
        );
    }

    #[test]
    fn upgrade_preserves_existing_preferences_without_repeating_setup() {
        let text = toml::to_string(&Config::default()).unwrap();
        let legacy = text
            .lines()
            .filter(|line| !line.starts_with("setup_completed ="))
            .collect::<Vec<_>>()
            .join("\n");
        let config: Config = toml::from_str(&legacy).unwrap();
        assert!(config.setup_completed);
        assert!(config.location_auto);
    }

    #[test]
    fn invalid_coordinates_restore_a_valid_location() {
        let mut config = Config {
            latitude: f64::NAN,
            longitude: f64::INFINITY,
            window_azimuth_deg: Some(f32::NAN),
            ..Config::default()
        };
        config.sanitize();
        assert_eq!(config.latitude, Config::default().latitude);
        assert_eq!(config.longitude, Config::default().longitude);
        assert_eq!(config.window_azimuth_deg, None);
    }
}
