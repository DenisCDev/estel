//! Durable, identity-bound display recovery. A snapshot is committed before a
//! driver is allowed to change the display; unresolved devices remain recorded.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::color::GammaRamp;

const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DdcOriginal {
    pub id: String,
    pub value: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GammaOriginal {
    pub id: String,
    pub channels: Vec<Vec<u16>>,
}

impl GammaOriginal {
    pub fn new(id: String, ramp: &GammaRamp) -> Self {
        Self {
            id,
            channels: ramp.iter().map(|channel| channel.to_vec()).collect(),
        }
    }

    pub fn ramp(&self) -> Option<GammaRamp> {
        if self.channels.len() != 3 || self.channels.iter().any(|channel| channel.len() != 256) {
            return None;
        }
        let mut ramp = [[0; 256]; 3];
        for (dest, source) in ramp.iter_mut().zip(&self.channels) {
            dest.copy_from_slice(source);
        }
        Some(ramp)
    }
}

pub enum DdcSnapshot {
    Missing,
    Invalid,
    Named(Vec<DdcOriginal>),
    Legacy(Vec<u32>),
}

fn dir() -> PathBuf {
    crate::config::Config::config_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

pub fn is_dirty() -> bool {
    dir().join("dirty").exists()
}

pub fn mark_dirty() -> bool {
    if is_dirty() {
        return true;
    }
    match write_atomic(&dir().join("dirty"), b"2") {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(%error, "não foi possível registrar a recuperação da tela");
            false
        }
    }
}

pub fn legacy_gamma_missing() -> bool {
    is_dirty()
        && std::fs::read(dir().join("dirty")).map_or(true, |version| version != b"2")
        && !dir().join("gamma_original.json").exists()
}

fn read_bounded(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_SNAPSHOT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_SNAPSHOT_BYTES,
        "registro de recuperação grande demais"
    );
    Ok(bytes)
}

pub fn mark_clean() {
    remove_file(&dir().join("dirty"));
}

fn read_snapshot<T: DeserializeOwned>(name: &str) -> anyhow::Result<Option<T>> {
    let path = dir().join(name);
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.len() <= MAX_SNAPSHOT_BYTES,
        "registro de recuperação grande demais"
    );
    Ok(Some(serde_json::from_slice(&read_bounded(&path)?)?))
}

pub fn load_gamma_originals() -> anyhow::Result<Vec<GammaOriginal>> {
    let values: Vec<GammaOriginal> = read_snapshot("gamma_original.json")?.unwrap_or_default();
    anyhow::ensure!(
        unique_ids(values.iter().map(|value| value.id.as_str())),
        "identidades de gama inválidas"
    );
    anyhow::ensure!(
        values.iter().all(|value| value.ramp().is_some()),
        "rampa de recuperação inválida"
    );
    Ok(values)
}

pub fn load_ddc_originals() -> DdcSnapshot {
    let path = dir().join("ddc_original");
    if !path.exists() {
        return DdcSnapshot::Missing;
    }
    let bytes = match read_bounded(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, "não foi possível ler a recuperação de brilho");
            return DdcSnapshot::Invalid;
        }
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return DdcSnapshot::Invalid;
    };
    if text.trim_start().starts_with('[') {
        match serde_json::from_str::<Vec<DdcOriginal>>(text) {
            Ok(values) if unique_ids(values.iter().map(|value| value.id.as_str())) => {
                DdcSnapshot::Named(values)
            }
            _ => DdcSnapshot::Invalid,
        }
    } else {
        match text
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .map(str::parse)
            .collect::<Result<Vec<u32>, _>>()
        {
            Ok(values) if !values.is_empty() => DdcSnapshot::Legacy(values),
            _ => DdcSnapshot::Invalid,
        }
    }
}

pub fn save_gamma_originals(values: &[GammaOriginal]) -> bool {
    save_snapshot("gamma_original.json", &values)
}

pub fn save_ddc_originals(values: &[DdcOriginal]) -> bool {
    save_snapshot("ddc_original", &values)
}

fn save_snapshot(name: &str, values: &impl Serialize) -> bool {
    let result = serde_json::to_vec(values)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| write_atomic(&dir().join(name), &bytes));
    if let Err(error) = result {
        tracing::error!(%error, "não foi possível salvar os ajustes originais da tela");
        return false;
    }
    true
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = HashSet::new();
    ids.into_iter()
        .all(|id| !id.is_empty() && id.len() <= 1024 && seen.insert(id))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("registro sem diretório"))?;
    std::fs::create_dir_all(parent)?;
    crate::config::atomic_write(path, bytes)?;
    Ok(())
}

fn remove_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, "não foi possível limpar o registro de recuperação");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_snapshot_preserves_existing_calibration() {
        let mut ramp = crate::color::identity_ramp();
        ramp[1][230] = 58000;
        let serialized =
            serde_json::to_vec(&GammaOriginal::new("monitor-a".into(), &ramp)).unwrap();
        let restored: GammaOriginal = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(restored.ramp(), Some(ramp));
    }

    #[test]
    fn corrupt_gamma_cannot_be_used_for_recovery() {
        assert!(
            GammaOriginal {
                id: "a".into(),
                channels: vec![vec![0; 255]; 3]
            }
            .ramp()
            .is_none()
        );
        assert!(!unique_ids(["same", "same"].into_iter()));
        assert!(!unique_ids([""].into_iter()));
    }
}
