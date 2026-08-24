//! Ambient pink/brown noise with a hard volume cap and slow envelopes.
//!
//! Optional and non-fatal: no output device → `Audio::try_new` returns `None`.
//!
//! Startle protection (Blumenthal & Berg, 1986): never jump the volume. Every
//! start and stop uses a ≥3 s raised-cosine fade. There is no chime — a tone
//! on a phase change is itself an acoustic startle, even with a Hann window.
//!
//! Pink → `rodio::source::noise::Pink` (1/f). Brown → `Red` (1/f²).

use std::num::NonZero;
use std::time::Instant;

use rodio::Player;
use rodio::source::noise::{Pink, Red};
use rodio::{MixerDeviceSink, SampleRate};

use crate::target::NoiseColor;

const SR: u32 = 44_100;
/// Noise is a wash under other audio, never a foreground track.
const NOISE_SCALE: f32 = 0.10;
/// Absolute ceiling after scale. User `max_volume` cannot exceed this.
const HARD_CAP: f32 = 0.12;
/// Seconds to fade between silence and target (smoothstep).
const FADE_SECS: f32 = 4.0;

fn sample_rate() -> SampleRate {
    NonZero::new(SR).expect("44100 != 0")
}

/// Final mixer volume for Estel's noise. Always ≤ [`HARD_CAP`].
pub fn noise_volume(noise_gain: f32, max_volume: f32) -> f32 {
    (noise_gain.clamp(0.0, 1.0) * max_volume.clamp(0.0, 1.0) * NOISE_SCALE).clamp(0.0, HARD_CAP)
}

/// Holds the output stream and the noise player. Drop stops audio.
pub struct Audio {
    _sink: MixerDeviceSink,
    noise_player: Player,
    active_color: Option<NoiseColor>,
    current: f32,
    fade_from: f32,
    fade_to: f32,
    fade_started: Instant,
}

impl Audio {
    /// Open the default device. `None` when none is available.
    pub fn try_new() -> Option<Self> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| tracing::warn!("áudio indisponível: {e}"))
            .ok()?;
        sink.log_on_drop(false);

        let noise_player = Player::connect_new(sink.mixer());
        Some(Audio {
            _sink: sink,
            noise_player,
            active_color: None,
            current: 0.0,
            fade_from: 0.0,
            fade_to: 0.0,
            fade_started: Instant::now(),
        })
    }

    /// Drive noise each pump tick (~50 ms). Call even when the schedule is
    /// silent so fade-out can finish.
    ///
    /// Volume is forced to 0 *before* `append` — rodio's Player defaults to
    /// 1.0, and the first mixer quantum would otherwise be full-scale noise.
    pub fn tick(&mut self, color: Option<NoiseColor>, noise_gain: f32, max_volume: f32) {
        if color != self.active_color {
            match color {
                Some(c) => {
                    self.noise_player.set_volume(0.0);
                    self.current = 0.0;
                    self.noise_player.stop();
                    self.noise_player.set_volume(0.0);
                    append_noise(&self.noise_player, c);
                    self.noise_player.set_volume(0.0);
                }
                None => {
                    // Keep the source playing until fade-out hits zero.
                }
            }
            self.active_color = color;
        }

        let new_target = if color.is_some() {
            noise_volume(noise_gain, max_volume)
        } else {
            0.0
        };
        if (new_target - self.fade_to).abs() > 1e-5 {
            self.fade_from = self.current;
            self.fade_to = new_target;
            self.fade_started = Instant::now();
        }

        self.current = fade_gain(
            self.fade_from,
            self.fade_to,
            self.fade_started.elapsed().as_secs_f32(),
        );

        let vol = self.current.clamp(0.0, HARD_CAP);
        self.noise_player.set_volume(vol);

        if vol <= 1e-4 && color.is_none() {
            self.noise_player.stop();
            self.active_color = None;
        }
    }

    /// Immediate silence (pause / exit). Fade is skipped because the process
    /// is about to drop the stream anyway; the envelope already covers live use.
    pub fn silence(&mut self) {
        self.fade_from = 0.0;
        self.fade_to = 0.0;
        self.current = 0.0;
        self.active_color = None;
        self.noise_player.stop();
        self.noise_player.set_volume(0.0);
    }
}

/// Raised-cosine from `from` to `to` over [`FADE_SECS`]. Independent of
/// how small `to` is — a 0.005 target still takes the full envelope.
pub fn fade_gain(from: f32, to: f32, elapsed: f32) -> f32 {
    let t = (elapsed / FADE_SECS).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t);
    from + (to - from) * s
}

fn append_noise(player: &Player, color: NoiseColor) {
    let sr = sample_rate();
    match color {
        NoiseColor::Pink => player.append(Pink::new(sr)),
        NoiseColor::Brown => player.append(Red::new(sr)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_never_exceeds_hard_cap() {
        assert!(noise_volume(1.0, 1.0) <= HARD_CAP);
        assert!(noise_volume(1.0, 10.0) <= HARD_CAP);
        assert!(noise_volume(99.0, 99.0) <= HARD_CAP);
    }

    #[test]
    fn volume_zero_when_gain_or_cap_is_zero() {
        assert_eq!(noise_volume(0.0, 1.0), 0.0);
        assert_eq!(noise_volume(1.0, 0.0), 0.0);
    }

    #[test]
    fn volume_scales_inside_the_cap() {
        let a = noise_volume(1.0, 0.5);
        let b = noise_volume(0.5, 0.5);
        assert!(a > b);
        assert!(a < HARD_CAP || (a - HARD_CAP).abs() < 1e-6);
    }

    #[test]
    fn fade_uses_the_full_envelope_even_for_a_quiet_target() {
        let to = noise_volume(0.15, 0.35);
        assert!(to > 0.0);
        assert!((fade_gain(0.0, to, 0.0) - 0.0).abs() < 1e-6);
        assert!((fade_gain(0.0, to, FADE_SECS) - to).abs() < 1e-6);
        let mid = fade_gain(0.0, to, FADE_SECS / 2.0);
        assert!((mid - to / 2.0).abs() < 1e-5);
        assert!(fade_gain(0.0, to, 0.2) < to * 0.05);
    }
}
