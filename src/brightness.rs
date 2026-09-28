//! DDC/CI physical backlight brightness control.
//!
//! Gamma handles CCT. DDC handles real dimming on external monitors that
//! speak MCCS. Built-in laptop panels usually do not — then the overlay dims.
//!
//! Restore is idempotent: `DestroyPhysicalMonitor` runs once. Original
//! backlight is persisted so a killed process can put it back on next launch.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitor, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR, SetMonitorBrightness,
};
use windows::Win32::Foundation::{GetLastError, HANDLE, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR,
    MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;
use windows::core::{BOOL, PCWSTR};

use crate::session;

struct MonState {
    id: String,
    raw_handle: usize,
    min: u32,
    original: u32,
    max: u32,
}

unsafe impl Send for MonState {}
unsafe impl Sync for MonState {}

static MONITORS: OnceLock<Vec<MonState>> = OnceLock::new();
static TOPOLOGY: OnceLock<Vec<usize>> = OnceLock::new();
static LAST_Q: AtomicU32 = AtomicU32::new(u32::MAX);
static DDC_FALLBACK: AtomicBool = AtomicBool::new(false);
static FALLBACK_RESTORED: AtomicBool = AtomicBool::new(false);
static RESTORED: AtomicBool = AtomicBool::new(false);
static RESTORE_OK: AtomicBool = AtomicBool::new(false);

/// Initialise DDC brightness only when every attached monitor supports it.
pub fn init() -> bool {
    let _ = TOPOLOGY.set(current_topology());
    match build_states() {
        Ok((mut states, mut complete)) if !states.is_empty() => {
            if session::is_dirty() {
                match session::load_ddc_originals() {
                    session::DdcSnapshot::Named(saved) => {
                        if let Some(by_id) = named_restore_values(&saved) {
                            for mon in &mut states {
                                if let Some(&value) = by_id.get(mon.id.as_str()) {
                                    mon.original = value.clamp(mon.min, mon.max);
                                    if unsafe {
                                        SetMonitorBrightness(handle(mon.raw_handle), mon.original)
                                    } == 0
                                    {
                                        tracing::warn!(id = %mon.id, "não foi possível recuperar o brilho DDC");
                                        complete = false;
                                    }
                                }
                            }
                            if !named_mapping_complete(&saved, &states) {
                                complete = false;
                                tracing::warn!(
                                    "monitores diferentes do registro DDC; recuperação parcial preservada"
                                );
                            }
                        } else {
                            complete = false;
                            tracing::warn!("identidades DDC duplicadas; recuperação adiada");
                        }
                    }
                    session::DdcSnapshot::Legacy(saved)
                        if legacy_mapping_complete(&saved, &states) =>
                    {
                        let mon = &mut states[0];
                        mon.original = saved[0].clamp(mon.min, mon.max);
                        if unsafe { SetMonitorBrightness(handle(mon.raw_handle), mon.original) }
                            == 0
                        {
                            tracing::warn!("não foi possível recuperar o brilho DDC anterior");
                            complete = false;
                        }
                    }
                    session::DdcSnapshot::Legacy(_) => {
                        tracing::warn!(
                            "registro antigo não identifica cada tela; recuperação DDC adiada"
                        );
                        complete = false;
                    }
                    session::DdcSnapshot::Invalid => complete = false,
                    session::DdcSnapshot::Missing => complete = false,
                }
            } else if complete {
                let originals: Vec<session::DdcOriginal> = states
                    .iter()
                    .map(|m| session::DdcOriginal {
                        id: m.id.clone(),
                        value: m.original,
                    })
                    .collect();
                if !session::save_ddc_originals(&originals) {
                    complete = false;
                }
            }
            let n = states.len();
            let _ = MONITORS.set(states);
            RESTORED.store(false, Ordering::SeqCst);
            DDC_FALLBACK.store(!complete, Ordering::SeqCst);
            FALLBACK_RESTORED.store(false, Ordering::SeqCst);
            if complete {
                tracing::info!(monitors = n, "DDC de brilho pronto");
            } else {
                tracing::warn!(
                    monitors = n,
                    "DDC parcial; brilho será aplicado por sobreposição em todas as telas"
                );
            }
            complete
        }
        Ok((_, _)) => {
            tracing::debug!("DDC: nenhum monitor respondeu");
            false
        }
        Err(e) => {
            tracing::debug!("DDC init: {e}");
            false
        }
    }
}

/// Apply `brightness` (0.0 = DDC min, 1.0 = DDC max).
/// No-op if brightness hasn't changed by more than 2 %.
/// Returns whether every monitor is using DDC for this target.
pub fn apply(brightness: f32) -> bool {
    if RESTORED.load(Ordering::SeqCst) {
        return false;
    }
    if !DDC_FALLBACK.load(Ordering::SeqCst)
        && TOPOLOGY
            .get()
            .is_some_and(|initial| monitor_topology_changed(initial, &current_topology()))
    {
        DDC_FALLBACK.store(true, Ordering::SeqCst);
        FALLBACK_RESTORED.store(false, Ordering::SeqCst);
        tracing::warn!("monitores alterados; brilho DDC desativado até reiniciar o Estel");
    }
    if DDC_FALLBACK.load(Ordering::SeqCst) {
        if !FALLBACK_RESTORED.load(Ordering::SeqCst) {
            let _ = park();
        }
        return false;
    }
    let states = match MONITORS.get() {
        Some(s) => s,
        None => return false,
    };
    let q = (brightness.clamp(0.0, 1.0) * 50.0) as u32;
    if LAST_Q.swap(q, Ordering::Relaxed) == q {
        return true;
    }
    let mut index = 0;
    let complete = write_all_brightness(states, brightness, |monitor, value| {
        index += 1;
        let ok = unsafe { SetMonitorBrightness(monitor, value) } != 0;
        if !ok {
            tracing::warn!(monitor = index, error = ?unsafe { GetLastError() }, "falha ao ajustar brilho DDC; usando sobreposição em todas as telas");
        }
        ok
    });
    if !complete {
        DDC_FALLBACK.store(true, Ordering::SeqCst);
        if !park() {
            tracing::warn!("não foi possível restaurar todos os monitores após falha DDC");
        }
    }
    complete
}

fn write_all_brightness(
    states: &[MonState],
    brightness: f32,
    mut write: impl FnMut(HANDLE, u32) -> bool,
) -> bool {
    let mut complete = true;
    for mon in states {
        let range = mon.max.saturating_sub(mon.min);
        let val =
            (mon.min + (brightness.clamp(0.0, 1.0) * range as f32) as u32).clamp(mon.min, mon.max);
        if !write(handle(mon.raw_handle), val) {
            complete = false;
        }
    }
    complete
}

/// Put the backlight back without releasing handles. Used by Pausar.
pub fn park() -> bool {
    let mut restored = true;
    if let Some(states) = MONITORS.get() {
        for mon in states {
            if unsafe { SetMonitorBrightness(handle(mon.raw_handle), mon.original) } == 0 {
                tracing::warn!(error = ?unsafe { GetLastError() }, "não foi possível restaurar o brilho do monitor");
                restored = false;
            }
        }
        match session::load_ddc_originals() {
            session::DdcSnapshot::Named(saved) if !named_mapping_complete(&saved, states) => {
                restored = false;
            }
            session::DdcSnapshot::Legacy(saved) if !legacy_mapping_complete(&saved, states) => {
                restored = false;
            }
            session::DdcSnapshot::Invalid => restored = false,
            _ => {}
        }
    } else {
        restored = matches!(session::load_ddc_originals(), session::DdcSnapshot::Missing);
    }
    LAST_Q.store(u32::MAX, Ordering::Relaxed);
    if DDC_FALLBACK.load(Ordering::SeqCst) {
        FALLBACK_RESTORED.store(restored, Ordering::SeqCst);
    }
    restored
}

fn named_mapping_complete(saved: &[session::DdcOriginal], states: &[MonState]) -> bool {
    if saved.len() != states.len() || saved.is_empty() {
        return false;
    }
    if named_restore_values(saved).is_none() {
        return false;
    }
    let ids = saved
        .iter()
        .map(|record| record.id.as_str())
        .collect::<HashSet<_>>();
    states.iter().all(|mon| ids.contains(mon.id.as_str()))
}

fn named_restore_values(saved: &[session::DdcOriginal]) -> Option<HashMap<&str, u32>> {
    let unique = saved
        .iter()
        .map(|record| record.id.as_str())
        .collect::<HashSet<_>>()
        .len()
        == saved.len();
    unique.then(|| {
        saved
            .iter()
            .map(|record| (record.id.as_str(), record.value))
            .collect()
    })
}

fn legacy_mapping_complete(saved: &[u32], states: &[MonState]) -> bool {
    saved.len() == 1 && states.len() == 1
}

/// Restore original backlight and release DDC handles. Idempotent. Exit path.
pub fn restore() -> bool {
    if RESTORED.swap(true, Ordering::SeqCst) {
        return RESTORE_OK.load(Ordering::SeqCst);
    }
    let restored = park();
    if let Some(states) = MONITORS.get() {
        for mon in states {
            unsafe {
                let _ = DestroyPhysicalMonitor(handle(mon.raw_handle));
            }
        }
    }
    if restored {
        session::clear_ddc_original();
    }
    RESTORE_OK.store(restored, Ordering::SeqCst);
    restored
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

fn build_states() -> anyhow::Result<(Vec<MonState>, bool)> {
    unsafe {
        let mut states = Vec::new();
        let mut complete = true;
        let mut seen = HashSet::new();
        for hmon in enum_hmonitors() {
            let mut count = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count).is_err() || count == 0 {
                complete = false;
                continue;
            }
            let mut phys: Vec<PHYSICAL_MONITOR> =
                (0..count).map(|_| PHYSICAL_MONITOR::default()).collect();
            if GetPhysicalMonitorsFromHMONITOR(hmon, &mut phys).is_err() {
                complete = false;
                continue;
            }
            let identity = if count == 1 {
                monitor_identity(hmon)
            } else {
                None
            };
            if identity.is_none() || identity.as_ref().is_some_and(|id| !seen.insert(id.clone())) {
                complete = false;
                for p in &phys {
                    let _ = DestroyPhysicalMonitor(p.hPhysicalMonitor);
                }
                continue;
            }
            for p in &phys {
                let mut mn = 0u32;
                let mut cur = 0u32;
                let mut mx = 0u32;
                let ok = GetMonitorBrightness(p.hPhysicalMonitor, &mut mn, &mut cur, &mut mx);
                if ok != 0 && mx > mn {
                    states.push(MonState {
                        id: identity.clone().unwrap_or_default(),
                        raw_handle: p.hPhysicalMonitor.0 as usize,
                        min: mn,
                        original: cur,
                        max: mx,
                    });
                } else {
                    complete = false;
                    let _ = DestroyPhysicalMonitor(p.hPhysicalMonitor);
                }
            }
        }
        Ok((states, complete))
    }
}

fn monitor_identity(hmon: HMONITOR) -> Option<String> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !unsafe {
        GetMonitorInfoW(
            hmon,
            (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
        )
    }
    .as_bool()
    {
        return None;
    }
    let mut device = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    if !unsafe {
        EnumDisplayDevicesW(
            PCWSTR::from_raw(info.szDevice.as_ptr()),
            0,
            &mut device,
            EDD_GET_DEVICE_INTERFACE_NAME,
        )
    }
    .as_bool()
    {
        return None;
    }
    let end = device.DeviceID.iter().position(|&unit| unit == 0)?;
    if end == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&device.DeviceID[..end]).to_ascii_lowercase())
}

