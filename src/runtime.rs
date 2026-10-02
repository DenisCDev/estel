//! Event-driven coordination for the desktop host and its background workers.

use std::sync::Arc;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    HPOWERNOTIFY, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
    UnregisterPowerSettingNotification,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_UNLOCK, WTSActive,
    WTSFreeMemory, WTSINFOEXW, WTSQuerySessionInformationW, WTSRegisterSessionNotification,
    WTSSessionInfoEx, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::SystemServices::GUID_SESSION_DISPLAY_STATUS;
use windows::Win32::System::Threading::{CreateEventW, SetEvent};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, PWSTR, w};

struct EventHandle(isize);

impl Drop for EventHandle {
    fn drop(&mut self) {
        if let Err(error) = unsafe { CloseHandle(HANDLE(self.0 as _)) } {
            tracing::warn!(%error, "não foi possível liberar um evento do Windows");
        }
    }
}

/// Kernel events can be signaled across threads; ownership outlives every sender.
#[derive(Clone)]
pub struct WakeSignal(Arc<EventHandle>);

impl WakeSignal {
    pub fn new() -> windows::core::Result<Self> {
        Self::named(PCWSTR::null())
    }

    pub(crate) fn named(name: PCWSTR) -> windows::core::Result<Self> {
        let handle = unsafe { CreateEventW(None, false, false, name)? };
        Ok(Self(Arc::new(EventHandle(handle.0 as isize))))
    }

    pub fn handle(&self) -> HANDLE {
        HANDLE(self.0.0 as _)
    }

    pub fn notify(&self) {
        if let Err(error) = unsafe { SetEvent(self.handle()) } {
            tracing::error!(%error, "não foi possível avisar o processo principal");
        }
    }
}

pub fn wait_for_work(handles: &[HANDLE], timeout: Duration) -> windows::core::Result<usize> {
    let result = unsafe {
        MsgWaitForMultipleObjectsEx(
            Some(handles),
            timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
            QS_ALLINPUT,
            MWMO_INPUTAVAILABLE,
        )
    };
    if result == windows::Win32::Foundation::WAIT_FAILED {
        return Err(windows::core::Error::from_win32());
    }
    Ok(result.0 as usize)
}

#[derive(Debug)]
struct ActivityState {
    display_on: bool,
    locked: bool,
    suspended: bool,
    refresh: bool,
    closing: bool,
}

impl Default for ActivityState {
    fn default() -> Self {
        Self {
            display_on: true,
            locked: false,
            suspended: false,
            refresh: true,
            closing: false,
        }
    }
}

impl ActivityState {
    fn available(&self) -> bool {
        self.display_on && !self.locked && !self.suspended
    }

    fn session_changed(&mut self, event: u32) {
        match event {
            WTS_SESSION_LOCK
            | WTS_SESSION_LOGOFF
            | WTS_CONSOLE_DISCONNECT
            | WTS_REMOTE_DISCONNECT => self.locked = true,
            WTS_SESSION_UNLOCK | WTS_SESSION_LOGON | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                self.locked = false;
                self.refresh = true;
            }
            _ => {}
        }
    }
}

pub struct ActivityMonitor {
    hwnd: HWND,
    state: Box<ActivityState>,
    power: Option<HPOWERNOTIFY>,
    session_registered: bool,
}

impl ActivityMonitor {
    pub fn new() -> anyhow::Result<Self> {
        let mut state = Box::<ActivityState>::default();
        state.locked = match session_is_unlocked() {
            Ok(unlocked) => !unlocked,
            Err(error) => {
                tracing::warn!(%error, "não foi possível identificar a sessão; aguardando desbloqueio");
                true
            }
        };
        let instance = unsafe { GetModuleHandleW(None)? };
        let class = WNDCLASSW {
            lpfnWndProc: Some(activity_window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("EstelActivityEvents"),
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(windows::core::Error::from_win32().into());
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.lpszClassName,
                w!("EstelActivityEvents"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        // The Box address stays stable until the window has been destroyed.
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (&mut *state as *mut ActivityState) as isize,
            )
        };
        let power = unsafe {
            RegisterPowerSettingNotification(
                HANDLE(hwnd.0),
                &GUID_SESSION_DISPLAY_STATUS,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        }
        .map_err(|error| tracing::warn!(%error, "avisos de energia indisponíveis"))
        .ok();
        let session_registered =
            unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) }
                .map_err(
                    |error| tracing::warn!(%error, "avisos de bloqueio da sessão indisponíveis"),
                )
                .is_ok();
        Ok(Self {
            hwnd,
            state,
            power,
            session_registered,
        })
    }

