//! Tracks which documents have unsaved changes.

use crate::TabKind;
use nw_core::DocPath;
use std::collections::BTreeSet;

#[derive(Default, Debug, Clone)]
pub struct DirtyTracker {
    paths: BTreeSet<DocPath>,
}

impl DirtyTracker {
    pub fn mark(&mut self, paths: impl IntoIterator<Item = DocPath>) {
        self.paths.extend(paths);
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }

    /// True if any document, including `Song`, is dirty.
    pub fn is_dirty(&self) -> bool {
        !self.paths.is_empty()
    }

    pub fn tab_is_dirty(&self, tab: &TabKind) -> bool {
        match tab {
            TabKind::Pattern(p) => self
                .paths
                .iter()
                .any(|d| matches!(d, DocPath::Notes(q, _) if q == p)),
            TabKind::PianoRoll(p, i) => self.paths.contains(&DocPath::Notes(p.clone(), i.clone())),
            TabKind::Instrument(i) => self.paths.contains(&DocPath::Instrument(i.clone())),
        }
    }
}
