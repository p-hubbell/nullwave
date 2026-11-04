//! App-wide settings stored outside any project.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AppSettings {
    pub sample_folders: Vec<PathBuf>,
    pub last_project: Option<PathBuf>,
}

/// `~/Library/Application Support/Nullwave` on macOS; holds `settings.ron` and `keymap.ron`.
pub fn settings_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("Nullwave")
}

/// Missing or corrupt files yield the default; never panics.
pub fn load_settings(path: &Path) -> AppSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| ron::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_settings(path: &Path, settings: &AppSettings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::new())
        .map_err(io::Error::other)?;
    crate::fsutil::write_atomic(path, &text)
}
