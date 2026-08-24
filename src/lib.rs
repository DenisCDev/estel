//! Estel — a calm, circadian ambient environment for the desktop.
//!
//! Open-loop by time and location: no biometrics, no prompts, no data
//! collection. Adjunctive comfort, not treatment.

pub mod audio;
#[cfg(windows)]
pub mod brightness;
pub mod color;
pub mod config;
#[cfg(windows)]
pub mod display;
#[cfg(windows)]
pub mod overlay;
pub mod schedule;
pub mod session;
#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod ui;
pub mod target;

pub use color::{GammaRamp, build_gamma_ramp, cct_to_rgb, clamp_ramp_to_driver, identity_ramp};
pub use config::Config;
pub use schedule::{Anchor, DayContext, Keypoint, Schedule};
pub use target::{NoiseColor, Target};
