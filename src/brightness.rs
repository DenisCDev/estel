//! Physical brightness chosen independently for each output: DDC/CI for
//! external monitors and native WMI for supported built-in panels.

use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitor, GetMonitorBrightness, GetMonitorCapabilities, GetMonitorTechnologyType,
    GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    MC_CAPS_MONITOR_TECHNOLOGY_TYPE, MC_DISPLAY_TECHNOLOGY_TYPE, MC_ORGANIC_LIGHT_EMITTING_DIODE,
    MC_THIN_FILM_TRANSISTOR, PHYSICAL_MONITOR, SetMonitorBrightness,
};
use windows::Win32::Foundation::{GetLastError, HANDLE};
use windows::Win32::Graphics::Gdi::HMONITOR;

use crate::display_topology::Output;
use crate::hardware_wmi::{self, Panel, Service};
use crate::session::{self, DdcOriginal, DdcSnapshot};

enum Backend {
    Ddc { handle: usize, min: u32, max: u32 },
    Wmi(Panel),
}

struct Monitor {
    output: Output,
    original: u32,
    last: Option<u32>,
    enabled: bool,
    technology: Option<String>,
    backend: Backend,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        if let Backend::Ddc { handle, .. } = self.backend {
            if let Err(error) = unsafe { DestroyPhysicalMonitor(raw_handle(handle)) } {
                tracing::debug!(%error, "identificador físico já indisponível");
            }
        }
    }
}

pub struct Controller {
    monitors: Vec<Monitor>,
    originals: Vec<DdcOriginal>,
    wmi: Option<Service>,
    recovery_valid: bool,
}

impl Default for Controller {
    fn default() -> Self {
        let (originals, recovery_valid) = match session::load_ddc_originals() {
            DdcSnapshot::Named(values) => (values, true),
            DdcSnapshot::Missing => (Vec::new(), true),
            DdcSnapshot::Invalid | DdcSnapshot::Legacy(_) => {
                tracing::warn!(
                    "recuperação de brilho sem identidades válidas; ajuste físico preservado"
                );
                (Vec::new(), false)
            }
        };
        Self {
            monitors: Vec::new(),
            originals,
            wmi: None,
            recovery_valid,
        }
    }
}

impl Controller {
    pub fn refresh(&mut self, outputs: &[Output]) {
        self.monitors.clear();
        self.wmi = None;
        if !self.recovery_valid {
            return;
        }
        let mut panels = Vec::new();
        if outputs
            .iter()
            .any(|output| output.internal && !output.id.is_empty())
        {
            match Service::connect()
                .and_then(|service| service.panels().map(|panels| (service, panels)))
            {
                Ok((service, found)) => {
                    self.wmi = Some(service);
                    panels = found;
                }
                Err(error) => tracing::debug!(%error, "controle de brilho interno indisponível"),
            }
        }
        for output in outputs
            .iter()
            .filter(|output| !output.id.is_empty() && !output.cloned)
        {
            let panel = panels
                .iter()
                .position(|panel| hardware_wmi::matches_instance(&output.id, &panel.instance));
            let monitor = if output.internal {
                panel.map(|index| {
                    let panel = panels.remove(index);
                    Monitor {
                        output: output.clone(),
                        original: panel.current,
                        last: None,
                        enabled: true,
                        technology: None,
                        backend: Backend::Wmi(panel),
                    }
                })
            } else {
                match open_ddc(output) {
                    Ok(monitor) => monitor,
                    Err(error) => {
                        tracing::debug!(id = %output.id, %error, "DDC indisponível nesta tela");
                        None
                    }
                }
            };
            let Some(mut monitor) = monitor else {
                continue;
            };
            if let Some(saved) = self.originals.iter().find(|saved| saved.id == output.id) {
                if !valid_original(&monitor.backend, saved.value) {
                    tracing::warn!(id = %output.id, "brilho salvo fora do intervalo do monitor");
                    continue;
                }
                monitor.original = saved.value;
                if let Err(error) = set_value(&monitor, self.wmi.as_ref(), saved.value) {
                    tracing::warn!(id = %output.id, %error, "recuperação de brilho pendente");
                    continue;
                }
                self.originals.retain(|saved| saved.id != output.id);
                if !session::save_ddc_originals(&self.originals) {
                    self.recovery_valid = false;
                    return;
                }
            }
            self.monitors.push(monitor);
        }
    }

