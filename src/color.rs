//! Pure color math: correlated color temperature (CCT) -> sRGB channel scale,
//! and gamma-ramp construction.
//!
//! Warming the display is done by multiplying an identity gamma ramp by
//! per-channel scale factors derived from the target color temperature, plus an
//! overall brightness scale. The resulting 3x256 `u16` ramp is exactly what
//! Windows' `SetDeviceGammaRamp` consumes. This module performs no OS calls, so
//! it is unit-testable and portable.

/// A full display gamma ramp: 256 entries per channel (R, G, B), 0..=65535.
pub type GammaRamp = [[u16; 256]; 3];

/// Convert a correlated color temperature (Kelvin) to sRGB channel multipliers
/// in 0.0..=1.0.
///
/// Uses the Tanner Helland approximation of the Planckian locus: cheap,
/// monotonic, and visually smooth across the warm-shift range we care about
/// (~1900..6500 K). At ~6500 K the result is near-neutral (1, ~1, ~1); as the
/// temperature drops, blue then green fall away, yielding a warm amber. Exact
/// colorimetry is unnecessary for a calming tint, but the curve is smooth so the
/// automatic ramp is never jarring.
pub fn cct_to_rgb(kelvin: f32) -> [f32; 3] {
    let t = kelvin.clamp(1000.0, 40000.0) / 100.0;

    let r = if t <= 66.0 {
        255.0
    } else {
        329.698_73 * (t - 60.0).powf(-0.133_204_76)
    };

    let g = if t <= 66.0 {
        99.470_8 * t.ln() - 161.119_57
    } else {
        288.122_17 * (t - 60.0).powf(-0.075_514_85)
    };

    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_77
    };

    [
        (r / 255.0).clamp(0.0, 1.0),
        (g / 255.0).clamp(0.0, 1.0),
        (b / 255.0).clamp(0.0, 1.0),
    ]
}

/// Build a display gamma ramp that tints toward `rgb_scale` and dims by
/// `brightness`.
///
/// * `rgb_scale` is typically `cct_to_rgb(kelvin)`.
/// * `brightness` is the 0.0..=1.0 control from the engine.
/// * `min_lum` is the luminance floor (0.0..1.0) so the screen never goes fully
///   black even when `brightness` is 0 — important at deep night.
///
/// The base is an identity ramp (`value = index * 257`, exact 0..=65535); each
/// channel is scaled by `rgb_scale[ch] * lum`. This produces a monotonic,
/// in-range ramp that Windows will accept.
pub fn build_gamma_ramp(rgb_scale: [f32; 3], brightness: f32, min_lum: f32) -> GammaRamp {
    let b = brightness.clamp(0.0, 1.0);
    let floor = min_lum.clamp(0.0, 1.0);
    let lum = floor + (1.0 - floor) * b;

    let mut ramp: GammaRamp = [[0u16; 256]; 3];
    for ch in 0..3 {
        let scale = rgb_scale[ch].clamp(0.0, 1.0) * lum;
        for (i, slot) in ramp[ch].iter_mut().enumerate() {
            // exact identity: i * 257 == i * 65535 / 255
            let base = (i as f32) * 257.0;
            *slot = (base * scale).round().clamp(0.0, 65535.0) as u16;
        }
    }
    ramp
}

/// Identity ramp (no tint, no dim). Used to recover a stuck display after a
/// crash that skipped the normal restore path.
pub fn identity_ramp() -> GammaRamp {
    build_gamma_ramp([1.0, 1.0, 1.0], 1.0, 0.0)
}

/// Win11 rejects (silently) any entry more than 32768 away from identity.
/// Pull every slot back so `SetDeviceGammaRamp` cannot no-op the whole LUT.
pub fn clamp_ramp_to_driver(mut ramp: GammaRamp) -> GammaRamp {
    for channel in &mut ramp {
        for (i, slot) in channel.iter_mut().enumerate() {
            let ident = (i as i32) * 257;
            let lo = (ident - 32768).max(0);
            let hi = (ident + 32768).min(65535);
            *slot = (*slot as i32).clamp(lo, hi) as u16;
        }
    }
    ramp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_at_6500k_is_near_white() {
        let [r, g, b] = cct_to_rgb(6500.0);
        assert!((r - 1.0).abs() < 1e-3, "red {r}");
        assert!(g > 0.9, "green {g}");
        assert!(b > 0.9, "blue {b}");
    }

    #[test]
    fn warm_drops_blue() {
        let [r, _g, b] = cct_to_rgb(2000.0);
        assert!((r - 1.0).abs() < 1e-3, "red should saturate when warm: {r}");
        assert!(b < 0.2, "blue should be low when warm: {b}");
    }

    #[test]
    fn blue_increases_with_temperature() {
        let warm = cct_to_rgb(2300.0)[2];
        let mid = cct_to_rgb(3400.0)[2];
        let cool = cct_to_rgb(6500.0)[2];
        assert!(
            warm < mid && mid < cool,
            "blue must rise with K: {warm} {mid} {cool}"
        );
    }

    #[test]
    fn ramp_is_monotonic_and_in_range() {
        let ramp = build_gamma_ramp([1.0, 1.0, 1.0], 1.0, 0.0);
        for (ch, channel) in ramp.iter().enumerate() {
            assert_eq!(channel[0], 0);
            assert_eq!(channel[255], 65535);
            for i in 1..256 {
                assert!(
                    channel[i] >= channel[i - 1],
                    "channel {ch} not monotonic at {i}"
                );
            }
        }
    }

    #[test]
    fn brightness_floor_keeps_screen_visible() {
        // brightness 0 with a 0.3 floor must still emit ~30% at the top entry.
        let ramp = build_gamma_ramp([1.0, 1.0, 1.0], 0.0, 0.3);
        let top = ramp[0][255];
        let expected = (65535.0 * 0.3) as u16;
        assert!(
            (top as i32 - expected as i32).abs() < 300,
            "floor not applied: {top}"
        );
    }

    #[test]
    fn driver_clamp_keeps_night_ramp_inside_win11_window() {
        // Night defaults used to produce scale < 0.5, which Win11 silently
        // rejects. After clamp, every entry is within 32768 of identity.
        let rgb = cct_to_rgb(3400.0);
        let raw = build_gamma_ramp(rgb, 0.18, 0.30);
        let ramp = clamp_ramp_to_driver(raw);
        for (ch, channel) in ramp.iter().enumerate() {
            for (i, slot) in channel.iter().enumerate() {
                let ident = (i as i32) * 257;
                let v = *slot as i32;
                assert!(
                    (v - ident).abs() <= 32768,
                    "ch {ch} i {i}: {v} vs ident {ident}"
                );
            }
        }
    }

    #[test]
    fn identity_ramp_is_exact() {
        let ramp = identity_ramp();
        assert_eq!(ramp[0][0], 0);
        assert_eq!(ramp[0][255], 65535);
        assert_eq!(ramp[1][255], 65535);
        assert_eq!(ramp[2][255], 65535);
    }
}
