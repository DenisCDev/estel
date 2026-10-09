//! A bounded recovery boundary around the desktop process, including native crashes.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, ResetEvent, SetEvent,
    WaitForSingleObject,
};
use windows::core::{HSTRING, w};

const START_DEADLINE: Duration = Duration::from_secs(45);
const STOP_DEADLINE: Duration = Duration::from_secs(30);
const MAX_RESTARTS: u32 = 3;

pub fn is_helper() -> bool {
    std::env::args_os().any(|arg| {
        matches!(
            arg.to_str(),
            Some(
                "--host"
                    | "--settings-window"
                    | "--display-worker"
                    | "--display-diagnostics"
                    | "--list-cameras"
                    | "--list-camera-devices"
                    | "--sample-ambient"
                    | "--sample-light-sensor"
                    | "--diagnostics"
                    | "--quit"
            )
        )
    })
}

fn handle(raw: HANDLE) -> OwnedHandle {
    // Every handle here is newly acquired and has exactly one owner.
    unsafe { OwnedHandle::from_raw_handle(raw.0) }
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

fn signaled(event: &OwnedHandle, timeout: Duration) -> anyhow::Result<bool> {
    match unsafe {
        WaitForSingleObject(
            raw(event),
            timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
        )
    } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(windows::core::Error::from_thread().into()),
    }
}

pub fn run() -> anyhow::Result<()> {
    let mutex = unsafe { CreateMutexW(None, false, w!("Local\\EstelLauncher"))? };
    let duplicate = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let _mutex = handle(mutex);
    if duplicate {
        if !std::env::args_os().any(|arg| arg == "--startup") {
            let quit = handle(unsafe { CreateEventW(None, true, false, w!("Local\\EstelQuit"))? });
            if signaled(&quit, Duration::ZERO)? {
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                        None,
                        w!(
                            "O Estel está encerrando e restaurando as telas. Aguarde alguns segundos e abra novamente."
                        ),
                        w!("Estel"),
                        windows::Win32::UI::WindowsAndMessaging::MB_OK
                            | windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
                    );
                }
                return Ok(());
            }
            let event = handle(unsafe {
                CreateEventW(None, false, false, w!("Local\\EstelOpenSettings"))?
            });
            unsafe { SetEvent(raw(&event))? };
            let manual =
                handle(unsafe { CreateEventW(None, true, false, w!("Local\\EstelManualLaunch"))? });
            unsafe { SetEvent(raw(&manual))? };
        }
        return Ok(());
    }

    let quit = handle(unsafe { CreateEventW(None, true, false, w!("Local\\EstelQuit"))? });
    unsafe { ResetEvent(raw(&quit))? };
    // Keep requests sent while the host is starting or recovering alive until it can read them.
    let _settings =
        handle(unsafe { CreateEventW(None, false, false, w!("Local\\EstelOpenSettings"))? });
    let manual =
        handle(unsafe { CreateEventW(None, true, false, w!("Local\\EstelManualLaunch"))? });
    let ready_name = HSTRING::from(format!("Local\\EstelReady-{}", std::process::id()));
    let ready = handle(unsafe { CreateEventW(None, true, false, &ready_name)? });
    let mut last_error = None;
    let mut progress = crate::launcher_progress::Progress::default();

    for attempt in 0..=MAX_RESTARTS {
        if signaled(&quit, Duration::ZERO)? {
            return Ok(());
        }
        unsafe { ResetEvent(raw(&ready))? };
        if attempt > 0 {
            progress.show(attempt)?;
        }
        let result = launch(&ready, &quit, &manual, attempt, &mut progress);
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                tracing::error!(attempt, %error, "processo principal falhou");
                last_error = Some(error);
            }
        }
        crate::overlay::pump_messages();
        if progress.cancelled() {
            unsafe { SetEvent(raw(&quit))? };
        }
        if signaled(&quit, Duration::ZERO)? {
            return Ok(());
        }
        if attempt < MAX_RESTARTS {
            tracing::warn!(restart = attempt + 1, "reiniciando o Estel após falha");
            progress.hide();
            progress.show(attempt + 1)?;
            let retry = Instant::now() + Duration::from_secs(3 * u64::from(attempt + 1));
            while Instant::now() < retry {
                crate::overlay::pump_messages();
                if signaled(&manual, Duration::ZERO)? {
                    unsafe { ResetEvent(raw(&manual))? };
                    progress.enable();
                    progress.show(attempt + 1)?;
                }
                if progress.cancelled() {
                    unsafe { SetEvent(raw(&quit))? };
                }
                if signaled(&quit, Duration::ZERO)? {
                    return Ok(());
                }
                crate::runtime::wait_for_work(
                    &[raw(&quit), raw(&manual)],
                    retry.saturating_duration_since(Instant::now()),
                )?;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("o Estel não respondeu")))
}

