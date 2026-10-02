//! One isolated driver process, one pending intent, and a hard request deadline.
//! The tray never waits for DDC, WMI or a display driver's gamma implementation.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use crate::{brightness, display, display_topology, runtime::WakeSignal, session, target::Target};

const DRIVER_DEADLINE: Duration = Duration::from_secs(4);
const MAX_MESSAGE: u64 = 128 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OutputStatus {
    pub id: String,
    pub name: String,
    pub label: String,
    pub rect: [i32; 4],
    pub color_mode: display_topology::ColorMode,
    pub internal: bool,
    pub gamma_active: bool,
    pub gamma_cct: Option<f32>,
    pub brightness_active: bool,
    pub brightness_method: Option<String>,
    pub technology: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HardwareStatus {
    pub outputs: Vec<OutputStatus>,
    pub restored: bool,
    pub applied_target: Option<Target>,
    pub recovery_warning: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum Action {
    Target {
        target: Target,
        gamma_floor: f32,
        min_lum: f32,
    },
    Park,
    Refresh,
    Inspect,
    Stop,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Request {
    action: Action,
    refresh: bool,
}

#[derive(Default)]
struct Pending {
    action: Option<Action>,
    refresh: bool,
    stop: bool,
}

struct Shared {
    pending: Mutex<Pending>,
    changed: Condvar,
    status: Mutex<Option<Result<HardwareStatus, String>>>,
}

pub struct HardwareClient {
    shared: Arc<Shared>,
    stopped: mpsc::Receiver<bool>,
}

impl HardwareClient {
    pub fn start(wake: WakeSignal) -> anyhow::Result<Self> {
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending::default()),
            changed: Condvar::new(),
            status: Mutex::new(None),
        });
        let (done, stopped) = mpsc::sync_channel(1);
        let supervisor = shared.clone();
        std::thread::Builder::new()
            .name("display-supervisor".into())
            .spawn(move || {
                let restored = supervise(&supervisor, &wake);
                let _ = done.send(restored);
            })?;
        Ok(Self { shared, stopped })
    }

    pub fn set_target(&mut self, target: &Target, gamma_floor: f32, min_lum: f32) {
        self.submit(Action::Target {
            target: *target,
            gamma_floor,
            min_lum,
        });
    }

    pub fn park(&mut self) {
        self.submit(Action::Park);
    }

    pub fn refresh(&mut self) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        pending.refresh = true;
        self.shared.changed.notify_one();
    }

    pub fn poll(&mut self) -> Option<Result<HardwareStatus, String>> {
        self.shared
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }

    pub fn shutdown(&mut self) -> bool {
        {
            let mut pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            pending.stop = true;
            pending.action = None;
            self.shared.changed.notify_one();
        }
        // An in-flight request finishes before Stop can restore the displays.
        self.stopped
            .recv_timeout(DRIVER_DEADLINE * 2 + Duration::from_secs(1))
            .unwrap_or(false)
    }

    fn submit(&self, action: Action) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !pending.stop {
            pending.action = Some(action);
            self.shared.changed.notify_one();
        }
    }
}

impl Drop for HardwareClient {
    fn drop(&mut self) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        pending.stop = true;
        pending.action = None;
        self.shared.changed.notify_one();
    }
}

fn supervise(shared: &Shared, wake: &WakeSignal) -> bool {
    let mut process: Option<DriverProcess> = None;
    let mut failed = false;
    loop {
        let request = {
            let mut pending = shared
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while pending.action.is_none() && !pending.refresh && !pending.stop {
                pending = shared
                    .changed
                    .wait(pending)
                    .unwrap_or_else(|error| error.into_inner());
            }
            Request {
                action: if pending.stop {
                    Action::Stop
                } else {
                    pending.action.take().unwrap_or(Action::Refresh)
                },
                refresh: std::mem::take(&mut pending.refresh),
            }
        };
        let stop = matches!(request.action, Action::Stop);
        let park = matches!(request.action, Action::Park);
        if failed && !request.refresh && matches!(request.action, Action::Target { .. }) {
            continue;
        }
        let result = (|| -> anyhow::Result<HardwareStatus> {
            if process.is_none() {
                process = Some(DriverProcess::spawn()?);
            }
            process
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("processo de tela indisponível"))?
                .request(&request)
        })();
        match result {
            Ok(status) => {
                failed = false;
                let restored = status.restored;
                *shared
                    .status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(Ok(status));
                wake.notify();
                if stop {
                    return restored;
                }
                if park {
                    process.take();
                }
            }
            Err(error) => {
                process.take();
                failed = true;
                tracing::error!(%error, "controle da tela interrompido; recuperação preservada");
                *shared
                    .status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) =
                    Some(Err(format!("Controle da tela interrompido: {error}")));
                wake.notify();
                if stop {
                    return false;
                }
            }
        }
    }
}

