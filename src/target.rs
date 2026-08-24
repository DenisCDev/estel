//! Ambient state the engine wants at one instant.
//!
//! Pure output: no OS calls. Windows turns `cct_kelvin` + `brightness` into a
//! gamma ramp (and DDC backlight) plus overlay; the audio layer uses `noise`
//! and `noise_gain`. Android applies the same numbers to an overlay.

use serde::{Deserialize, Serialize};

/// Low-level ambient noise. Opt-in, never a default, always volume-capped.
///
/// Pink (1/f, rain-like) and brown (1/f², ocean-like) have modest evidence for
/// sleep onset. They are a wash, not a treatment sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseColor {
    Pink,
    Brown,
}

/// Desired ambient environment at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// White point in Kelvin (warm ~1900 .. neutral ~6500).
    pub cct_kelvin: f32,
    /// Software luminance, 0.0 (dimmest comfortable) .. 1.0 (full).
    pub brightness: f32,
    /// Relative gain for Estel's own noise (0..1). Not the OS volume.
    pub noise_gain: f32,
    /// Suggested ambient noise (`None` = silence).
    pub noise: Option<NoiseColor>,
}

impl Target {
    /// Neutral daylight: 6500 K, full brightness, silence.
    pub fn neutral() -> Self {
        Target {
            cct_kelvin: 6500.0,
            brightness: 1.0,
            noise_gain: 0.0,
            noise: None,
        }
    }

    /// Scale effects toward neutral. `factor` 1.0 = full schedule, 0.0 = off.
    ///
    /// CCT interpolates in mired space (perceptually even). Noise is silenced
    /// below 0.5 — fainter than that is inaudible under game/video audio.
    pub fn attenuate(self, factor: f32) -> Self {
        const NEUTRAL_CCT: f32 = 6500.0;
        let t = factor.clamp(0.0, 1.0);
        let cct = self.cct_kelvin.max(1.0);

        let self_mired = 1_000_000.0 / cct;
        let neutral_mired = 1_000_000.0 / NEUTRAL_CCT;
        let mired = neutral_mired + (self_mired - neutral_mired) * t;

        Target {
            cct_kelvin: 1_000_000.0 / mired.max(1.0),
            brightness: 1.0 + (self.brightness - 1.0) * t,
            noise_gain: self.noise_gain * t,
            noise: if t >= 0.2 { self.noise } else { None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_factor_keeps_values() {
        let t = Target {
            cct_kelvin: 2300.0,
            brightness: 0.2,
            noise_gain: 0.4,
            noise: Some(NoiseColor::Pink),
        };
        let a = t.attenuate(1.0);
        assert!((a.cct_kelvin - 2300.0).abs() < 1.0);
        assert!((a.brightness - 0.2).abs() < 1e-5);
        assert_eq!(a.noise, Some(NoiseColor::Pink));
    }

    #[test]
    fn zero_factor_is_neutral_and_silent() {
        let t = Target {
            cct_kelvin: 1900.0,
            brightness: 0.0,
            noise_gain: 1.0,
            noise: Some(NoiseColor::Brown),
        };
        let a = t.attenuate(0.0);
        assert!((a.cct_kelvin - 6500.0).abs() < 1.0);
        assert!((a.brightness - 1.0).abs() < 1e-5);
        assert_eq!(a.noise, None);
        assert!(a.noise_gain.abs() < 1e-5);
    }

    #[test]
    fn suave_silences_noise() {
        let t = Target {
            cct_kelvin: 2300.0,
            brightness: 0.2,
            noise_gain: 0.5,
            noise: Some(NoiseColor::Pink),
        };
        assert_eq!(t.attenuate(0.1).noise, None);
        assert_eq!(t.attenuate(0.3).noise, Some(NoiseColor::Pink));
    }

    #[test]
    fn zero_cct_does_not_produce_nan() {
        let t = Target {
            cct_kelvin: 0.0,
            brightness: 0.5,
            noise_gain: 0.0,
            noise: None,
        };
        let a = t.attenuate(1.0);
        assert!(a.cct_kelvin.is_finite());
        assert!(a.cct_kelvin > 0.0);
    }
}
