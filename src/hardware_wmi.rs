//! Native WMI brightness for built-in panels. COM stays inside the display
//! worker, whose parent enforces the deadline even if a provider never returns.

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoSetProxyBlanket, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Ole::{
    SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
};
use windows::Win32::System::Variant::{VARIANT, VT_ARRAY, VT_UI1};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WbemLocator,
};
use windows::core::{BSTR, PCWSTR, w};

pub struct Panel {
    pub instance: String,
    pub current: u32,
    pub levels: Vec<u32>,
    path: String,
}

pub struct Service {
    service: IWbemServices,
    parameters: IWbemClassObject,
}

impl Service {
    pub fn connect() -> anyhow::Result<Self> {
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let empty = BSTR::new();
            let service = locator.ConnectServer(
                &BSTR::from("ROOT\\WMI"),
                &empty,
                &empty,
                &empty,
                0x80,
                &empty,
                None,
            )?;
            CoSetProxyBlanket(
                &service,
                10,
                0,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )?;
            let mut class = None;
            service.GetObject(
                &BSTR::from("WmiMonitorBrightnessMethods"),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                Some(&mut class),
                None,
            )?;
            let class = class.ok_or_else(|| anyhow::anyhow!("classe de brilho indisponível"))?;
            let mut input = None;
            class.GetMethod(w!("WmiSetBrightness"), 0, &mut input, std::ptr::null_mut())?;
            let parameters =
                input.ok_or_else(|| anyhow::anyhow!("método de brilho indisponível"))?;
            Ok(Self {
                service,
                parameters,
            })
        }
    }

    fn query(&self, query: &str) -> anyhow::Result<Vec<IWbemClassObject>> {
        let enumerator = unsafe {
            self.service.ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )?
        };
        let mut objects = Vec::new();
        for _ in 0..8 {
            let mut slot = [None];
            let mut count = 0;
            let status = unsafe { enumerator.Next(500, &mut slot, &mut count) };
            status.ok()?;
            if count == 0 {
                anyhow::ensure!(status.0 != 0x40004, "consulta de brilho excedeu o prazo");
                return Ok(objects);
            }
            if let Some(object) = slot[0].take() {
                objects.push(object);
            }
        }
        anyhow::bail!("quantidade inesperada de painéis internos")
    }

    pub fn panels(&self) -> anyhow::Result<Vec<Panel>> {
        let values = self.query("SELECT InstanceName, CurrentBrightness, Level FROM WmiMonitorBrightness WHERE Active = TRUE")?;
        let methods = self.query(
            "SELECT InstanceName, __PATH FROM WmiMonitorBrightnessMethods WHERE Active = TRUE",
        )?;
        let mut panels = Vec::new();
        for object in values {
            let instance = string_property(&object, w!("InstanceName"))?;
            let current = u32::try_from(&property(&object, w!("CurrentBrightness"))?)?;
            anyhow::ensure!(current <= 100, "brilho interno inválido");
            let mut paths = methods.iter().filter_map(|method| {
                match string_property(method, w!("InstanceName")) {
                    Ok(name) if name.eq_ignore_ascii_case(&instance) => {
                        Some(string_property(method, w!("__PATH")))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                }
            });
            let Some(path) = paths.next() else {
                continue;
            };
            let path = path?;
            if paths.next().is_some() {
                continue;
            }
            panels.push(Panel {
                instance,
                current,
                levels: levels(&object)?,
                path,
            });
        }
        Ok(panels)
    }

    pub fn set(&self, panel: &Panel, value: u32) -> anyhow::Result<()> {
        anyhow::ensure!(value <= 100, "brilho interno fora do intervalo");
        unsafe {
            let input = self.parameters.SpawnInstance(0)?;
            input.Put(w!("Timeout"), 0, &VARIANT::from("0"), 0)?;
            input.Put(w!("Brightness"), 0, &VARIANT::from(value as u8), 0)?;
            let mut result = None;
            self.service.ExecMethod(
                &BSTR::from(panel.path.as_str()),
                &BSTR::from("WmiSetBrightness"),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                &input,
                Some(&mut result),
                None,
            )?;
            let result = result.ok_or_else(|| anyhow::anyhow!("resposta de brilho ausente"))?;
            let code = u32::try_from(&property(&result, w!("ReturnValue"))?)?;
            anyhow::ensure!(code == 0, "o painel recusou o brilho (código {code})");
        }
        Ok(())
    }
}

fn property(object: &IWbemClassObject, name: PCWSTR) -> anyhow::Result<VARIANT> {
    let mut value = VARIANT::default();
    unsafe {
        object.Get(name, 0, &mut value, None, None)?;
    }
    Ok(value)
}

fn string_property(object: &IWbemClassObject, name: PCWSTR) -> anyhow::Result<String> {
    Ok(BSTR::try_from(&property(object, name)?)?.to_string())
}

fn levels(object: &IWbemClassObject) -> anyhow::Result<Vec<u32>> {
    let value = property(object, w!("Level"))?;
    unsafe {
        let raw = &value.Anonymous.Anonymous;
        anyhow::ensure!(raw.vt == (VT_ARRAY | VT_UI1), "níveis de brilho inválidos");
        let array = raw.Anonymous.parray;
        anyhow::ensure!(!array.is_null(), "níveis de brilho ausentes");
        anyhow::ensure!(
            SafeArrayGetDim(array) == 1,
            "dimensões dos níveis de brilho inválidas"
        );
        let low = SafeArrayGetLBound(array, 1)?;
        let high = SafeArrayGetUBound(array, 1)?;
        anyhow::ensure!(
            high >= low && high - low < 101,
            "quantidade de níveis de brilho inválida"
        );
        let mut levels = Vec::new();
        for index in low..=high {
            let mut level = 0u8;
            SafeArrayGetElement(array, &index, (&mut level as *mut u8).cast())?;
            anyhow::ensure!(level <= 100, "nível de brilho fora do intervalo");
            levels.push(level as u32);
        }
        levels.sort_unstable();
        levels.dedup();
        Ok(levels)
    }
}

pub fn matches_instance(device_path: &str, instance: &str) -> bool {
    let path = device_path.to_ascii_lowercase();
    let path = path
        .trim_start_matches("\\\\?\\")
        .split('#')
        .take(3)
        .collect::<Vec<_>>()
        .join("\\");
    let instance = instance.to_ascii_lowercase();
    let instance = instance
        .rsplit_once('_')
        .filter(|(_, suffix)| suffix.parse::<u32>().is_ok())
        .map_or(instance.as_str(), |(name, _)| name);
    path == instance
}

pub fn nearest_level(levels: &[u32], requested: u32) -> u32 {
    levels
        .iter()
        .copied()
        .min_by_key(|level| level.abs_diff(requested))
        .unwrap_or(requested)
        .min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_panel_instance_without_guessing_model_names() {
        assert!(matches_instance(
            r"\\?\DISPLAY#ABC123#4&abc&0&UID123#{guid}",
            r"DISPLAY\ABC123\4&abc&0&UID123_0"
        ));
        assert!(!matches_instance(
            r"\\?\DISPLAY#ABC123#4&abc&0&UID123#{guid}",
            r"DISPLAY\ABC123\4&abc&0&UID124_0"
        ));
    }

    #[test]
    fn respects_driver_brightness_steps() {
        assert_eq!(nearest_level(&[0, 20, 40, 60, 80, 100], 47), 40);
        assert_eq!(nearest_level(&[10, 50, 100], 0), 10);
    }
}