    pub fn apply(&mut self, brightness: f32) {
        if !self.recovery_valid {
            return;
        }
        for monitor in self.monitors.iter_mut().filter(|monitor| monitor.enabled) {
            let value = match &monitor.backend {
                Backend::Ddc { min, max, .. } => brightness_value(brightness, *min, *max),
                Backend::Wmi(panel) => {
                    hardware_wmi::nearest_level(&panel.levels, brightness_value(brightness, 0, 100))
                }
            };
            if monitor.last == Some(value) {
                continue;
            }
            if !self
                .originals
                .iter()
                .any(|saved| saved.id == monitor.output.id)
            {
                self.originals.push(DdcOriginal {
                    id: monitor.output.id.clone(),
                    value: monitor.original,
                });
                if !session::save_ddc_originals(&self.originals) || !session::mark_dirty() {
                    self.recovery_valid = false;
                    return;
                }
            }
            match set_value(monitor, self.wmi.as_ref(), value) {
                Ok(()) => monitor.last = Some(value),
                Err(error) => {
                    monitor.enabled = false;
                    monitor.last = None;
                    tracing::warn!(id = %monitor.output.id, %error, "controle físico indisponível nesta tela");
                    if let Err(error) = set_value(monitor, self.wmi.as_ref(), monitor.original) {
                        tracing::warn!(id = %monitor.output.id, %error, "restauração de brilho pendente");
                    }
                }
            }
        }
    }

    pub fn active_for(&self, name: &str) -> bool {
        self.monitors
            .iter()
            .any(|monitor| monitor.output.name == name && monitor.enabled && monitor.last.is_some())
    }

    pub fn technology_for(&self, name: &str) -> Option<String> {
        self.monitors
            .iter()
            .find(|monitor| monitor.output.name == name)
            .and_then(|monitor| monitor.technology.clone())
    }

    pub fn method_for(&self, name: &str) -> Option<String> {
        self.monitors
            .iter()
            .find(|monitor| monitor.output.name == name && monitor.enabled)
            .map(|monitor| match monitor.backend {
                Backend::Ddc { .. } => "DDC/CI".into(),
                Backend::Wmi(_) => "WMI".into(),
            })
    }

    pub fn warning(&self) -> Option<String> {
        (!self.recovery_valid).then(|| "O registro do brilho original está inválido, é de uma versão antiga sem identidades ou não pôde ser salvo. O ajuste físico foi interrompido.".into())
    }

    pub fn park(&mut self) -> bool {
        if !self.recovery_valid {
            return false;
        }
        for monitor in &mut self.monitors {
            if !self
                .originals
                .iter()
                .any(|saved| saved.id == monitor.output.id)
            {
                continue;
            }
            match set_value(monitor, self.wmi.as_ref(), monitor.original) {
                Ok(()) => {
                    self.originals.retain(|saved| saved.id != monitor.output.id);
                    monitor.last = None;
                }
                Err(error) => {
                    tracing::warn!(id = %monitor.output.id, %error, "não foi possível restaurar o brilho")
                }
            }
        }
        session::save_ddc_originals(&self.originals) && self.originals.is_empty()
    }
}

fn valid_original(backend: &Backend, value: u32) -> bool {
    match backend {
        Backend::Ddc { min, max, .. } => (*min..=*max).contains(&value),
        Backend::Wmi(_) => value <= 100,
    }
}

fn brightness_value(brightness: f32, min: u32, max: u32) -> u32 {
    let percent = (brightness.clamp(0.0, 1.0) * 50.0).round() / 50.0;
    min + (percent * max.saturating_sub(min) as f32).round() as u32
}

fn set_value(monitor: &Monitor, wmi: Option<&Service>, value: u32) -> anyhow::Result<()> {
    match &monitor.backend {
        Backend::Ddc { handle, .. } => {
            if unsafe { SetMonitorBrightness(raw_handle(*handle), value) } == 0 {
                anyhow::bail!("SetMonitorBrightness: {:?}", unsafe { GetLastError() });
            }
            Ok(())
        }
        Backend::Wmi(panel) => wmi
            .ok_or_else(|| anyhow::anyhow!("controle WMI indisponível"))?
            .set(panel, value),
    }
}

fn raw_handle(value: usize) -> HANDLE {
    HANDLE(value as *mut core::ffi::c_void)
}

