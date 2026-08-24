//! Circadian schedule: resolve a continuous (CCT, brightness, audio) target for
//! any instant by interpolating between anchored keypoints on the 24 h ring.
//!
//! The engine is pure and platform-agnostic: it works in *minutes since local
//! midnight* (`f64`) plus a [`DayContext`] of resolved solar/sleep anchor times.
//! The host computes those from the system clock + location; this module owns
//! only the math, so it ports cleanly to the Android twin and is fully testable
//! with no OS calls.
//!
//! Two deliberate quality choices (see `docs/VERIFIED-DECISIONS.md`):
//! * color temperature is interpolated in **mired** (reciprocal-Kelvin) space,
//!   so the perceived warm-shift speed is even — the most common circadian bug;
//! * every transition uses **smoothstep** easing, because abrupt change is
//!   itself arousing.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::target::{NoiseColor, Target};

/// Minutes in a day.
pub const DAY_MIN: f64 = 1440.0;

/// When a keypoint occurs, relative to a clock time or a daily event.
///
/// Serializes as a compact, hand-editable string in the TOML config:
/// `"07:00"`, `"sunrise+30"`, `"sunset-15"`, `"wake"`, `"bed-180"`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    /// Fixed local wall-clock time (hour, minute).
    Clock(u8, u8),
    /// Minutes from local sunrise (negative = before).
    SunriseOffset(i32),
    /// Minutes from local sunset.
    SunsetOffset(i32),
    /// Minutes from the user's wake time.
    WakeOffset(i32),
    /// Minutes from the user's bedtime.
    BedOffset(i32),
}

impl Anchor {
    /// Resolve to minutes since local midnight in `[0, 1440)`.
    pub fn resolve(self, ctx: &DayContext) -> f64 {
        let raw = match self {
            Anchor::Clock(h, m) => h as f64 * 60.0 + m as f64,
            Anchor::SunriseOffset(o) => ctx.sunrise_min + o as f64,
            Anchor::SunsetOffset(o) => ctx.sunset_min + o as f64,
            Anchor::WakeOffset(o) => ctx.wake_min + o as f64,
            Anchor::BedOffset(o) => ctx.bed_min + o as f64,
        };
        raw.rem_euclid(DAY_MIN)
    }
}

fn fmt_offset(o: i32) -> String {
    if o == 0 {
        String::new()
    } else if o > 0 {
        format!("+{o}")
    } else {
        o.to_string()
    }
}

impl fmt::Display for Anchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Anchor::Clock(h, m) => write!(f, "{h:02}:{m:02}"),
            Anchor::SunriseOffset(o) => write!(f, "sunrise{}", fmt_offset(*o)),
            Anchor::SunsetOffset(o) => write!(f, "sunset{}", fmt_offset(*o)),
            Anchor::WakeOffset(o) => write!(f, "wake{}", fmt_offset(*o)),
            Anchor::BedOffset(o) => write!(f, "bed{}", fmt_offset(*o)),
        }
    }
}

impl FromStr for Anchor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        // "HH:MM"
        if let Some((h, m)) = s.split_once(':') {
            if let (Ok(h), Ok(m)) = (h.trim().parse::<u8>(), m.trim().parse::<u8>())
                && h < 24 && m < 60
            {
                return Ok(Anchor::Clock(h, m));
            }
            return Err(format!("invalid clock anchor {s:?} (want HH:MM)"));
        }
        let lower = s.to_ascii_lowercase();
        // keyword + optional signed minute offset, e.g. "bed-180", "sunrise+30", "wake"
        let parse_kw = |kw: &str| -> Option<i32> {
            let rest = lower.strip_prefix(kw)?;
            let rest = rest.trim();
            if rest.is_empty() {
                Some(0)
            } else {
                rest.parse::<i32>().ok()
            }
        };
        if let Some(o) = parse_kw("sunrise") {
            return Ok(Anchor::SunriseOffset(o));
        }
        if let Some(o) = parse_kw("sunset") {
            return Ok(Anchor::SunsetOffset(o));
        }
        if let Some(o) = parse_kw("wake") {
            return Ok(Anchor::WakeOffset(o));
        }
        if let Some(o) = parse_kw("bed") {
            return Ok(Anchor::BedOffset(o));
        }
        Err(format!("invalid anchor {s:?}"))
    }
}

