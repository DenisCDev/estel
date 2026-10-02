//! Per-output overlays with separate neutral dimming and approximate warmth.
//!
//! Alpha blending cannot multiply RGB channels independently. The black layer
//! only attenuates; the optional warm layer is an approximation that can lift
//! shadows. SDR gamma provides the channel-attenuating part where supported.

use std::cell::RefCell;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HBRUSH, HGDIOBJ, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetWindowLongPtrW, HCURSOR, HICON, HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE, PeekMessageW,
    RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetLayeredWindowAttributes,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WM_DESTROY, WM_PAINT, WM_QUIT,
    WNDCLASS_STYLES, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::hardware_worker::OutputStatus;

struct Layers {
    name: String,
    dim: HWND,
    warm: HWND,
}

impl Drop for Layers {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.dim);
            let _ = DestroyWindow(self.warm);
        }
    }
}

thread_local! {
    static LAYERS: RefCell<Vec<Layers>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                let color = if GetWindowLongPtrW(hwnd, GWLP_USERDATA) == 1 {
                    0x00_B4_D2_FF
                } else {
                    0
                };
                let brush = CreateSolidBrush(COLORREF(color));
                FillRect(dc, &paint.rcPaint, brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Create a hidden dispatcher on the UI thread. All overlay operations must
/// remain on this thread so Windows can dispatch their messages while idle.
pub fn create() -> anyhow::Result<HWND> {
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: WNDCLASS_STYLES(0),
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE(module.0),
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            lpszClassName: w!("EstelOverlay"),
            ..Default::default()
        };
        let _ = RegisterClassExW(&class);
    }
    create_layer([0; 4], false)
}

fn create_layer(rect: [i32; 4], warm: bool) -> anyhow::Result<HWND> {
    unsafe {
        let module = GetModuleHandleW(None)?;
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("EstelOverlay"),
            PCWSTR::null(),
            WS_POPUP,
            rect[0],
            rect[1],
            rect[2] - rect[0],
            rect[3] - rect[1],
            None,
            None,
            Some(HINSTANCE(module.0)),
            None,
        )?;
        SetWindowLongPtrW(window, GWLP_USERDATA, isize::from(warm));
        SetLayeredWindowAttributes(window, COLORREF(0), 0, LWA_ALPHA)?;
        Ok(window)
    }
}

pub fn update_outputs(_dispatcher: HWND, cct: f32, brightness: f32, outputs: &[OutputStatus]) {
    LAYERS.with(|layers| {
        let mut layers = layers.borrow_mut();
        layers.retain(|layer| outputs.iter().any(|output| output.name == layer.name));
        for output in outputs {
            if !layers.iter().any(|layer| layer.name == output.name) {
                match create_layers(output) {
                    Ok(layer) => layers.push(layer),
                    Err(error) => {
                        tracing::error!(%error, "não foi possível criar a sobreposição da tela");
                        continue;
                    }
                }
            }
            let Some(layer) = layers.iter().find(|layer| layer.name == output.name) else {
                continue;
            };
            let (warm, dim) =
                layer_alphas(cct, brightness, output.brightness_active, output.gamma_cct);
            // Dimming stays above warmth so it attenuates both the desktop and
            // the warm approximation; lowering brightness never raises black.
            set_layer(layer.warm, output.rect, warm);
            set_layer(layer.dim, output.rect, dim);
        }
    });
}

fn create_layers(output: &OutputStatus) -> anyhow::Result<Layers> {
    let dim = create_layer(output.rect, false)?;
    match create_layer(output.rect, true) {
        Ok(warm) => Ok(Layers {
            name: output.name.clone(),
            dim,
            warm,
        }),
        Err(error) => {
            unsafe {
                let _ = DestroyWindow(dim);
            }
            Err(error)
        }
    }
}

