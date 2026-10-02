//! Estel — a calm, circadian ambient environment for the desktop.
//!
//! Open-loop by time and location: no biometrics, no prompts, no data
//! collection. Adjunctive comfort, not treatment.

#[cfg(windows)]
pub mod ambient;
pub mod audio;
#[cfg(windows)]
pub mod brightness;
pub mod color;
pub mod comfort;
pub mod config;
#[cfg(windows)]
pub mod display;
#[cfg(windows)]
pub mod display_topology;
#[cfg(windows)]
pub mod hardware_wmi;
#[cfg(windows)]
pub mod hardware_worker;
#[cfg(windows)]
pub mod location;
#[cfg(windows)]
pub mod overlay;
#[cfg(windows)]
pub mod runtime;
pub mod schedule;
pub mod session;
#[cfg(windows)]
pub mod status;
pub mod target;
#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod ui;
#[cfg(windows)]
pub mod update;
pub mod weather;

pub use color::{GammaRamp, build_gamma_ramp, cct_to_rgb, clamp_ramp_to_driver, identity_ramp};
pub use config::Config;
pub use schedule::{Anchor, DayContext, Keypoint, Schedule};
pub use target::{NoiseColor, Target};