impl Serialize for Anchor {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Anchor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Resolved daily anchor times (minutes since local midnight) for "today".
/// The host fills these each tick from the clock + location.
#[derive(Debug, Clone, Copy)]
pub struct DayContext {
    pub sunrise_min: f64,
    pub sunset_min: f64,
    pub wake_min: f64,
    pub bed_min: f64,
}

/// A single anchor in the daily curve: at `anchor`, aim for these values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keypoint {
    pub anchor: Anchor,
    pub cct_kelvin: f32,
    pub brightness: f32,
    /// Gain for Estel's own noise (0..1). `notify_volume` is accepted as a
    /// legacy alias from older config files.
    #[serde(alias = "notify_volume")]
    pub noise_gain: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise: Option<NoiseColor>,
}

impl Keypoint {
    fn target(&self) -> Target {
        Target {
            cct_kelvin: self.cct_kelvin.max(1.0),
            brightness: self.brightness.clamp(0.0, 1.0),
            noise_gain: self.noise_gain.clamp(0.0, 1.0),
            noise: self.noise,
        }
    }
}

/// The full daily curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub keypoints: Vec<Keypoint>,
}

impl Schedule {
    /// Sample the continuous target at `now_min` (minutes since local midnight).
    ///
    /// Resolves every keypoint to an absolute minute, sorts them on the 24 h
    /// ring, finds the bracketing pair around `now`, and interpolates with
    /// smoothstep easing (mired space for CCT, linear for brightness/volume).
    pub fn target_at(&self, now_min: f64, ctx: &DayContext) -> Target {
        if self.keypoints.is_empty() {
            return Target::neutral();
        }

        let mut pts: Vec<(f64, &Keypoint)> = self
            .keypoints
            .iter()
            .map(|k| (k.anchor.resolve(ctx), k))
            .collect();
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);

        let n = pts.len();
        if n == 1 {
            return pts[0].1.target();
        }

        let t = now_min.rem_euclid(DAY_MIN);

        // lo = last keypoint at or before `t`; if `t` precedes them all, wrap to
        // the last keypoint (yesterday's tail).
        let mut lo_idx = n - 1;
        for (i, (min, _)) in pts.iter().enumerate() {
            if *min <= t {
                lo_idx = i;
            } else {
                break;
            }
        }
        let hi_idx = (lo_idx + 1) % n;

        let lo_min = pts[lo_idx].0;
        let hi_min = pts[hi_idx].0;

        let span = (hi_min - lo_min).rem_euclid(DAY_MIN);
        let pos = (t - lo_min).rem_euclid(DAY_MIN);
        let frac = if span <= 1e-6 { 0.0 } else { (pos / span).clamp(0.0, 1.0) };
        let s = smoothstep(frac) as f32;

        let lo = pts[lo_idx].1;
        let hi = pts[hi_idx].1;