struct DriverProcess {
    child: Child,
    input: ChildStdin,
    replies: mpsc::Receiver<anyhow::Result<HardwareStatus>>,
}

impl DriverProcess {
    fn spawn() -> anyhow::Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--display-worker")
            .env("ESTEL_DISPLAY_PARENT_PID", std::process::id().to_string())
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("entrada do controle da tela ausente"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("saída do controle da tela ausente"))?;
        let (send, replies) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("display-replies".into())
            .spawn(move || {
                let mut reader = BufReader::new(output);
                loop {
                    let result = read_message::<HardwareStatus>(&mut reader);
                    let ended = result.is_err();
                    if send.send(result).is_err() || ended {
                        break;
                    }
                }
            })?;
        Ok(Self {
            child,
            input,
            replies,
        })
    }

    fn request(&mut self, request: &Request) -> anyhow::Result<HardwareStatus> {
        serde_json::to_writer(&mut self.input, request)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        self.replies
            .recv_timeout(DRIVER_DEADLINE)
            .map_err(|_| anyhow::anyhow!("o driver excedeu o prazo de 4 segundos"))?
    }
}

impl Drop for DriverProcess {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            if self.child.try_wait().ok().flatten().is_none() {
                tracing::error!(%error, "não foi possível encerrar o controle da tela");
            }
        }
        // try_wait avoids turning a failed TerminateProcess into an unbounded wait.
        if let Err(error) = self.child.try_wait() {
            tracing::debug!(%error, "estado final do controle da tela indisponível");
        }
    }
}