pub struct Capabilities {
    pub name: String,
    pub method: String,
    pub technology: Option<String>,
}

/// Discovery only: no snapshot recovery and no write to a physical monitor.
pub fn inspect(outputs: &[Output]) -> Vec<Capabilities> {
    let mut found = Vec::new();
    if outputs.iter().any(|output| output.internal) {
        match Service::connect().and_then(|service| service.panels()) {
            Ok(panels) => {
                for output in outputs
                    .iter()
                    .filter(|output| output.internal && !output.cloned)
                {
                    if panels
                        .iter()
                        .filter(|panel| hardware_wmi::matches_instance(&output.id, &panel.instance))
                        .count()
                        == 1
                    {
                        found.push(Capabilities {
                            name: output.name.clone(),
                            method: "WMI".into(),
                            technology: None,
                        });
                    }
                }
            }
            Err(error) => tracing::debug!(%error, "capacidade de brilho interno não identificada"),
        }
    }
    for output in outputs
        .iter()
        .filter(|output| !output.internal && !output.id.is_empty() && !output.cloned)
    {
        match open_ddc(output) {
            Ok(Some(monitor)) => found.push(Capabilities {
                name: output.name.clone(),
                method: "DDC/CI".into(),
                technology: monitor.technology.clone(),
            }),
            Ok(None) => {}
            Err(error) => tracing::debug!(%error, "capacidade DDC não identificada"),
        }
    }
    found
}

fn open_ddc(output: &Output) -> anyhow::Result<Option<Monitor>> {
    let mut count = 0;
    unsafe {
        GetNumberOfPhysicalMonitorsFromHMONITOR(
            HMONITOR(output.monitor as *mut core::ffi::c_void),
            &mut count,
        )?;
        if count == 0 {
            return Ok(None);
        }
        anyhow::ensure!(count <= 8, "quantidade de monitores físicos inválida");
        let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
        GetPhysicalMonitorsFromHMONITOR(
            HMONITOR(output.monitor as *mut core::ffi::c_void),
            &mut physical,
        )?;
        if count != 1 {
            for item in physical {
                let _ = DestroyPhysicalMonitor(item.hPhysicalMonitor);
            }
            return Ok(None);
        }
        let handle = physical[0].hPhysicalMonitor;
        let (mut min, mut original, mut max) = (0, 0, 0);
        if GetMonitorBrightness(handle, &mut min, &mut original, &mut max) == 0
            || max <= min
            || !(min..=max).contains(&original)
        {
            let _ = DestroyPhysicalMonitor(handle);
            return Ok(None);
        }
        let (mut caps, mut temperatures) = (0, 0);
        let mut technology = None;
        if GetMonitorCapabilities(handle, &mut caps, &mut temperatures) != 0
            && caps & MC_CAPS_MONITOR_TECHNOLOGY_TYPE != 0
        {
            let mut kind = MC_DISPLAY_TECHNOLOGY_TYPE::default();
            if GetMonitorTechnologyType(handle, &mut kind) != 0 {
                technology = Some(match kind {
                    MC_ORGANIC_LIGHT_EMITTING_DIODE => "OLED".into(),
                    MC_THIN_FILM_TRANSISTOR => "LCD TFT".into(),
                    _ => format!("Tecnologia informada: {}", kind.0),
                });
            }
        }
        Ok(Some(Monitor {
            output: output.clone(),
            original,
            last: None,
            enabled: true,
            technology,
            backend: Backend::Ddc {
                handle: handle.0 as usize,
                min,
                max,
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_monitor_ranges_produce_correct_brightness() {
        assert_eq!(brightness_value(0.5, 0, 100), 50);
        assert_eq!(brightness_value(0.5, 20, 80), 50);
        assert_eq!(brightness_value(0.0, 20, 80), 20);
        assert_eq!(brightness_value(1.0, 20, 80), 80);
    }

    #[test]
    fn nearby_targets_do_not_churn_ddc() {
        assert_eq!(
            brightness_value(0.500, 0, 100),
            brightness_value(0.505, 0, 100)
        );
    }

    #[test]
    fn invalid_saved_brightness_is_not_clamped_and_written() {
        let backend = Backend::Ddc {
            handle: 0,
            min: 20,
            max: 80,
        };
        assert!(!valid_original(&backend, 10));
        assert!(valid_original(&backend, 40));
    }
}
