//! Read-only display discovery. Driver operations belong in the isolated worker.

use serde::{Deserialize, Serialize};
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::core::BOOL;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum ColorMode {
    Sdr,
    Hdr,
    WideColor,
    AdvancedUnknown,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Output {
    pub id: String,
    pub name: String,
    pub label: String,
    pub rect: [i32; 4],
    pub color_mode: ColorMode,
    pub internal: bool,
    pub cloned: bool,
    #[serde(skip)]
    pub monitor: usize,
}

impl Output {
    pub fn gamma_safe(&self) -> bool {
        self.color_mode == ColorMode::Sdr && !self.cloned && !self.id.is_empty()
    }
}

// The SDK 10.0.26100 wingdi.h exposes INFO_2, but windows 0.62 does not project
// it yet. The ABI is fixed; older Windows versions reject packet type 15.
#[repr(C)]
#[derive(Default)]
struct AdvancedColorInfo2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    flags: u32,
    color_encoding: u32,
    bits_per_channel: u32,
    active_color_mode: u32,
}

pub fn enumerate() -> anyhow::Result<Vec<Output>> {
    let paths = active_paths()?;
    let mut outputs = Vec::new();
    let monitors = logical_monitors();
    for (monitor, name, rect) in monitors {
        let matching = paths
            .iter()
            .filter(|path| source_name(path).as_deref() == Some(name.as_str()))
            .collect::<Vec<_>>();
        let mut output = Output {
            id: String::new(),
            name,
            label: String::new(),
            rect,
            color_mode: ColorMode::Unknown,
            internal: false,
            cloned: matching.len() != 1,
            monitor,
        };
        if let [path] = matching.as_slice() {
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                header: header(
                    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(),
                    path,
                ),
                ..Default::default()
            };
            if unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } == 0 {
                output.id = wide_string(&target.monitorDevicePath).to_ascii_lowercase();
                output.label = wide_string(&target.monitorFriendlyDeviceName);
                output.internal = matches!(
                    target.outputTechnology,
                    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
                        | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
                        | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED
                        | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS
                );
            }
            output.color_mode = color_mode(path);
        }
        outputs.push(output);
    }
    outputs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(outputs)
}

fn header(
    kind: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    size: usize,
    path: &DISPLAYCONFIG_PATH_INFO,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: kind,
        size: size as u32,
        adapterId: path.targetInfo.adapterId,
        id: path.targetInfo.id,
    }
}

fn color_mode(path: &DISPLAYCONFIG_PATH_INFO) -> ColorMode {
    let mut modern = AdvancedColorInfo2 {
        header: header(
            DISPLAYCONFIG_DEVICE_INFO_TYPE(15),
            std::mem::size_of::<AdvancedColorInfo2>(),
            path,
        ),
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut modern.header) } == 0 {
        return match modern.active_color_mode {
            0 => ColorMode::Sdr,
            1 => ColorMode::WideColor,
            2 => ColorMode::Hdr,
            _ => ColorMode::Unknown,
        };
    }
    let mut legacy = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: header(
            DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(),
            path,
        ),
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut legacy.header) } == 0 {
        // The old API combines HDR and automatic color management. Do not
        // mislabel either, or send a legacy gamma ramp into either pipeline.
        if unsafe { legacy.Anonymous.value } & 0b110 == 0 {
            ColorMode::Sdr
        } else {
            ColorMode::AdvancedUnknown
        }
    } else {
        ColorMode::Unknown
    }
}

fn active_paths() -> anyhow::Result<Vec<DISPLAYCONFIG_PATH_INFO>> {
    for _ in 0..3 {
        let (mut path_count, mut mode_count) = (0, 0);
        unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        }
        .ok()?;
        anyhow::ensure!(
            path_count <= 64 && mode_count <= 256,
            "quantidade de telas inválida"
        );
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        let result = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            )
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        result.ok()?;
        paths.truncate(path_count as usize);
        return Ok(paths);
    }
    anyhow::bail!("as telas mudaram durante a descoberta")
}

fn source_name(path: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
    let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
            size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
            adapterId: path.sourceInfo.adapterId,
            id: path.sourceInfo.id,
        },
        ..Default::default()
    };
    (unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } == 0)
        .then(|| wide_string(&source.viewGdiDeviceName))
}

pub fn wide_string(value: &[u16]) -> String {
    String::from_utf16_lossy(
        &value[..value
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(value.len())],
    )
}

pub fn logical_monitors() -> Vec<(usize, String, [i32; 4])> {
    let mut monitors = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(on_monitor),
            LPARAM(&mut monitors as *mut Vec<(usize, String, [i32; 4])> as isize),
        );
    }
    monitors
}

unsafe extern "system" fn on_monitor(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if unsafe {
        GetMonitorInfoW(
            monitor,
            (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
        )
    }
    .as_bool()
    {
        let rect = info.monitorInfo.rcMonitor;
        unsafe { &mut *(data.0 as *mut Vec<(usize, String, [i32; 4])>) }.push((
            monitor.0 as usize,
            wide_string(&info.szDevice),
            [rect.left, rect.top, rect.right, rect.bottom],
        ));
    }
    BOOL(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_acm_and_unknown_modes_never_use_legacy_gamma() {
        let mut output = Output {
            id: "display-a".into(),
            name: "screen".into(),
            label: String::new(),
            rect: [0, 0, 1920, 1080],
            color_mode: ColorMode::Sdr,
            internal: false,
            cloned: false,
            monitor: 0,
        };
        assert!(output.gamma_safe());
        for mode in [
            ColorMode::Hdr,
            ColorMode::WideColor,
            ColorMode::AdvancedUnknown,
            ColorMode::Unknown,
        ] {
            output.color_mode = mode;
            assert!(!output.gamma_safe());
        }
        output.color_mode = ColorMode::Sdr;
        output.cloned = true;
        assert!(!output.gamma_safe());
    }
}
