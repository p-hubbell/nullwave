use crate::edit::{DocPath, Edit};
use crate::model::Project;
use std::collections::BTreeSet;

fn touched_all<'a>(edits: impl Iterator<Item = &'a Edit>) -> Vec<DocPath> {
    edits.flat_map(Edit::touched).collect::<BTreeSet<_>>().into_iter().collect()
}

/// Undo/redo stacks of edit groups; one group is one undo step.
#[derive(Default)]
pub struct History {
    undo: Vec<Vec<Edit>>,
    redo: Vec<Vec<Edit>>,
}

impl History {
    pub fn new() -> History {
        History::default()
    }

    /// Applies the group, clears redo, returns touched documents.
    pub fn commit(&mut self, project: &mut Project, edits: Vec<Edit>) -> Vec<DocPath> {
        if edits.is_empty() {
            return Vec::new();
        }
        for e in &edits {
            e.apply(project);
        }
        let touched = touched_all(edits.iter());
        self.undo.push(edits);
        self.redo.clear();
        touched
    }

    pub fn undo(&mut self, project: &mut Project) -> Option<Vec<DocPath>> {
        let group = self.undo.pop()?;
        let inverses: Vec<Edit> = group.iter().rev().map(Edit::inverse).collect();
        for e in &inverses {
            e.apply(project);
        }
        let touched = touched_all(inverses.iter());
        self.redo.push(group);
        Some(touched)
    }

    pub fn redo(&mut self, project: &mut Project) -> Option<Vec<DocPath>> {
        let group = self.redo.pop()?;
        for e in &group {
            e.apply(project);
        }
        let touched = touched_all(group.iter());
        self.undo.push(group);
        Some(touched)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
