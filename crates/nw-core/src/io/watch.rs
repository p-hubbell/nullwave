use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(200);

/// Watches a project directory recursively and reports changed paths in debounced batches.
/// Watching stops when this value is dropped.
pub struct ProjectWatcher {
    _watcher: RecommendedWatcher,
}

impl ProjectWatcher {
    pub fn new(
        dir: &Path,
        on_change: impl Fn(Vec<PathBuf>) + Send + 'static,
    ) -> notify::Result<ProjectWatcher> {
        // Event paths may be canonical (e.g. /private/var on macOS), so compare against that.
        let root = std::fs::canonicalize(dir).map_err(notify::Error::io)?;
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let filter_root = root.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            for p in event.paths {
                if p.starts_with(&filter_root) {
                    let _ = tx.send(p);
                }
            }
        })?;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        // Ends when the watcher (and with it the sender) is dropped.
        std::thread::spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut batch = BTreeSet::from([first]);
                loop {
                    match rx.recv_timeout(DEBOUNCE) {
                        Ok(p) => {
                            batch.insert(p);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                on_change(batch.into_iter().collect());
            }
        });
        Ok(ProjectWatcher { _watcher: watcher })
    }
}