fn read_message<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> anyhow::Result<T> {
    let mut bytes = Vec::new();
    reader.take(MAX_MESSAGE + 1).read_until(b'\n', &mut bytes)?;
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_MESSAGE && bytes.last() == Some(&b'\n'),
        "resposta do controle da tela inválida"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

fn validate_target(target: &Target, gamma_floor: f32, min_lum: f32) -> anyhow::Result<()> {
    anyhow::ensure!(
        target.cct_kelvin.is_finite() && (1000.0..=10000.0).contains(&target.cct_kelvin),
        "temperatura de cor inválida"
    );
    anyhow::ensure!(
        target.brightness.is_finite() && (0.0..=1.0).contains(&target.brightness),
        "brilho inválido"
    );
    anyhow::ensure!(
        gamma_floor.is_finite() && (1000.0..=6500.0).contains(&gamma_floor),
        "limite de cor inválido"
    );
    anyhow::ensure!(
        min_lum.is_finite() && (0.0..=1.0).contains(&min_lum),
        "limite de brilho inválido"
    );
    Ok(())
}

pub fn inspect() -> anyhow::Result<HardwareStatus> {
    DriverProcess::spawn()?.request(&Request {
        action: Action::Inspect,
        refresh: false,
    })
}

fn inspect_local() -> anyhow::Result<HardwareStatus> {
    let outputs = display_topology::enumerate()?;
    let mut report = status(
        &outputs,
        &display::Controller::default(),
        &brightness::Controller::default(),
        None,
        !session::is_dirty(),
    );
    let capabilities = brightness::inspect(&outputs);
    for output in &mut report.outputs {
        if let Some(capability) = capabilities
            .iter()
            .find(|capability| capability.name == output.name)
        {
            output.brightness_method = Some(capability.method.clone());
            output.technology = capability.technology.clone();
        }
    }
    Ok(report)
}

fn status(
    outputs: &[display_topology::Output],
    gamma: &display::Controller,
    backlight: &brightness::Controller,
    target: Option<Target>,
    restored: bool,
) -> HardwareStatus {
    HardwareStatus {
        outputs: outputs.iter().map(|output| OutputStatus {
            id: output.id.clone(), name: output.name.clone(), label: output.label.clone(),
            rect: output.rect, color_mode: output.color_mode, internal: output.internal,
            gamma_active: gamma.active_for(&output.name),
            gamma_cct: gamma.cct_for(&output.name),
            brightness_active: backlight.active_for(&output.name),
            brightness_method: backlight.method_for(&output.name),
            technology: backlight.technology_for(&output.name),
        }).collect(),
        restored,
        applied_target: target,
        recovery_warning: gamma.warning().or_else(|| backlight.warning()).or_else(|| {
            (target.is_none() && !restored).then(|| "Restauração pendente. Reconecte as telas para recuperar os ajustes originais; os registros existentes foram preservados.".into())
        }),
    }
}

pub fn run_stdio() -> anyhow::Result<()> {
    monitor_parent()?;
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    let result = serve();
    unsafe {
        CoUninitialize();
    }
    result
}

fn serve() -> anyhow::Result<()> {
    let mut gamma = display::Controller::default();
    let mut backlight = brightness::Controller::default();
    let mut outputs = Vec::new();
    let mut initialized = false;
    let mut target = None;
    let mut floor = 3400.0;
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    loop {
        let request: Request = match read_message(&mut input) {
            Ok(request) => request,
            Err(error) => {
                let restored = gamma.park() & backlight.park();
                if restored {
                    session::mark_clean();
                }
                return Err(error);
            }
        };
        if matches!(request.action, Action::Inspect) {
            serde_json::to_writer(&mut output, &inspect_local()?)?;
            output.write_all(b"\n")?;
            output.flush()?;
            return Ok(());
        }
        let current = display_topology::enumerate()?;
        if !initialized
            || request.refresh
            || matches!(request.action, Action::Refresh)
            || !same_topology(&outputs, &current)
        {
            outputs = current;
            gamma.refresh(&outputs);
            backlight.refresh(&outputs);
            initialized = true;
        }
        let stopping = matches!(request.action, Action::Stop);
        let restored = match request.action {
            Action::Target {
                target: next,
                gamma_floor,
                min_lum,
            } => {
                validate_target(&next, gamma_floor, min_lum)?;
                floor = gamma_floor;
                target = Some(next);
                gamma.apply(next.cct_kelvin, floor);
                backlight.apply(next.brightness);
                false
            }
            Action::Park | Action::Stop => {
                target = None;
                let restored = gamma.park() & backlight.park();
                if restored {
                    session::mark_clean();
                }
                restored
            }
            Action::Refresh => {
                if let Some(target) = target {
                    gamma.apply(target.cct_kelvin, floor);
                    backlight.apply(target.brightness);
                    false
                } else {
                    let restored = gamma.park() & backlight.park();
                    if restored {
                        session::mark_clean();
                    }
                    restored
                }
            }
            Action::Inspect => unreachable!(),
        };
        serde_json::to_writer(
            &mut output,
            &status(&outputs, &gamma, &backlight, target, restored),
        )?;
        output.write_all(b"\n")?;
        output.flush()?;
        if stopping {
            return Ok(());
        }
    }
}

fn same_topology(before: &[display_topology::Output], after: &[display_topology::Output]) -> bool {
    before.len() == after.len()
        && before.iter().zip(after).all(|(a, b)| {
            a.id == b.id
                && a.name == b.name
                && a.color_mode == b.color_mode
                && a.rect == b.rect
                && a.cloned == b.cloned
                && a.monitor == b.monitor
        })
}

fn monitor_parent() -> anyhow::Result<()> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_SYNCHRONIZE, TerminateProcess, WaitForSingleObject,
    };
    let parent: u32 = std::env::var("ESTEL_DISPLAY_PARENT_PID")?.parse()?;
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, parent)? };
    let raw = process.0 as usize;
    std::thread::Builder::new()
        .name("display-parent".into())
        .spawn(move || unsafe {
            let process = windows::Win32::Foundation::HANDLE(raw as *mut core::ffi::c_void);
            let stopped = WaitForSingleObject(process, u32::MAX);
            let _ = CloseHandle(process);
            if stopped == WAIT_OBJECT_0 {
                let _ = TerminateProcess(GetCurrentProcess(), 1);
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_waits_for_in_flight_work_then_restoration() {
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending::default()),
            changed: Condvar::new(),
            status: Mutex::new(None),
        });
        let (done, stopped) = mpsc::sync_channel(1);
        let supervisor = shared.clone();
        let worker = std::thread::spawn(move || {
            let mut pending = supervisor.pending.lock().unwrap();
            while !pending.stop {
                pending = supervisor.changed.wait(pending).unwrap();
            }
            drop(pending);
            // Both operations fit their individual deadlines, but not five seconds together.
            std::thread::sleep(Duration::from_secs(3));
            std::thread::sleep(Duration::from_secs(3));
            done.send(true).unwrap();
        });
        let mut client = HardwareClient { shared, stopped };
        assert!(client.shutdown());
        worker.join().unwrap();
    }

    #[test]
    fn ipc_rejects_truncated_and_oversized_messages() {
        let mut truncated = std::io::Cursor::new(b"{\"outputs\":[]}");
        assert!(read_message::<HardwareStatus>(&mut truncated).is_err());
        let mut huge = std::io::Cursor::new(vec![b' '; MAX_MESSAGE as usize + 1]);
        assert!(read_message::<HardwareStatus>(&mut huge).is_err());
    }

    #[test]
    fn invalid_targets_never_reach_display_drivers() {
        let mut target = Target::neutral();
        target.brightness = f32::NAN;
        assert!(validate_target(&target, 3400.0, 0.3).is_err());
        target.brightness = 0.3;
        target.cct_kelvin = 0.0;
        assert!(validate_target(&target, 3400.0, 0.3).is_err());
        assert!(validate_target(&Target::neutral(), 3400.0, 0.3).is_ok());
    }
}
