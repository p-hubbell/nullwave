//! Tracks which documents have unsaved changes.

use crate::TabKind;
use nw_core::{Container, DocPath};
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
            TabKind::Pattern(p) | TabKind::PianoRoll(p, _) => self.paths.iter().any(|d| match d {
                DocPath::PatternLayout(q) => q == p,
                DocPath::Clip(r) => matches!(&r.container, Container::Pattern(q) if q == p),
                _ => false,
            }),
            TabKind::Song => self.paths.iter().any(|d| match d {
                DocPath::Arrangement => true,
                DocPath::Clip(r) => r.container == Container::Song,
                _ => false,
            }),
            TabKind::Instrument(i) => self.paths.contains(&DocPath::Instrument(i.clone())),
        }
    }
}
