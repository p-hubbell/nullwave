use crate::model::{Instrument, InstrumentId, Note, Pattern, PatternId, Project};

/// The unit of dirtiness and of on-disk files.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DocPath {
    Song,
    Instrument(InstrumentId),
    Notes(PatternId, InstrumentId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    AddNotes { pattern: PatternId, instrument: InstrumentId, notes: Vec<Note> },
    RemoveNotes { pattern: PatternId, instrument: InstrumentId, notes: Vec<Note> },
    /// Move, resize, velocity.
    ReplaceNotes { pattern: PatternId, instrument: InstrumentId, before: Vec<Note>, after: Vec<Note> },
    AddInstrument { instrument: Instrument, index: usize },
    RemoveInstrument { instrument: Instrument, index: usize, notes: Vec<(PatternId, Vec<Note>)> },
    /// Inverse of `RemoveInstrument`: re-adds the instrument together with its notes.
    RestoreInstrument { instrument: Instrument, index: usize, notes: Vec<(PatternId, Vec<Note>)> },
    UpdateInstrument { before: Instrument, after: Instrument },
    AddPattern { pattern: Pattern, index: usize },
    RemovePattern { pattern: Pattern, index: usize },
    RenamePattern { id: PatternId, before: String, after: String },
    SetTempo { before: f64, after: f64 },
}

fn add_notes(project: &mut Project, pattern: &PatternId, instrument: &InstrumentId, notes: &[Note]) {
    if notes.is_empty() {
        return;
    }
    if let Some(p) = project.pattern_mut(pattern) {
        let list = p.notes.entry(instrument.clone()).or_default();
        list.extend(notes.iter().cloned());
        list.sort_by_key(|n| (n.start, n.pitch));
    }
}

fn remove_notes(project: &mut Project, pattern: &PatternId, instrument: &InstrumentId, notes: &[Note]) {
    if let Some(p) = project.pattern_mut(pattern) {
        if let Some(list) = p.notes.get_mut(instrument) {
            for n in notes {
                if let Some(i) = list.iter().position(|x| x == n) {
                    list.remove(i);
                }
            }
            if list.is_empty() {
                p.notes.remove(instrument);
            }
        }
    }
}

fn insert_instrument(project: &mut Project, instrument: &Instrument, index: usize, notes: &[(PatternId, Vec<Note>)]) {
    let index = index.min(project.instruments.len());
    project.instruments.insert(index, instrument.clone());
    for (pattern, list) in notes {
        add_notes(project, pattern, &instrument.id, list);
    }
}

impl Edit {
    pub fn apply(&self, project: &mut Project) {
        match self {
            Edit::AddNotes { pattern, instrument, notes } => add_notes(project, pattern, instrument, notes),
            Edit::RemoveNotes { pattern, instrument, notes } => remove_notes(project, pattern, instrument, notes),
            Edit::ReplaceNotes { pattern, instrument, before, after } => {
                remove_notes(project, pattern, instrument, before);
                add_notes(project, pattern, instrument, after);
            }
            Edit::AddInstrument { instrument, index } => insert_instrument(project, instrument, *index, &[]),
            Edit::RestoreInstrument { instrument, index, notes } => {
                insert_instrument(project, instrument, *index, notes)
            }
            Edit::RemoveInstrument { instrument, .. } => {
                project.instruments.retain(|i| i.id != instrument.id);
                for p in &mut project.patterns {
                    p.notes.remove(&instrument.id);
                }
            }
            Edit::UpdateInstrument { before, after } => {
                if let Some(i) = project.instrument_mut(&before.id) {
                    *i = after.clone();
                }
            }
            Edit::AddPattern { pattern, index } => {
                let index = (*index).min(project.patterns.len());
                project.patterns.insert(index, pattern.clone());
            }
            Edit::RemovePattern { pattern, .. } => project.patterns.retain(|p| p.id != pattern.id),
            Edit::RenamePattern { id, after, .. } => {
                if let Some(p) = project.pattern_mut(id) {
                    p.name = after.clone();
                }
            }
            Edit::SetTempo { after, .. } => project.tempo = *after,
        }
    }

    pub fn inverse(&self) -> Edit {
        match self.clone() {
            Edit::AddNotes { pattern, instrument, notes } => Edit::RemoveNotes { pattern, instrument, notes },
            Edit::RemoveNotes { pattern, instrument, notes } => Edit::AddNotes { pattern, instrument, notes },
            Edit::ReplaceNotes { pattern, instrument, before, after } => {
                Edit::ReplaceNotes { pattern, instrument, before: after, after: before }
            }
            Edit::AddInstrument { instrument, index } => {
                Edit::RemoveInstrument { instrument, index, notes: Vec::new() }
            }
            Edit::RemoveInstrument { instrument, index, notes } => {
                Edit::RestoreInstrument { instrument, index, notes }
            }
            Edit::RestoreInstrument { instrument, index, notes } => {
                Edit::RemoveInstrument { instrument, index, notes }
            }
            Edit::UpdateInstrument { before, after } => Edit::UpdateInstrument { before: after, after: before },
            Edit::AddPattern { pattern, index } => Edit::RemovePattern { pattern, index },
            Edit::RemovePattern { pattern, index } => Edit::AddPattern { pattern, index },
            Edit::RenamePattern { id, before, after } => Edit::RenamePattern { id, before: after, after: before },
            Edit::SetTempo { before, after } => Edit::SetTempo { before: after, after: before },
        }
    }

    /// Documents this edit changes; `Song` first, then others in deterministic order.
    pub fn touched(&self) -> Vec<DocPath> {
        match self {
            Edit::AddNotes { pattern, instrument, .. }
            | Edit::RemoveNotes { pattern, instrument, .. }
            | Edit::ReplaceNotes { pattern, instrument, .. } => {
                vec![DocPath::Notes(pattern.clone(), instrument.clone())]
            }
            Edit::AddInstrument { instrument, .. } => {
                vec![DocPath::Song, DocPath::Instrument(instrument.id.clone())]
            }
            Edit::RemoveInstrument { instrument, notes, .. }
            | Edit::RestoreInstrument { instrument, notes, .. } => {
                let mut v = vec![DocPath::Song, DocPath::Instrument(instrument.id.clone())];
                v.extend(
                    notes
                        .iter()
                        .filter(|(_, n)| !n.is_empty())
                        .map(|(p, _)| DocPath::Notes(p.clone(), instrument.id.clone())),
                );
                v
            }
            Edit::UpdateInstrument { before, .. } => vec![DocPath::Instrument(before.id.clone())],
            Edit::AddPattern { pattern, .. } | Edit::RemovePattern { pattern, .. } => {
                let mut v = vec![DocPath::Song];
                v.extend(pattern.notes.iter().filter(|(_, n)| !n.is_empty()).map(|(i, _)| {
                    DocPath::Notes(pattern.id.clone(), i.clone())
                }));
                v
            }
            Edit::RenamePattern { .. } | Edit::SetTempo { .. } => vec![DocPath::Song],
        }
    }
}

/// Builds a `RemoveInstrument` capturing the instrument's notes in every pattern.
pub fn remove_instrument_edit(project: &Project, id: &InstrumentId) -> Option<Edit> {
    let index = project.instrument_index(id)?;
    let notes = project
        .patterns
        .iter()
        .filter_map(|p| p.notes.get(id).filter(|n| !n.is_empty()).map(|n| (p.id.clone(), n.clone())))
        .collect();
    Some(Edit::RemoveInstrument { instrument: project.instruments[index].clone(), index, notes })
}