fn set_layer(window: HWND, rect: [i32; 4], alpha: u8) {
    unsafe {
        if let Err(error) = SetLayeredWindowAttributes(window, COLORREF(0), alpha, LWA_ALPHA) {
            tracing::warn!(%error, "não foi possível atualizar a sobreposição");
            return;
        }
        if alpha == 0 {
            let _ = ShowWindow(window, SW_HIDE);
        } else {
            if let Err(error) = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                rect[0],
                rect[1],
                rect[2] - rect[0],
                rect[3] - rect[1],
                SWP_NOACTIVATE,
            ) {
                tracing::warn!(%error, "não foi possível posicionar a sobreposição");
                return;
            }
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        }
    }
}

pub fn hide(_dispatcher: HWND) {
    LAYERS.with(|layers| {
        for layer in layers.borrow().iter() {
            unsafe {
                let _ = ShowWindow(layer.dim, SW_HIDE);
                let _ = ShowWindow(layer.warm, SW_HIDE);
            }
        }
    });
}

pub fn pump_messages() {
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message == WM_QUIT {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Returns independent (warmth, neutral dimming) alpha values.
pub fn overlay_alphas(
    cct: f32,
    brightness: f32,
    physical_active: bool,
    gamma_active: bool,
) -> (u8, u8) {
    layer_alphas(
        cct,
        brightness,
        physical_active,
        gamma_active.then_some(3400.0),
    )
}

fn layer_alphas(
    cct: f32,
    brightness: f32,
    physical_active: bool,
    gamma_cct: Option<f32>,
) -> (u8, u8) {
    let (start, max_warm) = gamma_cct.map_or((6500.0, 70.0), |applied| (applied.max(1901.0), 24.0));
    let warm = smoothstep(((start - cct) / (start - 1900.0)).clamp(0.0, 1.0)) * max_warm;
    let dim = if physical_active {
        0.0
    } else {
        smoothstep(1.0 - brightness.clamp(0.0, 1.0)) * 150.0
    };
    (warm.round() as u8, dim.round() as u8)
}

fn smoothstep(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composite(pixel: f32, warm_color: f32, warm: u8, dim: u8) -> f32 {
        let warm = warm as f32 / 255.0;
        let dim = dim as f32 / 255.0;
        (pixel * (1.0 - warm) + warm_color * warm) * (1.0 - dim)
    }

    #[test]
    fn neutral_dimming_preserves_black_and_never_adds_warmth() {
        let (warm, dim) = overlay_alphas(6500.0, 0.2, false, false);
        assert_eq!(warm, 0);
        assert!(dim > 0);
        assert_eq!(composite(0.0, 255.0, warm, dim), 0.0);
        assert!(composite(128.0, 255.0, warm, dim) < 128.0);
    }

    #[test]
    fn decreasing_brightness_cannot_raise_shadows_even_with_warmth() {
        let (warm, day_dim) = overlay_alphas(2300.0, 0.9, false, false);
        let (_, night_dim) = overlay_alphas(2300.0, 0.2, false, false);
        for pixel in [0.0, 64.0, 128.0, 255.0] {
            assert!(
                composite(pixel, 255.0, warm, night_dim) <= composite(pixel, 255.0, warm, day_dim)
            );
        }
    }

    #[test]
    fn only_the_output_with_physical_brightness_skips_software_dimming() {
        assert_eq!(overlay_alphas(6500.0, 0.2, true, true).1, 0);
        assert!(overlay_alphas(6500.0, 0.2, false, true).1 > 0);
    }

    #[test]
    fn warmth_remains_available_when_gamma_is_unsupported() {
        assert!(overlay_alphas(5500.0, 0.9, true, false).0 > 0);
        assert_eq!(overlay_alphas(5500.0, 0.9, true, true).0, 0);
        assert!(overlay_alphas(2300.0, 0.9, true, true).0 > 0);
    }

    #[test]
    fn maximum_dimming_keeps_content_visible() {
        assert!(overlay_alphas(1900.0, 0.0, false, false).1 < 200);
    }

    #[test]
    fn warm_approximation_completes_the_actual_gamma_floor() {
        assert!(layer_alphas(3600.0, 0.9, true, Some(4500.0)).0 > 0);
        assert_eq!(layer_alphas(3600.0, 0.9, true, Some(3600.0)).0, 0);
    }
}
