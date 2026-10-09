//! Small local snapshots, read only when the host reports a change.

use std::io::Read;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use windows::Win32::Foundation::{CloseHandle, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForMultipleObjects,
};
use windows::core::w;

use crate::config::Config;
use crate::hardware_worker::HardwareStatus;
use crate::runtime::WakeSignal;

const MAX_STATUS_BYTES: u64 = 256 * 1024;

#[derive(serde::Serialize, serde::Deserialize)]
struct HardwareReport {
    pid: u32,
    status: Result<HardwareStatus, String>,
}

pub fn notify() {
    let event =
        match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\EstelStatusChanged")) } {
            Ok(event) => event,
            // No settings process is listening while the panel is closed.
            Err(error) if error.code().0 as u32 == 0x80070002 => return,
            Err(error) => {
                tracing::warn!(%error, "não foi possível avisar o painel");
                return;
            }
        };
    if let Err(error) = unsafe { SetEvent(event) } {
        tracing::warn!(%error, "não foi possível atualizar o estado no painel");
    }
    if let Err(error) = unsafe { CloseHandle(event) } {
        tracing::warn!(%error, "não foi possível liberar o aviso do painel");
    }
}

pub fn publish_hardware(status: &HardwareStatus) {
    publish(HardwareReport {
        pid: std::process::id(),
        status: Ok(status.clone()),
    });
}

pub fn publish_error() {
    publish(HardwareReport { pid: std::process::id(), status: Err("O ajuste das telas não respondeu. Os efeitos foram ocultados; confira a conexão dos monitores e reinicie o Estel para tentar novamente.".into()) });
}

pub fn publish_stopped(restored: bool) {
    let message = if restored {
        "O Estel foi fechado e a restauração das telas foi confirmada. Abra o Estel novamente para retomar os ajustes."
    } else {
        "O Estel foi fechado com a restauração das telas pendente. Reconecte os monitores e abra o Estel para tentar recuperar os ajustes originais."
    };
    publish(HardwareReport {
        pid: 0,
        status: Err(message.into()),
    });
}

pub fn publish_stopping() {
    publish(HardwareReport {
        pid: std::process::id(),
        status: Err("Encerramento solicitado. Aguarde a restauração das telas antes de abrir o Estel novamente.".into()),
    });
}

fn publish(report: HardwareReport) {
    let result = serde_json::to_vec(&report)
        .map_err(std::io::Error::other)
        .and_then(|bytes| {
            crate::config::atomic_write(
                &Config::config_path().with_file_name("hardware-status.json"),
                &bytes,
            )
        });
    match result {
        Ok(()) => notify(),
        Err(error) => tracing::warn!(%error, "não foi possível publicar o estado das telas"),
    }
}

pub fn hardware() -> Result<HardwareStatus, String> {
    let result = (|| -> anyhow::Result<HardwareReport> {
        let mut bytes = Vec::new();
        std::fs::File::open(Config::config_path().with_file_name("hardware-status.json"))?
            .take(MAX_STATUS_BYTES + 1)
            .read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_STATUS_BYTES,
            "estado das telas grande demais"
        );
        let report: HardwareReport = serde_json::from_slice(&bytes)?;
        if let Ok(status) = &report.status {
            anyhow::ensure!(status.outputs.len() <= 128, "quantidade de telas inválida");
            anyhow::ensure!(
                process_is_running(report.pid),
                "processo principal encerrado"
            );
        }
        Ok(report)
    })();
    result
        .map_err(|error| {
            tracing::debug!(%error, "estado das telas ainda indisponível");
            "Aguardando a identificação das telas pelo processo principal.".into()
        })
        .and_then(|report| report.status)
}

fn process_is_running(pid: u32) -> bool {
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    let Ok(process) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        return false;
    };
    let running =
        unsafe { WaitForSingleObject(process, 0) } == windows::Win32::Foundation::WAIT_TIMEOUT;
    if let Err(error) = unsafe { CloseHandle(process) } {
        tracing::warn!(%error, "não foi possível liberar a consulta do processo principal");
    }
    running
}

pub struct UiUpdates {
    stop: WakeSignal,
    changed: Arc<AtomicBool>,
    errors: std::sync::mpsc::Receiver<String>,
}

impl UiUpdates {
    pub fn start(context: eframe::egui::Context) -> anyhow::Result<Self> {
        let stop = WakeSignal::new()?;
        let worker_stop = stop.clone();
        let changed = Arc::new(AtomicBool::new(true));
        let worker_changed = changed.clone();
        let event = WakeSignal::named(w!("Local\\EstelStatusChanged"))?;
        let (error_tx, errors) = std::sync::mpsc::sync_channel(1);
        tracing::debug!("painel acompanhando as atualizações do Estel");
        std::thread::Builder::new().name("estel-status-ui".into()).spawn(move || {
            // A panel can stay open across host recovery and closing/reopening the desktop.
            let handles = [worker_stop.handle(), event.handle()];
            loop {
                let result = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                if result == WAIT_OBJECT_0 { break; }
                if result == WAIT_FAILED {
                        tracing::warn!(error = %windows::core::Error::from_thread(), "avisos do painel indisponíveis");
                    let _ = error_tx.send("A atualização automática das informações parou. Use Atualizar informações ou reabra o painel.".into());
                    context.request_repaint();
                    break;
                }
                worker_changed.store(true, Ordering::Release);
                tracing::debug!("painel recebeu atualização de estado");
                context.request_repaint();
            }
        })?;
        Ok(Self {
            stop,
            changed,
            errors,
        })
    }

    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }

    pub fn take_error(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }
}

impl Drop for UiUpdates {
    fn drop(&mut self) {
        self.stop.notify();
    }
}