fn current_topology() -> Vec<usize> {
    let mut ids = enum_hmonitors()
        .into_iter()
        .map(|monitor| monitor.0 as usize)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

fn monitor_topology_changed(initial: &[usize], current: &[usize]) -> bool {
    initial != current
}

#[cfg(test)]
mod tests {
    use super::{
        MonState, legacy_mapping_complete, monitor_topology_changed, named_mapping_complete,
        named_restore_values, write_all_brightness,
    };
    use crate::session::DdcOriginal;

    #[test]
    fn saved_brightness_matches_identity_in_any_enumeration_order() {
        let saved = vec![
            DdcOriginal {
                id: "a".into(),
                value: 20,
            },
            DdcOriginal {
                id: "b".into(),
                value: 80,
            },
        ];
        let states = vec![
            MonState {
                id: "b".into(),
                raw_handle: 1,
                min: 0,
                original: 40,
                max: 100,
            },
            MonState {
                id: "a".into(),
                raw_handle: 2,
                min: 0,
                original: 40,
                max: 100,
            },
        ];
        assert!(named_mapping_complete(&saved, &states));
        assert_eq!(
            saved
                .iter()
                .find(|record| record.id == states[0].id)
                .unwrap()
                .value,
            80
        );
        assert_eq!(
            saved
                .iter()
                .find(|record| record.id == states[1].id)
                .unwrap()
                .value,
            20
        );
    }

    #[test]
    fn missing_or_replaced_monitor_keeps_ddc_in_fallback() {
        let saved = vec![
            DdcOriginal {
                id: "a".into(),
                value: 20,
            },
            DdcOriginal {
                id: "b".into(),
                value: 80,
            },
        ];
        let b = MonState {
            id: "b".into(),
            raw_handle: 1,
            min: 0,
            original: 40,
            max: 100,
        };
        let c = MonState {
            id: "c".into(),
            raw_handle: 2,
            min: 0,
            original: 40,
            max: 100,
        };
        assert!(!named_mapping_complete(&saved, &[b]));
        assert!(!named_mapping_complete(&saved, &[c]));
        let values = named_restore_values(&saved).unwrap();
        assert_eq!(values.get("b"), Some(&80));
        assert_eq!(values.get("c"), None);
    }

    #[test]
    fn duplicate_ids_cannot_be_restored_by_name() {
        let saved = vec![
            DdcOriginal {
                id: "a".into(),
                value: 20,
            },
            DdcOriginal {
                id: "a".into(),
                value: 80,
            },
        ];
        assert!(named_restore_values(&saved).is_none());
    }

    #[test]
    fn legacy_two_monitor_snapshot_cannot_be_consumed() {
        let states = vec![
            MonState {
                id: "a".into(),
                raw_handle: 1,
                min: 0,
                original: 40,
                max: 100,
            },
            MonState {
                id: "b".into(),
                raw_handle: 2,
                min: 0,
                original: 40,
                max: 100,
            },
        ];
        assert!(!legacy_mapping_complete(&[20, 80], &states));
        assert!(!legacy_mapping_complete(&[20], &states));
        assert!(legacy_mapping_complete(&[20], &states[..1]));
    }

    #[test]
    fn attaching_or_replacing_a_display_disables_old_ddc_mapping() {
        assert!(!monitor_topology_changed(&[1, 2], &[1, 2]));
        assert!(monitor_topology_changed(&[1, 2], &[1, 2, 3]));
        assert!(monitor_topology_changed(&[1, 2], &[1, 3]));
    }

    #[test]
    fn failed_write_on_one_monitor_fails_the_whole_ddc_batch() {
        let monitors = vec![
            MonState {
                id: "first".into(),
                raw_handle: 1,
                min: 0,
                original: 50,
                max: 100,
            },
            MonState {
                id: "second".into(),
                raw_handle: 2,
                min: 20,
                original: 60,
                max: 80,
            },
        ];
        let mut writes = Vec::new();
        let complete = write_all_brightness(&monitors, 0.5, |handle, value| {
            writes.push((handle.0 as usize, value));
            handle.0 as usize != 2
        });
        assert!(!complete);
        assert_eq!(writes, vec![(1, 50), (2, 50)]);
    }
}
