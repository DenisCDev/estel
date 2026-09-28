//! Windows gamma-ramp display controller — every attached output, not just
//! the primary. HDR heads fail GetDeviceGammaRamp; we skip those and let the
//! overlay cover them.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::GetLastError;
use windows::Win32::Graphics::Gdi::{
    CreateDCW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_MIRRORING_DRIVER,
    DISPLAY_DEVICEW, DeleteDC, EnumDisplayDevicesW,
};
use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};
use windows::core::{PCWSTR, w};

use crate::color::{GammaRamp, build_gamma_ramp, cct_to_rgb, clamp_ramp_to_driver, identity_ramp};
use crate::session;
use crate::target::Target;

struct Head {
    name: [u16; 32],
    saved: GammaRamp,
}

static HEADS: OnceLock<Vec<Head>> = OnceLock::new();
static TOPOLOGY: OnceLock<Vec<[u16; 32]>> = OnceLock::new();
static RESTORED: AtomicBool = AtomicBool::new(false);
static RESTORE_OK: AtomicBool = AtomicBool::new(false);
static RECOVERY_FAILED: AtomicBool = AtomicBool::new(false);
static GAMMA_FALLBACK: AtomicBool = AtomicBool::new(false);
static FALLBACK_RESTORED: AtomicBool = AtomicBool::new(false);

pub fn init() -> bool {
    let recovering = session::is_dirty();
    if recovering {
        tracing::warn!("sessão anterior não restaurou o monitor — aplicando rampa identidade");
        let mut recovered = true;
        for name in enum_device_names() {
            if let Err(error) = write_named(&name, &identity_ramp()) {
                tracing::warn!(%error, "não foi possível recuperar a gama da sessão anterior");
                recovered = false;
            }
        }
        RECOVERY_FAILED.store(!recovered, Ordering::SeqCst);
    }

    let names = enum_device_names();
    let _ = TOPOLOGY.set(sorted_names(names.clone()));
    let mut heads = Vec::new();
    for name in &names {
        match read_named(name) {
            Ok(ramp) => heads.push(Head {
                name: *name,
                saved: if recovering { identity_ramp() } else { ramp },
            }),
            Err(e) => tracing::debug!("gamma indisponível em um output ({e}) — overlay cobre esse"),
        }
    }

    if heads.is_empty() {
        tracing::warn!("gamma ramp indisponível em todos os monitores — usando só a sobreposição");
        false
    } else {
        let complete = heads.len() == names.len();
        tracing::info!(outputs = heads.len(), complete, "gamma pronta");
        RESTORED.store(false, Ordering::SeqCst);
        GAMMA_FALLBACK.store(!complete, Ordering::SeqCst);
        FALLBACK_RESTORED.store(false, Ordering::SeqCst);
        let _ = HEADS.set(heads);
        complete
    }
}

pub fn apply(target: &Target, gamma_floor_k: f32, min_lum: f32) -> anyhow::Result<bool> {
    if !GAMMA_FALLBACK.load(Ordering::SeqCst)
        && TOPOLOGY.get().is_some_and(|initial| {
            output_topology_changed(initial, &sorted_names(enum_device_names()))
        })
    {
        GAMMA_FALLBACK.store(true, Ordering::SeqCst);
        FALLBACK_RESTORED.store(false, Ordering::SeqCst);
        tracing::warn!("monitores alterados; gama desativada até reiniciar o Estel");
    }
    if GAMMA_FALLBACK.load(Ordering::SeqCst) {
        if !FALLBACK_RESTORED.load(Ordering::SeqCst) {
            let _ = park();
        }
        return Ok(false);
    }
    let heads = match HEADS.get() {
        Some(h) if !h.is_empty() => h,
        _ => return Ok(false),
    };
    let cct = target.cct_kelvin.max(gamma_floor_k);
    let rgb = cct_to_rgb(cct);
    let safe_min = min_lum.max(0.52);
    let ramp = clamp_ramp_to_driver(build_gamma_ramp(rgb, target.brightness, safe_min));
    let complete = write_all_named(heads, &ramp, write_named);
    if !complete {
        GAMMA_FALLBACK.store(true, Ordering::SeqCst);
        if !park() {
            tracing::warn!("não foi possível restaurar todos os monitores após falha de gama");
        }
    }
    Ok(complete)
}

fn write_all_named(
    heads: &[Head],
    ramp: &GammaRamp,
    mut write: impl FnMut(&[u16; 32], &GammaRamp) -> anyhow::Result<()>,
) -> bool {
    let mut complete = true;
    for (index, head) in heads.iter().enumerate() {
        if let Err(error) = write(&head.name, ramp) {
            tracing::warn!(monitor = index + 1, %error, "falha ao ajustar cor; usando sobreposição em todas as telas");
            complete = false;
        }
    }
    complete
}

