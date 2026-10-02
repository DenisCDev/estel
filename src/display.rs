//! Per-output SDR gamma control. HDR, automatic color management and unknown
//! pipelines are excluded before touching the driver's legacy lookup table.

use windows::Win32::Foundation::GetLastError;
use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, HDC};
use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};
use windows::core::{PCWSTR, w};

use crate::color::{GammaRamp, cct_to_rgb, clamp_ramp_to_driver};
use crate::display_topology::Output;
use crate::session::{self, GammaOriginal};

struct Head {
    output: Output,
    original: GammaRamp,
    last: Option<GammaRamp>,
    cct: Option<f32>,
    enabled: bool,
}

pub struct Controller {
    heads: Vec<Head>,
    originals: Vec<GammaOriginal>,
    recovery_valid: bool,
}

impl Default for Controller {
    fn default() -> Self {
        if session::legacy_gamma_missing() {
            tracing::error!(
                "a sessão antiga não salvou a calibração original; recuperação automática da gama indisponível"
            );
            return Self {
                heads: Vec::new(),
                originals: Vec::new(),
                recovery_valid: false,
            };
        }
        match session::load_gamma_originals() {
            Ok(originals) => Self {
                heads: Vec::new(),
                originals,
                recovery_valid: true,
            },
            Err(error) => {
                tracing::error!(%error, "registro de gama inválido; ajustes de cor preservados");
                Self {
                    heads: Vec::new(),
                    originals: Vec::new(),
                    recovery_valid: false,
                }
            }
        }
    }
}

impl Controller {
    pub fn refresh(&mut self, outputs: &[Output]) {
        self.heads.clear();
        if !self.recovery_valid {
            return;
        }
        for output in outputs.iter().filter(|output| output.gamma_safe()) {
            let saved = self
                .originals
                .iter()
                .find(|record| record.id == output.id)
                .and_then(GammaOriginal::ramp);
            if let Some(original) = saved {
                if let Err(error) = write_named(&output.name, &original) {
                    tracing::warn!(id = %output.id, %error, "recuperação da gama pendente");
                    continue;
                }
                self.originals.retain(|record| record.id != output.id);
                if !session::save_gamma_originals(&self.originals) {
                    self.recovery_valid = false;
                    return;
                }
            }
            match read_named(&output.name) {
                Ok(original) => self.heads.push(Head {
                    output: output.clone(),
                    original,
                    last: None,
                    cct: None,
                    enabled: true,
                }),
                Err(error) => {
                    tracing::debug!(id = %output.id, %error, "gama indisponível nesta tela")
                }
            }
        }
    }

    pub fn apply(&mut self, cct: f32, floor: f32) {
        if !self.recovery_valid {
            return;
        }
        for head in self.heads.iter_mut().filter(|head| head.enabled) {
            let ramp = calibrated_ramp(&head.original, cct.max(floor));
            if head.last.as_ref() == Some(&ramp) {
                continue;
            }
            if head.last.is_none() && ramp == head.original {
                head.last = Some(ramp);
                head.cct = Some(cct.max(floor));
                continue;
            }
            if !self
                .originals
                .iter()
                .any(|record| record.id == head.output.id)
            {
                self.originals
                    .push(GammaOriginal::new(head.output.id.clone(), &head.original));
                if !session::save_gamma_originals(&self.originals) || !session::mark_dirty() {
                    self.recovery_valid = false;
                    return;
                }
            }
            let result = write_named(&head.output.name, &ramp);
            match result {
                Ok(()) => {
                    head.last = Some(ramp);
                    head.cct = Some(cct.max(floor));
                }
                Err(error) => {
                    head.enabled = false;
                    head.last = None;
                    head.cct = None;
                    tracing::warn!(id = %head.output.id, %error, "ajuste de cor indisponível nesta tela");
                    if let Err(error) = write_named(&head.output.name, &head.original) {
                        tracing::error!(id = %head.output.id, %error, "restauração da gama pendente");
                    }
                }
            }
        }
    }

    pub fn active_for(&self, name: &str) -> bool {
        self.heads
            .iter()
            .any(|head| head.output.name == name && head.enabled && head.last.is_some())
    }

    pub fn cct_for(&self, name: &str) -> Option<f32> {
        self.heads
            .iter()
            .find(|head| head.output.name == name && head.enabled)
            .and_then(|head| head.cct)
    }

    pub fn warning(&self) -> Option<String> {
        if self.recovery_valid {
            return None;
        }
        Some(if session::legacy_gamma_missing() {
            "A versão anterior não salvou a calibração original. A recuperação automática da cor não pode ser confirmada; o ajuste global permanece desativado."
        } else {
            "O registro da calibração está inválido ou não pôde ser salvo. O ajuste global de cor foi interrompido para preservar a tela."
        }.into())
    }

    pub fn park(&mut self) -> bool {
        self.park_with(write_named, session::save_gamma_originals)
    }