    pub fn available(&self) -> bool {
        self.state.available()
    }

    pub fn take_refresh(&mut self) -> bool {
        std::mem::take(&mut self.state.refresh)
    }

    pub fn closing(&self) -> bool {
        self.state.closing
    }
}

fn session_is_unlocked() -> anyhow::Result<bool> {
    let mut buffer = PWSTR::null();
    let mut length = 0;
    unsafe {
        WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut length,
        )?
    };
    let result = if !buffer.is_null() && length as usize >= std::mem::size_of::<WTSINFOEXW>() {
        let info = unsafe { &*buffer.0.cast::<WTSINFOEXW>() };
        if info.Level == 1 {
            let session = unsafe { info.Data.WTSInfoExLevel1 };
            Ok(session.SessionState == WTSActive
                && session.SessionFlags as u32 == WTS_SESSIONSTATE_UNLOCK)
        } else {
            Err(anyhow::anyhow!(
                "nível de informação da sessão não reconhecido"
            ))
        }
    } else {
        Err(anyhow::anyhow!("resposta da sessão incompleta"))
    };
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    result
}

impl Drop for ActivityMonitor {
    fn drop(&mut self) {
        if let Some(power) = self.power.take()
            && let Err(error) = unsafe { UnregisterPowerSettingNotification(power) }
        {
            tracing::warn!(%error, "não foi possível remover avisos de energia");
        }
        if self.session_registered
            && let Err(error) = unsafe { WTSUnRegisterSessionNotification(self.hwnd) }
        {
            tracing::warn!(%error, "não foi possível remover avisos da sessão");
        }
        if let Err(error) = unsafe { DestroyWindow(self.hwnd) } {
            tracing::warn!(%error, "não foi possível fechar a janela de eventos");
        }
    }
}

unsafe extern "system" fn activity_window_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ActivityState };
    if !state.is_null() {
        let state = unsafe { &mut *state };
        match message {
            WM_DISPLAYCHANGE => state.refresh = true,
            WM_WTSSESSION_CHANGE => state.session_changed(wp.0 as u32),
            WM_POWERBROADCAST => match wp.0 as u32 {
                PBT_APMSUSPEND => state.suspended = true,
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                    state.suspended = false;
                    state.refresh = true;
                }
                PBT_POWERSETTINGCHANGE if lp.0 != 0 => {
                    let setting = unsafe { &*(lp.0 as *const POWERBROADCAST_SETTING) };
                    if setting.PowerSetting == GUID_SESSION_DISPLAY_STATUS
                        && setting.DataLength == 4
                    {
                        let value = unsafe {
                            std::ptr::read_unaligned(setting.Data.as_ptr().cast::<u32>())
                        };
                        state.display_on = value != 0;
                        state.refresh |= state.display_on;
                    }
                }
                _ => {}
            },
            WM_QUERYENDSESSION => {
                state.closing = true;
                return LRESULT(1);
            }
            WM_CLOSE => {
                state.closing = true;
                return LRESULT(0);
            }
            WM_ENDSESSION => {
                state.closing = wp.0 != 0;
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocking_does_not_restart_sensors_while_display_is_off() {
        let mut state = ActivityState {
            display_on: false,
            ..Default::default()
        };
        state.session_changed(WTS_SESSION_LOCK);
        state.session_changed(WTS_SESSION_UNLOCK);
        assert!(!state.available());
        state.display_on = true;
        assert!(state.available());
    }

    #[test]
    fn suspend_and_remote_disconnect_prevent_capture() {
        let mut state = ActivityState::default();
        assert!(state.available());
        state.session_changed(WTS_REMOTE_DISCONNECT);
        assert!(!state.available());
        state.session_changed(WTS_REMOTE_CONNECT);
        state.suspended = true;
        assert!(!state.available());
        state.suspended = false;
        assert!(state.available());
    }
}