pub fn park() -> bool {
    if RECOVERY_FAILED.load(Ordering::SeqCst) {
        let names = enum_device_names();
        let mut recovered = !names.is_empty();
        for name in names {
            if let Err(error) = write_named(&name, &identity_ramp()) {
                tracing::warn!(%error, "não foi possível recuperar a gama do monitor");
                recovered = false;
            }
        }
        if recovered {
            RECOVERY_FAILED.store(false, Ordering::SeqCst);
        }
        if GAMMA_FALLBACK.load(Ordering::SeqCst) {
            FALLBACK_RESTORED.store(recovered, Ordering::SeqCst);
        }
        return recovered;
    }
    let mut restored = true;
    if let Some(heads) = HEADS.get() {
        for head in heads {
            if let Err(error) = write_named(&head.name, &head.saved) {
                tracing::warn!(%error, "não foi possível restaurar a gama do monitor");
                restored = false;
            }
        }
    }
    if GAMMA_FALLBACK.load(Ordering::SeqCst) {
        FALLBACK_RESTORED.store(restored, Ordering::SeqCst);
    }
    restored
}

pub fn restore() -> bool {
    if RESTORED.swap(true, Ordering::SeqCst) {
        return RESTORE_OK.load(Ordering::SeqCst);
    }
    let restored = park();
    RESTORE_OK.store(restored, Ordering::SeqCst);
    restored
}

fn enum_device_names() -> Vec<[u16; 32]> {
    let mut names = Vec::new();
    let mut i = 0u32;
    loop {
        let mut dev = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        let ok = unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dev, 0) };
        if !ok.as_bool() {
            break;
        }
        i += 1;
        if !dev.StateFlags.contains(DISPLAY_DEVICE_ATTACHED_TO_DESKTOP)
            || dev.StateFlags.contains(DISPLAY_DEVICE_MIRRORING_DRIVER)
        {
            continue;
        }
        names.push(dev.DeviceName);
    }
    names
}

fn open_named(name: &[u16; 32]) -> anyhow::Result<windows::Win32::Graphics::Gdi::HDC> {
    unsafe {
        let hdc = CreateDCW(
            w!("DISPLAY"),
            PCWSTR::from_raw(name.as_ptr()),
            PCWSTR::null(),
            None,
        );
        if hdc.0.is_null() {
            anyhow::bail!("CreateDCW retornou nulo");
        }
        Ok(hdc)
    }
}

fn read_named(name: &[u16; 32]) -> anyhow::Result<GammaRamp> {
    unsafe {
        let hdc = open_named(name)?;
        let mut ramp: GammaRamp = [[0u16; 256]; 3];
        let ok = GetDeviceGammaRamp(hdc, ramp.as_mut_ptr().cast());
        let err = GetLastError();
        let _ = DeleteDC(hdc);
        if !ok.as_bool() {
            anyhow::bail!("GetDeviceGammaRamp falhou (Win32 {:#x})", err.0);
        }
        Ok(ramp)
    }
}

fn write_named(name: &[u16; 32], ramp: &GammaRamp) -> anyhow::Result<()> {
    unsafe {
        let hdc = open_named(name)?;
        let ok = SetDeviceGammaRamp(hdc, ramp.as_ptr().cast());
        let err = GetLastError();
        let _ = DeleteDC(hdc);
        if !ok.as_bool() {
            anyhow::bail!("SetDeviceGammaRamp falhou (Win32 {:#x})", err.0);
        }
        Ok(())
    }
}

fn sorted_names(mut names: Vec<[u16; 32]>) -> Vec<[u16; 32]> {
    names.sort_unstable();
    names
}

fn output_topology_changed(initial: &[[u16; 32]], current: &[[u16; 32]]) -> bool {
    initial != current
}

#[cfg(test)]
mod tests {
    use super::{Head, identity_ramp, output_topology_changed, write_all_named};

    #[test]
    fn attaching_a_display_disables_old_gamma_mapping() {
        let before = [[1_u16; 32]];
        let after = [[1_u16; 32], [2_u16; 32]];
        assert!(!output_topology_changed(&before, &before));
        assert!(output_topology_changed(&before, &after));
    }

    #[test]
    fn gamma_batch_fails_when_one_display_rejects_it() {
        let mut first = [0_u16; 32];
        first[0] = 1;
        let mut second = [0_u16; 32];
        second[0] = 2;
        let heads = vec![
            Head {
                name: first,
                saved: identity_ramp(),
            },
            Head {
                name: second,
                saved: identity_ramp(),
            },
        ];
        let mut attempted = Vec::new();
        let applied = write_all_named(&heads, &identity_ramp(), |name, _| {
            attempted.push(name[0]);
            if name[0] == 2 {
                anyhow::bail!("failed")
            }
            Ok(())
        });
        assert!(!applied);
        assert_eq!(attempted, vec![1, 2]);
    }
}
