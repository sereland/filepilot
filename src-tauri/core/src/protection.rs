//! Persisted state for restored files and whole directories; no Tauri dependency.
use crate::{archive, directory, oplog};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Serialize, Deserialize)]
pub struct ProtectedFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory_state: Option<String>,
}

pub fn protection(path: &Path) -> Option<ProtectedFile> {
    archive::no_links(path).ok()?;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() && !meta.is_dir() {
        return None;
    }
    Some(ProtectedFile {
        path: path.to_path_buf(),
        size: meta.len(),
        modified: meta.modified().ok()?,
        directory_state: if meta.is_dir() {
            Some(directory::state(path).ok()?)
        } else {
            None
        },
    })
}

pub fn unchanged(file: &ProtectedFile) -> bool {
    protection(&file.path)
        .map(|now| {
            now.size == file.size
                && now.modified == file.modified
                && now.directory_state == file.directory_state
        })
        .unwrap_or(false)
}

pub fn protect_restored(path: &Path, files: &mut Vec<ProtectedFile>) {
    if let Some(file) = protection(path) {
        files.retain(|old| !oplog::paths_equal(&old.path, path));
        files.push(file);
    }
}