struct Host(Child);

impl Drop for Host {
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                if let Err(error) = self.0.kill() {
                    tracing::error!(%error, "não foi possível encerrar o processo principal");
                }
            }
        }
    }
}

fn launch(
    ready: &OwnedHandle,
    quit: &OwnedHandle,
    manual: &OwnedHandle,
    attempt: u32,
    progress: &mut crate::launcher_progress::Progress,
) -> anyhow::Result<()> {
    let mut child = Host(
        Command::new(std::env::current_exe()?)
            .args(std::env::args_os().skip(1))
            .arg("--host")
            .env("ESTEL_LAUNCHER_PID", std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()?,
    );
    tracing::info!(pid = child.0.id(), attempt, "processo principal iniciado");
    let start = Instant::now();
    let mut started = false;
    let mut stopping = None;
    loop {
        crate::overlay::pump_messages();
        if signaled(manual, Duration::ZERO)? {
            unsafe { ResetEvent(raw(manual))? };
            progress.enable();
            if !started {
                progress.show(attempt)?;
            }
        }
        if progress.cancelled() {
            unsafe { SetEvent(raw(quit))? };
        }
        if let Some(status) = child.0.try_wait()? {
            if status.success() || stopping.is_some() {
                return Ok(());
            }
            anyhow::bail!("o processo principal terminou com {status}");
        }
        if signaled(quit, Duration::ZERO)? {
            let stop = stopping.get_or_insert_with(Instant::now);
            if stop.elapsed() >= STOP_DEADLINE {
                anyhow::bail!("o encerramento excedeu 30 segundos");
            }
        }
        if !started {
            started = signaled(ready, Duration::ZERO)?;
            if started {
                progress.hide();
                tracing::info!(pid = child.0.id(), "inicialização confirmada");
            } else if stopping.is_none() && start.elapsed() >= START_DEADLINE {
                anyhow::bail!("a inicialização excedeu 45 segundos");
            } else if stopping.is_none() && start.elapsed() >= Duration::from_secs(3) {
                progress.show(attempt)?;
            }
        }
        // Once ready, wait for exit or an explicit quit without polling the desktop.
        let process = HANDLE(child.0.as_raw_handle());
        let handles = if stopping.is_some() {
            vec![process]
        } else if started {
            vec![process, raw(quit), raw(manual)]
        } else {
            vec![process, raw(quit), raw(ready), raw(manual)]
        };
        let timeout = if let Some(stop) = stopping {
            STOP_DEADLINE.saturating_sub(stop.elapsed()).as_millis() as u32
        } else if started {
            u32::MAX
        } else {
            START_DEADLINE.saturating_sub(start.elapsed()).as_millis() as u32
        };
        let timeout = if !started && progress.interactive() {
            timeout.min(3000)
        } else {
            timeout
        };
        crate::runtime::wait_for_work(&handles, Duration::from_millis(u64::from(timeout)))?;
    }
}

pub fn settings_instance() -> anyhow::Result<Option<OwnedHandle>> {
    let mutex = unsafe { CreateMutexW(None, false, w!("Local\\EstelSettingsWindow"))? };
    let duplicate = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let mutex = handle(mutex);
    if duplicate {
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
        };
        if let Ok(window) = unsafe { FindWindowW(None, w!("Estel")) } {
            unsafe {
                let _ = ShowWindow(window, SW_RESTORE);
            }
            if !unsafe { SetForegroundWindow(window) }.as_bool() {
                tracing::warn!("não foi possível trazer o painel do Estel para frente");
            }
        }
        return Ok(None);
    }
    Ok(Some(mutex))
}

pub fn notify_ready() -> anyhow::Result<()> {
    let Some(pid) = std::env::var_os("ESTEL_LAUNCHER_PID") else {
        return Ok(());
    };
    let pid = pid
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| anyhow::anyhow!("identificador do iniciador inválido"))?;
    let name = HSTRING::from(format!("Local\\EstelReady-{pid}"));
    let event = handle(unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &name)? });
    unsafe { SetEvent(raw(&event))? };
    Ok(())
}
