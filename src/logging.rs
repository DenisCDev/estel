//! Persist startup and panic diagnostics with a fixed disk budget per process role.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

struct LogFile {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
}

impl LogFile {
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let bytes = file.metadata()?.len();
        Ok(Self {
            path: path.into(),
            file: Some(file),
            bytes,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.take();
        let backup = self.path.with_extension("log.1");
        match std::fs::remove_file(&backup) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::fs::rename(&self.path, backup)?;
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
        );
        self.bytes = 0;
        Ok(())
    }
}

impl Write for LogFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.bytes + buffer.len() as u64 > MAX_BYTES {
            self.rotate()?;
        }
        let buffer = &buffer[..buffer.len().min(MAX_BYTES as usize)];
        let result = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("arquivo de log indisponível"))?
            .write(buffer);
        match result {
            Ok(written) => {
                self.bytes += written as u64;
                Ok(written)
            }
            Err(error) => {
                eprintln!("Estel: não foi possível gravar o diagnóstico: {error}");
                Err(error)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("arquivo de log indisponível"))?
            .flush()
    }
}

pub fn init() {
    let args: Vec<_> = std::env::args_os().collect();
    let role = if args.iter().any(|arg| arg == "--host") {
        "estel"
    } else if args.iter().any(|arg| arg == "--settings-window") {
        "settings"
    } else if args.iter().any(|arg| arg == "--display-worker") {
        "display"
    } else if args.iter().any(|arg| arg == "--sample-light-sensor") {
        "light-sensor"
    } else if args.iter().any(|arg| {
        arg == "--sample-ambient" || arg == "--list-cameras" || arg == "--list-camera-devices"
    }) {
        "camera"
    } else if crate::launcher::is_helper() {
        "diagnostics"
    } else {
        "launcher"
    };
    let path = crate::Config::config_path().with_file_name(format!("{role}.log"));
    let file = path
        .parent()
        .ok_or_else(|| io::Error::other("pasta de log inválida"))
        .and_then(std::fs::create_dir_all)
        .and_then(|()| LogFile::open(&path));
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,estel=info"));
    let initialized = match file {
        Ok(file) => tracing_subscriber::fmt()
            .with_writer(Mutex::new(file))
            .with_ansi(false)
            .with_thread_ids(true)
            .with_env_filter(filter)
            .try_init(),
        Err(error) => {
            eprintln!("Estel: não foi possível abrir o log: {error}");
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(filter)
                .try_init()
        }
    };
    if let Err(error) = initialized {
        eprintln!("Estel: diagnóstico indisponível: {error}");
    }
    std::panic::set_hook(Box::new(|info| {
        tracing::error!(panic = %info, pid = std::process::id(), "falha interna no Estel");
    }));
    tracing::info!(
        pid = std::process::id(),
        version = env!("CARGO_PKG_VERSION"),
        role,
        "processo iniciando"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_rotate_with_a_bounded_backup_and_preserve_latest_diagnostics() {
        let dir = std::env::temp_dir().join(format!("estel-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.log");
        let mut log = LogFile::open(&path).unwrap();
        log.write_all(&vec![b'a'; MAX_BYTES as usize]).unwrap();
        log.write_all(b"panic details").unwrap();
        log.flush().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"panic details");
        assert_eq!(
            std::fs::metadata(dir.join("test.log.1")).unwrap().len(),
            MAX_BYTES
        );
        log.write_all(&vec![b'b'; MAX_BYTES as usize]).unwrap();
        assert_eq!(
            std::fs::read(dir.join("test.log.1")).unwrap(),
            b"panic details"
        );
        drop(log);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
