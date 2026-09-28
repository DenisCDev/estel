//! Crash recovery for the display.
//!
//! A dirty flag is written after a successful snapshot and cleared only on a
//! clean restore. If the process is killed, the next launch sees the flag and
//! writes an identity gamma ramp *before* snapshotting — otherwise the warm
//! LUT would be saved as "original" and tray-quit would lock it in.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DdcOriginal {
    pub id: String,
    pub value: u32,
}

pub enum DdcSnapshot {
    Missing,
    Invalid,
    Named(Vec<DdcOriginal>),
    Legacy(Vec<u32>),
}

fn dir() -> PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("studio", "condado", "estel") {
        dirs.config_dir().to_path_buf()
    } else {
        PathBuf::from(".")
    }
}

fn dirty_path() -> PathBuf {
    dir().join("dirty")
}

fn ddc_path() -> PathBuf {
    dir().join("ddc_original")
}

pub fn is_dirty() -> bool {
    dirty_path().exists()
}

pub fn mark_dirty() {
    let path = dirty_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, b"1");
}

pub fn mark_clean() {
    let _ = std::fs::remove_file(dirty_path());
}

pub fn load_ddc_originals() -> DdcSnapshot {
    let Ok(text) = std::fs::read_to_string(ddc_path()) else {
        return DdcSnapshot::Missing;
    };
    if text.trim_start().starts_with('[') {
        match serde_json::from_str(&text) {
            Ok(values) => DdcSnapshot::Named(values),
            Err(error) => {
                tracing::warn!(%error, "registro de brilho DDC inválido; recuperação preservada");
                DdcSnapshot::Invalid
            }
        }
    } else {
        DdcSnapshot::Legacy(
            text.split(|c: char| c == ',' || c.is_whitespace())
                .filter_map(|part| part.parse().ok())
                .collect(),
        )
    }
}

pub fn save_ddc_originals(values: &[DdcOriginal]) -> bool {
    let path = ddc_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_vec(values) {
        Ok(bytes) => {
            if let Err(error) = std::fs::write(&path, bytes) {
                tracing::warn!(%error, "não foi possível salvar o brilho original dos monitores");
                return false;
            }
        }
        Err(error) => {
            tracing::warn!(%error, "não foi possível registrar o brilho original");
            return false;
        }
    }
    true
}

pub fn clear_ddc_original() {
    let _ = std::fs::remove_file(ddc_path());
}
