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
use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
use windows::core::BOOL;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

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
                let saved = session::load_ddc_originals();
                for (i, mon) in states.iter_mut().enumerate() {
                    if let Some(&v) = saved.get(i) {
                        mon.original = v.clamp(mon.min, mon.max);
                        unsafe {
                            let _ = SetMonitorBrightness(handle(mon.raw_handle), mon.original);
                        }
                    }
                }
            } else {
                let originals: Vec<u32> = states.iter().map(|m| m.original).collect();
                session::save_ddc_originals(&originals);
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

unsafe extern "system" fn on_monitor(
    hmon: HMONITOR,
    _hdc: HDC,
    _rc: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    list.push(hmon);
    BOOL(1)
}

fn enum_hmonitors() -> Vec<HMONITOR> {
    let mut mons = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(on_monitor),
            LPARAM(&mut mons as *mut Vec<HMONITOR> as isize),
        );
    }
    mons
}

fn build_states() -> anyhow::Result<Vec<MonState>> {
    unsafe {
        let mut states = Vec::new();
        for hmon in enum_hmonitors() {
            let mut count = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count).is_err() || count == 0 {
                continue;
            }
            let mut phys: Vec<PHYSICAL_MONITOR> = (0..count)
                .map(|_| PHYSICAL_MONITOR::default())
                .collect();
            if GetPhysicalMonitorsFromHMONITOR(hmon, &mut phys).is_err() {
                continue;
            }
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
        }
        Ok(states)
    }
}
