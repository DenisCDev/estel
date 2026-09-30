//! Personal comfort bounds. Percentages are preferences, not photometric doses.

use crate::{Config, DayContext, Target};

/// Protect the three hours before bed and sleep, including overnight/daytime routines.
pub fn is_rest_period(now: f64, wake: f64, bed: f64) -> bool {
    let start = (bed - 180.0).rem_euclid(1440.0);
    (now - start).rem_euclid(1440.0) < (wake - start).rem_euclid(1440.0)
}

pub fn brightness_ceiling(now: f64, ctx: &DayContext, cfg: &Config) -> f32 {
    if now < ctx.sunrise_min
        || now >= ctx.sunset_min
        || is_rest_period(now, ctx.wake_min, ctx.bed_min)
    {
        cfg.rest_brightness_max
    } else {
        cfg.day_brightness_max
    }
}

/// Ramp warmth toward the personal warm floor before bed, even before sunset.
pub fn prepare_for_rest(mut target: Target, now: f64, ctx: &DayContext, cfg: &Config) -> Target {
    if is_rest_period(now, ctx.wake_min, ctx.bed_min) {
        let since_bed = (now - ctx.bed_min).rem_euclid(1440.0);
        let progress = if since_bed <= (ctx.wake_min - ctx.bed_min).rem_euclid(1440.0) {
            1.0
        } else {
            (1.0 - (ctx.bed_min - now).rem_euclid(1440.0) / 180.0) as f32
        };
        let mired = 1_000_000.0 / 6500.0
            + (1_000_000.0 / cfg.gamma_warm_floor_k - 1_000_000.0 / 6500.0)
                * progress.clamp(0.0, 1.0);
        target.cct_kelvin = target.cct_kelvin.min(1_000_000.0 / mired);
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(wake: f64, bed: f64) -> DayContext {
        DayContext {
            sunrise_min: 360.0,
            sunset_min: 1200.0,
            wake_min: wake,
            bed_min: bed,
        }
    }

    #[test]
    fn bedtime_protection_crosses_midnight_and_ends_at_wake() {
        assert!(is_rest_period(1380.0, 480.0, 60.0));
        assert!(is_rest_period(420.0, 480.0, 60.0));
        assert!(!is_rest_period(480.0, 480.0, 60.0));
        assert!(!is_rest_period(1319.0, 480.0, 60.0));
    }

    #[test]
    fn early_bed_and_day_sleep_are_protected_without_camera() {
        let cfg = Config::default();
        assert_eq!(
            brightness_ceiling(960.0, &context(360.0, 1140.0), &cfg),
            0.25
        );
        assert_eq!(
            brightness_ceiling(600.0, &context(960.0, 480.0), &cfg),
            0.25
        );
        assert_eq!(
            brightness_ceiling(960.0, &context(960.0, 480.0), &cfg),
            0.85
        );
        let warm = prepare_for_rest(Target::neutral(), 480.0, &context(960.0, 480.0), &cfg);
        assert!((warm.cct_kelvin - 3400.0).abs() < 1.0);
    }

    #[test]
    fn personal_ceiling_applies_to_any_source_and_color_intensity() {
        let cfg = Config {
            rest_brightness_max: 0.32,
            ..Config::default()
        };
        let cap = brightness_ceiling(1320.0, &context(420.0, 1380.0), &cfg);
        for source in [0.16_f32, 0.70, 1.0] {
            for intensity in [0.3, 0.6, 1.0] {
                let target = Target {
                    brightness: source,
                    ..Target::neutral()
                }
                .attenuate(intensity);
                assert!(target.brightness.clamp(cfg.min_brightness, cap) <= 0.32);
            }
        }
    }
}
