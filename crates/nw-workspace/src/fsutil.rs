//! Small shared filesystem helpers.

use std::io;
use std::path::Path;

/// Writes `text` to `target` via a sibling temp file and a rename, so readers
/// never observe a half-written file.
pub(crate) fn write_atomic(target: &Path, text: &str) -> io::Result<()> {
    let mut tmp_name = target.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = target.with_file_name(tmp_name);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
