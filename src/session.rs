//! Crash recovery for the display.
//!
//! A dirty flag is written after a successful snapshot and cleared only on a
//! clean restore. If the process is killed, the next launch sees the flag and
//! writes an identity gamma ramp *before* snapshotting — otherwise the warm
//! LUT would be saved as "original" and tray-quit would lock it in.

use std::path::PathBuf;

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

pub fn load_ddc_original() -> Option<u32> {
    std::fs::read_to_string(ddc_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn save_ddc_original(value: u32) {
    let path = ddc_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, value.to_string());
}

pub fn clear_ddc_original() {
    let _ = std::fs::remove_file(ddc_path());
}
