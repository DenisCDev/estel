//! DDC/CI physical backlight brightness control.
//!
//! Gamma handles CCT. DDC handles real dimming on external monitors that
//! speak MCCS. Built-in laptop panels usually do not — then the overlay dims.
//!
//! Restore is idempotent: `DestroyPhysicalMonitor` runs once. Original
//! backlight is persisted so a killed process can put it back on next launch.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitor, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR, SetMonitorBrightness,
};
use windows::Win32::Foundation::{HANDLE, POINT};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};

use crate::session;

struct MonState {
    raw_handle: usize,
    min: u32,
    original: u32,
    max: u32,
}

unsafe impl Send for MonState {}
unsafe impl Sync for MonState {}

static MONITORS: OnceLock<Vec<MonState>> = OnceLock::new();
static LAST_Q: AtomicU32 = AtomicU32::new(u32::MAX);
static RESTORED: AtomicBool = AtomicBool::new(false);

/// Initialise DDC brightness. Returns `true` if at least one monitor supports it.
pub fn init() -> bool {
    match build_states() {
        Ok(mut states) if !states.is_empty() => {
            if session::is_dirty() {
                // Probe captured the *current* (possibly dimmed) level. Put
                // the persisted pre-Estel value back and keep it as original
                // — never rewrite ddc_original with the night snapshot.
                if let Some(saved) = session::load_ddc_original() {
                    for mon in &mut states {
                        mon.original = saved.clamp(mon.min, mon.max);
                        unsafe {
                            let _ = SetMonitorBrightness(handle(mon.raw_handle), mon.original);
                        }
                    }
                }
            } else if let Some(first) = states.first() {
                session::save_ddc_original(first.original);
            }
            let n = states.len();
            let _ = MONITORS.set(states);
            RESTORED.store(false, Ordering::SeqCst);
            tracing::info!(monitors = n, "DDC de brilho pronto");
            true
        }
        Ok(_) => {
            tracing::debug!("DDC: nenhum monitor respondeu");
            false
        }
        Err(e) => {
            tracing::debug!("DDC init: {e}");
            false
        }
    }
}

/// Whether DDC is actually driving a backlight. Overlay uses this to decide
/// if it must dim as well as tint.
pub fn is_active() -> bool {
    MONITORS.get().is_some_and(|s| !s.is_empty())
}

/// Apply `brightness` (0.0 = DDC min, 1.0 = DDC max).
/// No-op if brightness hasn't changed by more than 2 %.
pub fn apply(brightness: f32) {
    if RESTORED.load(Ordering::SeqCst) {
        return;
    }
    let states = match MONITORS.get() {
        Some(s) => s,
        None => return,
    };
    let q = (brightness.clamp(0.0, 1.0) * 50.0) as u32;
    if LAST_Q.swap(q, Ordering::Relaxed) == q {
        return;
    }
    for mon in states {
        let range = mon.max.saturating_sub(mon.min);
        let val = (mon.min + (brightness.clamp(0.0, 1.0) * range as f32) as u32)
            .clamp(mon.min, mon.max);
        unsafe {
            let _ = SetMonitorBrightness(handle(mon.raw_handle), val);
        }
    }
}

/// Put the backlight back without releasing handles. Used by Pausar.
pub fn park() {
    if let Some(states) = MONITORS.get() {
        for mon in states {
            unsafe {
                let _ = SetMonitorBrightness(handle(mon.raw_handle), mon.original);
            }
        }
    }
    LAST_Q.store(u32::MAX, Ordering::Relaxed);
}

/// Restore original backlight and release DDC handles. Idempotent. Exit path.
pub fn restore() {
    if RESTORED.swap(true, Ordering::SeqCst) {
        return;
    }
    park();
    if let Some(states) = MONITORS.get() {
        for mon in states {
            unsafe {
                let _ = DestroyPhysicalMonitor(handle(mon.raw_handle));
            }
        }
    }
    session::clear_ddc_original();
}

#[inline]
fn handle(raw: usize) -> HANDLE {
    HANDLE(raw as *mut core::ffi::c_void)
}

fn build_states() -> anyhow::Result<Vec<MonState>> {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);

        let mut count = 0u32;
        GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count)?;
        if count == 0 {
            return Ok(vec![]);
        }

        let mut phys: Vec<PHYSICAL_MONITOR> = (0..count)
            .map(|_| PHYSICAL_MONITOR::default())
            .collect();
        GetPhysicalMonitorsFromHMONITOR(hmon, &mut phys)?;

        let mut states = Vec::new();
        for p in &phys {
            let mut mn = 0u32;
            let mut cur = 0u32;
            let mut mx = 0u32;
            let ok = GetMonitorBrightness(p.hPhysicalMonitor, &mut mn, &mut cur, &mut mx);
            if ok != 0 && mx > mn {
                states.push(MonState {
                    raw_handle: p.hPhysicalMonitor.0 as usize,
                    min: mn,
                    original: cur,
                    max: mx,
                });
            } else {
                let _ = DestroyPhysicalMonitor(p.hPhysicalMonitor);
            }
        }
        Ok(states)
    }
}