    fn park_with(
        &mut self,
        mut restore: impl FnMut(&str, &GammaRamp) -> anyhow::Result<()>,
        save: impl FnOnce(&[GammaOriginal]) -> bool,
    ) -> bool {
        if !self.recovery_valid {
            return false;
        }
        for head in &mut self.heads {
            head.last = None;
            head.cct = None;
            if !self
                .originals
                .iter()
                .any(|record| record.id == head.output.id)
            {
                continue;
            }
            match restore(&head.output.name, &head.original) {
                Ok(()) => {
                    self.originals.retain(|record| record.id != head.output.id);
                    head.last = None;
                    head.cct = None;
                }
                Err(error) => {
                    tracing::error!(id = %head.output.id, %error, "não foi possível restaurar a gama")
                }
            }
        }
        save(&self.originals) && self.originals.is_empty()
    }
}

fn calibrated_ramp(original: &GammaRamp, cct: f32) -> GammaRamp {
    if cct >= 6500.0 {
        return *original;
    }
    let scale = cct_to_rgb(cct);
    let mut ramp = *original;
    for (channel, factor) in ramp.iter_mut().zip(scale) {
        for value in channel {
            *value = (*value as f32 * factor).round() as u16;
        }
    }
    let mut safe = clamp_ramp_to_driver(ramp);
    for (value, original) in safe.iter_mut().flatten().zip(original.iter().flatten()) {
        *value = (*value).min(*original);
    }
    safe
}

fn ramp_matches(requested: &GammaRamp, actual: &GammaRamp) -> bool {
    requested
        .iter()
        .flatten()
        .zip(actual.iter().flatten())
        .all(|(a, b)| a.abs_diff(*b) <= 256)
}

struct DeviceContext(HDC);

impl DeviceContext {
    fn open(name: &str) -> anyhow::Result<Self> {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let hdc = unsafe { CreateDCW(w!("DISPLAY"), PCWSTR(name.as_ptr()), PCWSTR::null(), None) };
        anyhow::ensure!(!hdc.0.is_null(), "CreateDCW retornou nulo");
        Ok(Self(hdc))
    }
}

impl Drop for DeviceContext {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

fn read_named(name: &str) -> anyhow::Result<GammaRamp> {
    let context = DeviceContext::open(name)?;
    let mut ramp = [[0; 256]; 3];
    if !unsafe { GetDeviceGammaRamp(context.0, ramp.as_mut_ptr().cast()) }.as_bool() {
        anyhow::bail!("GetDeviceGammaRamp: {:?}", unsafe { GetLastError() });
    }
    Ok(ramp)
}

fn write_named(name: &str, ramp: &GammaRamp) -> anyhow::Result<()> {
    let context = DeviceContext::open(name)?;
    checked_write(
        ramp,
        || {
            if !unsafe { SetDeviceGammaRamp(context.0, ramp.as_ptr().cast()) }.as_bool() {
                anyhow::bail!("SetDeviceGammaRamp: {:?}", unsafe { GetLastError() });
            }
            Ok(())
        },
        || read_named(name),
    )
}

fn checked_write(
    requested: &GammaRamp,
    write: impl FnOnce() -> anyhow::Result<()>,
    read: impl FnOnce() -> anyhow::Result<GammaRamp>,
) -> anyhow::Result<()> {
    write()?;
    anyhow::ensure!(
        ramp_matches(requested, &read()?),
        "driver não confirmou a rampa solicitada"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warming_attenuates_channels_without_lifting_black() {
        let original = crate::color::identity_ramp();
        let warm = calibrated_ramp(&original, 3400.0);
        for channel in &warm {
            assert_eq!(channel[0], 0);
        }
        assert!(warm[2][200] < original[2][200]);
        assert!(
            warm.iter()
                .flatten()
                .zip(original.iter().flatten())
                .all(|(a, b)| a <= b)
        );
    }

    #[test]
    fn neutral_keeps_existing_calibration() {
        let mut original = crate::color::identity_ramp();
        original[1][200] -= 1234;
        assert_eq!(calibrated_ramp(&original, 6500.0), original);
    }

    #[test]
    fn silently_rejected_gamma_is_not_reported_as_active() {
        let original = crate::color::identity_ramp();
        assert!(!ramp_matches(
            &calibrated_ramp(&original, 3400.0),
            &original
        ));
        assert!(ramp_matches(&original, &original));
    }

    #[test]
    fn rejected_restoration_keeps_original_until_readback_confirms_it() {
        let original = crate::color::identity_ramp();
        let altered = calibrated_ramp(&original, 3400.0);
        let output = Output {
            id: "display-a".into(),
            name: "screen-a".into(),
            label: String::new(),
            rect: [0, 0, 1920, 1080],
            color_mode: crate::display_topology::ColorMode::Sdr,
            internal: false,
            cloned: false,
            monitor: 0,
        };
        let mut controller = Controller {
            heads: vec![Head {
                output: output.clone(),
                original,
                last: Some(altered),
                cct: Some(3400.0),
                enabled: true,
            }],
            originals: vec![GammaOriginal::new(output.id, &original)],
            recovery_valid: true,
        };
        assert!(!controller.park_with(
            |_, requested| checked_write(requested, || Ok(()), || Ok(altered)),
            |saved| {
                assert_eq!(saved[0].ramp(), Some(original));
                true
            },
        ));
        assert_eq!(controller.originals.len(), 1);
        assert!(controller.park_with(
            |_, requested| checked_write(requested, || Ok(()), || Ok(original)),
            |saved| {
                assert!(saved.is_empty());
                true
            },
        ));
    }
}