        Target {
            cct_kelvin: lerp_cct(lo.cct_kelvin.max(1.0), hi.cct_kelvin.max(1.0), s),
            brightness: lerp(lo.brightness, hi.brightness, s),
            noise_gain: lerp(lo.noise_gain, hi.noise_gain, s),
            // Noise is discrete: hand over at the midpoint of the transition.
            noise: if s < 0.5 { lo.noise } else { hi.noise },
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Interpolate color temperature in **mired** (reciprocal-Kelvin) space so the
/// perceived rate of warm-shift is even.
fn lerp_cct(a: f32, b: f32, t: f32) -> f32 {
    let ma = 1.0e6 / a.max(1.0);
    let mb = 1.0e6 / b.max(1.0);
    1.0e6 / (ma + (mb - ma) * t)
}

/// Smoothstep ease-in-out (`3t² − 2t³`): gentle starts/ends so no transition is
/// ever abrupt.
fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> DayContext {
        DayContext {
            sunrise_min: 6.0 * 60.0,  // 06:00
            sunset_min: 18.0 * 60.0,  // 18:00
            wake_min: 7.0 * 60.0,     // 07:00
            bed_min: 23.0 * 60.0,     // 23:00
        }
    }

    fn sched() -> Schedule {
        Schedule {
            keypoints: vec![
                Keypoint { anchor: Anchor::WakeOffset(0), cct_kelvin: 3400.0, brightness: 0.5, noise_gain: 0.7, noise: None },
                Keypoint { anchor: Anchor::WakeOffset(120), cct_kelvin: 6500.0, brightness: 0.9, noise_gain: 0.9, noise: None },
                Keypoint { anchor: Anchor::BedOffset(-180), cct_kelvin: 3400.0, brightness: 0.55, noise_gain: 0.5, noise: None },
                Keypoint { anchor: Anchor::BedOffset(0), cct_kelvin: 2300.0, brightness: 0.18, noise_gain: 0.15, noise: Some(NoiseColor::Pink) },
                Keypoint { anchor: Anchor::BedOffset(120), cct_kelvin: 1900.0, brightness: 0.0, noise_gain: 0.1, noise: Some(NoiseColor::Brown) },
            ],
        }
    }

    #[test]
    fn anchor_roundtrips_through_string() {
        for a in [
            Anchor::Clock(7, 5),
            Anchor::Clock(0, 0),
            Anchor::SunriseOffset(30),
            Anchor::SunsetOffset(-15),
            Anchor::WakeOffset(0),
            Anchor::BedOffset(-180),
        ] {
            let s = a.to_string();
            let back: Anchor = s.parse().unwrap();
            assert_eq!(a, back, "roundtrip failed for {s:?}");
        }
    }

    #[test]
    fn anchor_parses_friendly_forms() {
        assert_eq!("bed".parse::<Anchor>().unwrap(), Anchor::BedOffset(0));
        assert_eq!("wake+45".parse::<Anchor>().unwrap(), Anchor::WakeOffset(45));
        assert_eq!("23:30".parse::<Anchor>().unwrap(), Anchor::Clock(23, 30));
        assert!("nonsense".parse::<Anchor>().is_err());
        assert!("99:99".parse::<Anchor>().is_err());
    }

    #[test]
    fn hits_keypoint_value_exactly() {
        let s = sched();
        let c = ctx();
        // exactly at bedtime (23:00 = 1380)
        let t = s.target_at(1380.0, &c);
        assert!((t.cct_kelvin - 2300.0).abs() < 1.0, "cct {}", t.cct_kelvin);
        assert!((t.brightness - 0.18).abs() < 1e-3);
        assert_eq!(t.noise, Some(NoiseColor::Pink));
    }

    #[test]
    fn cct_interpolation_is_monotonic_between_keypoints() {
        let s = sched();
        let c = ctx();
        // 07:00 (3400K) -> 09:00 (6500K): sample rising, CCT must increase.
        let mut prev = 0.0_f32;
        for m in (420..=540).step_by(10) {
            let cct = s.target_at(m as f64, &c).cct_kelvin;
            assert!(cct >= prev - 1.0, "cct dipped at {m}: {prev}->{cct}");
            prev = cct;
        }
        assert!(prev > 6000.0, "should reach near 6500K, got {prev}");
    }

    #[test]
    fn continuous_across_midnight_wrap() {
        let s = sched();
        let c = ctx();
        // deep-night keypoint is bed+120 = 01:00 (60). Around it the curve must
        // not jump: small dt -> small change.
        let a = s.target_at(59.0, &c);
        let b = s.target_at(61.0, &c);
        assert!((a.cct_kelvin - b.cct_kelvin).abs() < 50.0, "discontinuity at wrap");
    }

    #[test]
    fn smoothstep_endpoints() {
        assert!((smoothstep(0.0)).abs() < 1e-9);
        assert!((smoothstep(1.0) - 1.0).abs() < 1e-9);
        assert!((smoothstep(0.5) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn empty_schedule_is_neutral_not_a_panic() {
        let s = Schedule { keypoints: vec![] };
        let t = s.target_at(720.0, &ctx());
        assert_eq!(t, Target::neutral());
    }
}
