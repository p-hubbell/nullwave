use super::files::{hash_bytes, to_ron, write_atomic};
use crate::model::Project;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn recovery_path(dir: &Path) -> PathBuf {
    dir.join(".nullwave").join("recovery.ron")
}

/// Atomically writes the whole project to `.nullwave/recovery.ron`.
pub fn write_recovery(dir: &Path, project: &Project) -> io::Result<()> {
    write_atomic(&recovery_path(dir), to_ron(project).as_bytes())
}

/// `None` if there is no recovery file or it cannot be parsed.
pub fn read_recovery(dir: &Path) -> Option<Project> {
    let text = fs::read_to_string(recovery_path(dir)).ok()?;
    ron::from_str(&text).ok()
}

pub fn clear_recovery(dir: &Path) -> io::Result<()> {
    match fs::remove_file(recovery_path(dir)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn is_tracked(path: &Path) -> bool {
    if path.components().any(|c| c.as_os_str() == ".nullwave") {
        return false;
    }
    path.extension().is_some_and(|e| e == "ron")
        || path.file_name().is_some_and(|n| n == ".gitignore")
}

/// Returns the paths whose current content differs from what we last saved.
/// Paths are compared exactly as given. A saved file that is now missing counts as changed.
pub fn external_changes(
    paths: &[PathBuf],
    last_saved: &HashMap<PathBuf, u64>,
    read: impl Fn(&Path) -> Option<Vec<u8>>,
) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|p| is_tracked(p))
        .filter(|p| match (read(p), last_saved.get(*p)) {
            (Some(bytes), Some(h)) => hash_bytes(&bytes) != *h,
            (Some(_), None) => true,
            (None, saved) => saved.is_some(),
        })
        .cloned()
        .collect()
}
