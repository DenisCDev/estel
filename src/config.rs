//! User configuration: persisted as a hand-editable TOML file under
//! `%APPDATA%\Roaming\condado\estel\config\config.toml`.

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

    /// Enable webcam-driven ambient brightness adaptation.
    pub ambient_enabled: bool,
    /// Index in the webcam list used to capture ambient light.
    pub ambient_camera_index: usize,
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
    pub dark: f32,
    pub bright: f32,
}

impl AmbientCalibration {
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
            ambient_enabled: false,
            ambient_camera_index: 0,
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
            value.camera_index == self.ambient_camera_index && value.is_valid()
        })
    }

    pub fn preserve_colors(&self) -> bool {
        self.color_critical_work || self.color_vision_deficiency
    }
    /// `%APPDATA%\Roaming\condado\estel\config\config.toml` (or a CWD fallback).
    pub fn config_path() -> PathBuf {
        if let Some(dirs) = directories::ProjectDirs::from("studio", "condado", "estel") {
            dirs.config_dir().join("config.toml")
        } else {
            PathBuf::from("estel-config.toml")
        }
    }

    /// Load the config from the default path, writing defaults on first run.
    ///
    /// A broken file is left on disk (renamed to `config.toml.invalid`) and
    /// in-memory defaults are used — we never silently overwrite the user's
    /// file with a parsed-ok-but-stale clone.
    pub fn load_or_default() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(mut cfg) => {
                    cfg.sanitize();
                    if cfg.migrate_schedule_to_sunset() {
                        if let Err(e) = cfg.save(&path) {
                            tracing::warn!("não foi possível gravar a curva nova: {e}");
                        } else {
                            tracing::info!("curva atualizada: a noite agora segue o pôr do sol");
                        }
                    }
                    cfg
                }
                Err(e) => {
                    tracing::error!(
                        "config.toml inválido ({e}); usando padrões. O arquivo original foi copiado para config.toml.invalid"
                    );
                    let bak = path.with_extension("toml.invalid");
                    let _ = std::fs::copy(&path, &bak);
                    let mut cfg = Config::default();
                    cfg.sanitize();
                    cfg
                }
            },
            Err(_) => {
                let mut cfg = Config::default();
                cfg.sanitize();
                if let Err(e) = cfg.save(&path) {
                    tracing::warn!(
                        "não foi possível gravar o config padrão em {}: {e}",
                        path.display()
                    );
                }
                cfg
            }
        }
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
        self.ambient_sample_interval_seconds = self.ambient_sample_interval_seconds.clamp(2, 120);
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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
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

fn parse_hhmm(s: &str) -> f64 {
    let mut it = s.split(':');
    let h: f64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0.0);
    let m: f64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0.0);
    (h * 60.0 + m).rem_euclid(crate::schedule::DAY_MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_rejects_wrong_device_missing_contrast_and_extrapolation() {
        let calibration = AmbientCalibration {
            device_id: "camera-a".into(),
            camera_index: 0,
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
        assert!(cfg.ambient_sample_interval_seconds >= 2);
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
