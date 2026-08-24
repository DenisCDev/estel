//! Windows gamma-ramp display controller — every attached output, not just
//! the primary. HDR heads fail GetDeviceGammaRamp; we skip those and let the
//! overlay cover them.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::GetLastError;
use windows::Win32::Graphics::Gdi::{
    CreateDCW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP,
    DISPLAY_DEVICE_MIRRORING_DRIVER, DeleteDC, EnumDisplayDevicesW,
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
static RESTORED: AtomicBool = AtomicBool::new(false);

pub fn init() -> bool {
    if session::is_dirty() {
        tracing::warn!("sessão anterior não restaurou o monitor — aplicando rampa identidade");
        for name in enum_device_names() {
            let _ = write_named(&name, &identity_ramp());
        }
    }

    let mut heads = Vec::new();
    for name in enum_device_names() {
        match read_named(&name) {
            Ok(ramp) => heads.push(Head { name, saved: ramp }),
            Err(e) => tracing::debug!(
                "gamma indisponível em um output ({e}) — overlay cobre esse"
            ),
        }
    }

    if heads.is_empty() {
        tracing::warn!("gamma ramp indisponível em todos os monitores — usando só a sobreposição");
        false
    } else {
        tracing::info!(outputs = heads.len(), "gamma pronta");
        RESTORED.store(false, Ordering::SeqCst);
        let _ = HEADS.set(heads);
        true
    }
}

pub fn apply(target: &Target, gamma_floor_k: f32, min_lum: f32) -> anyhow::Result<bool> {
    let heads = match HEADS.get() {
        Some(h) if !h.is_empty() => h,
        _ => return Ok(false),
    };
    let cct = target.cct_kelvin.max(gamma_floor_k);
    let rgb = cct_to_rgb(cct);
    let safe_min = min_lum.max(0.52);
    let ramp = clamp_ramp_to_driver(build_gamma_ramp(rgb, target.brightness, safe_min));
    let mut any = false;
    for head in heads {
        if write_named(&head.name, &ramp).is_ok() {
            any = true;
        }
    }
    Ok(any)
}

pub fn park() {
    if let Some(heads) = HEADS.get() {
        for head in heads {
            let _ = write_named(&head.name, &head.saved);
        }
    } else {
        for name in enum_device_names() {
            let _ = write_named(&name, &identity_ramp());
        }
    }
}

pub fn restore() {
    if RESTORED.swap(true, Ordering::SeqCst) {
        return;
    }
    park();
    session::mark_clean();
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
