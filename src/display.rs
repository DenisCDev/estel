//! Windows gamma-ramp display controller.
//!
//! Applies CCT + a *safe* brightness scale via `SetDeviceGammaRamp`. Win11
//! silently rejects ramps whose entries stray more than 32768 from identity
//! and still returns TRUE — so every ramp is clamped before write, and we
//! never dim below ~50 % through gamma. Extra dim is DDC or the overlay.
//!
//! Crash recovery: if the previous run left a dirty flag, identity is written
//! *before* snapshotting, so a Task-Manager kill cannot lock in a warm LUT.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::GetLastError;
use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC};
use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};
use windows::core::{PCWSTR, w};

use crate::color::{GammaRamp, build_gamma_ramp, cct_to_rgb, clamp_ramp_to_driver, identity_ramp};
use crate::session;
use crate::target::Target;

static SAVED_RAMP: OnceLock<GammaRamp> = OnceLock::new();
static RESTORED: AtomicBool = AtomicBool::new(false);

/// Snapshot the current gamma ramp. Returns `true` if gamma ramps are
/// supported. Call once at startup.
pub fn init() -> bool {
    if session::is_dirty() {
        tracing::warn!("sessão anterior não restaurou o monitor — aplicando rampa identidade");
        let _ = write_gamma(&identity_ramp());
    }

    match read_gamma() {
        Ok(ramp) => {
            let _ = SAVED_RAMP.set(ramp);
            RESTORED.store(false, Ordering::SeqCst);
            true
        }
        Err(e) => {
            tracing::warn!("gamma ramp indisponível ({e}) — usando só a sobreposição");
            false
        }
    }
}

/// Apply `target` to the primary display gamma ramp.
///
/// CCT is clamped to `gamma_floor_k` (≈3400 K). Luminance is clamped so the
/// ramp stays inside Win11's silent-reject window. Returns `true` if accepted
/// (verified via read-back).
pub fn apply(target: &Target, gamma_floor_k: f32, min_lum: f32) -> anyhow::Result<bool> {
    if SAVED_RAMP.get().is_none() {
        return Ok(false);
    }
    let cct = target.cct_kelvin.max(gamma_floor_k);
    let rgb = cct_to_rgb(cct);
    // Gamma must not dim below ~0.52: extra dim is DDC/overlay (see overlay).
    let safe_min = min_lum.max(0.52);
    let ramp = clamp_ramp_to_driver(build_gamma_ramp(rgb, target.brightness, safe_min));
    write_gamma(&ramp)?;
    let back = read_gamma()?;
    Ok(ramps_match(&ramp, &back))
}

/// Write the saved original ramp without finishing the session.
/// Used by Pausar so Retomar can apply again.
pub fn park() {
    if let Some(ramp) = SAVED_RAMP.get() {
        let _ = write_gamma(ramp);
    } else {
        let _ = write_gamma(&identity_ramp());
    }
}

/// Restore the original gamma ramp saved at `init()`. Idempotent. Exit path.
pub fn restore() {
    if RESTORED.swap(true, Ordering::SeqCst) {
        return;
    }
    park();
    session::mark_clean();
}

fn open_display_dc() -> anyhow::Result<windows::Win32::Graphics::Gdi::HDC> {
    unsafe {
        let hdc = CreateDCW(w!("DISPLAY"), PCWSTR::null(), PCWSTR::null(), None);
        if hdc.0.is_null() {
            anyhow::bail!("CreateDCW(\"DISPLAY\") retornou nulo");
        }
        Ok(hdc)
    }
}

fn read_gamma() -> anyhow::Result<GammaRamp> {
    unsafe {
        let hdc = open_display_dc()?;
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

fn write_gamma(ramp: &GammaRamp) -> anyhow::Result<()> {
    unsafe {
        let hdc = open_display_dc()?;
        let ok = SetDeviceGammaRamp(hdc, ramp.as_ptr().cast());
        let err = GetLastError();
        let _ = DeleteDC(hdc);
        if !ok.as_bool() {
            anyhow::bail!("SetDeviceGammaRamp falhou (Win32 {:#x})", err.0);
        }
        Ok(())
    }
}

fn ramps_match(a: &GammaRamp, b: &GammaRamp) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(ac, bc)| ac.iter().zip(bc.iter()).all(|(av, bv)| av.abs_diff(*bv) <= 2))
}
