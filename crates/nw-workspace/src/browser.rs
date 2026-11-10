//! Sample folder listing for the sidebar browser.

use std::io;
use std::path::{Path, PathBuf};

pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "aif", "aiff", "flac", "mp3", "ogg"];

#[derive(Clone, Debug, PartialEq)]
pub struct BrowserEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// Whether `path` has one of `AUDIO_EXTENSIONS` (case-insensitive).
pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// Directories first, then audio files, each sorted case-insensitively.
/// `Err` if `path` is missing or not a directory (e.g. an unmounted drive).
pub fn list_dir(path: &Path) -> io::Result<Vec<BrowserEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)?.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if name.starts_with('.') {
            continue;
        }
        let p = entry.path();
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        let is_dir = meta.is_dir();
        if is_dir || (meta.is_file() && is_audio(&p)) {
            entries.push(BrowserEntry { path: p, name, is_dir });
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}
