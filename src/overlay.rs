//! Full-screen warm-tint + dim overlay.
//!
//! Topmost, click-through, layered. Covers two jobs gamma cannot:
//! - warmth below the ~3400 K gamma floor (Win11 clamp)
//! - dimming when DDC is unavailable (laptop panels)
//!
//! The main thread pumps every message so the tray window stays alive.
//! Overlay WM_DESTROY must not PostQuitMessage — that killed the process
//! (and the tray icon) the moment Windows recycled the toolwindow.

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HBRUSH, HGDIOBJ, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetSystemMetrics, HCURSOR, HICON,
    HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, TranslateMessage,
    WM_DESTROY, WM_DISPLAYCHANGE, WM_PAINT, WM_QUIT, WNDCLASS_STYLES, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                // Pale peach, not saturated orange. COLORREF is 0x00BBGGRR.
                let brush: HBRUSH = CreateSolidBrush(COLORREF(0x00_B4_D2_FF));
                FillRect(hdc, &ps.rcPaint, brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                resize_to_virtual(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn virtual_rect() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

fn resize_to_virtual(hwnd: HWND) {
    let (x, y, w, h) = virtual_rect();
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
    }
}

/// Create the overlay window. Must be called from the thread that will call
/// `pump_messages()`.
pub fn create() -> anyhow::Result<HWND> {
    unsafe {
        let hmodule = GetModuleHandleW(None)?;
        let hinstance = HINSTANCE(hmodule.0);

        let class_name = w!("EstelOverlay");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: WNDCLASS_STYLES(0),
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_name,
            hIconSm: HICON::default(),
        };
        let _ = RegisterClassExW(&wc);

        let (x, y, w, h) = virtual_rect();

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class_name,
            PCWSTR::null(),
            WS_POPUP,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(hinstance),
            None,
        )?;

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
        Ok(hwnd)
    }
}

/// Update tint + dim. Safe to call from any thread.
///
/// `ddc_active`: when true, DDC already dimmed the backlight — overlay only
/// tints. When false (laptop / HDR / no MCCS), overlay also carries dim.
pub fn update(hwnd: HWND, target_cct: f32, brightness: f32, ddc_active: bool) {
    let alpha = overlay_alpha(target_cct, brightness, ddc_active);
    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        if alpha == 0 {
            let _ = ShowWindow(hwnd, SW_HIDE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

pub fn hide(hwnd: HWND) {
    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// Drain the thread queue: overlay + tray. Skip WM_QUIT so a destroyed
/// overlay cannot tear down the whole app.
pub fn pump_messages() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Warmth grows as CCT drops; dim grows as brightness drops, but only when
/// DDC is not already doing that job. Capped so the wash stays a comfort
/// layer, not a blackout.
pub fn overlay_alpha(cct: f32, brightness: f32, ddc_active: bool) -> u8 {
    const MAX_WARM: f32 = 38.0;
    const MAX_DIM: f32 = 70.0;
    const START_K: f32 = 4800.0;
    const FLOOR_K: f32 = 2300.0;
    const MAX_TOTAL: f32 = 80.0;

    let warm = if cct >= START_K {
        0.0
    } else {
        let t = ((START_K - cct) / (START_K - FLOOR_K)).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        t * MAX_WARM
    };

    let dim = if ddc_active || brightness >= 0.85 {
        0.0
    } else {
        let t = (1.0 - brightness.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        t * MAX_DIM
    };

    (warm + dim).min(MAX_TOTAL) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_is_invisible() {
        assert_eq!(overlay_alpha(6500.0, 0.9, false), 0);
        assert_eq!(overlay_alpha(6500.0, 1.0, true), 0);
        assert!(
            overlay_alpha(4700.0, 0.8, true) < 12,
            "suave evening must not punch orange"
        );
    }

    #[test]
    fn alta_night_is_a_wash_not_a_filter() {
        let a = overlay_alpha(2700.0, 0.3, true);
        assert!(a > 8, "alta night should tint a little, got {a}");
        assert!(a < 45, "alta night must stay a wash, got {a}");
    }

    #[test]
    fn night_without_ddc_is_darker_than_with_ddc() {
        let laptop = overlay_alpha(2300.0, 0.18, false);
        let desktop = overlay_alpha(2300.0, 0.18, true);
        assert!(laptop > desktop, "{laptop} vs {desktop}");
        assert!(laptop > 40, "laptop night must actually dim, got {laptop}");
    }

    #[test]
    fn never_blacks_out() {
        assert!(overlay_alpha(1900.0, 0.0, false) < 200);
    }
}
