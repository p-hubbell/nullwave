use crate::model::{
    AudioLane, AudioPlacement, Clip, ClipRef, Container, Instrument, InstrumentId, Pattern, PatternId,
    PatternPlacement, Placement, Project, Timeline,
};

/// The unit of dirtiness and of on-disk files.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DocPath {
    /// `song.ron`: tempo, time signature, instrument and pattern lists.
    Song,
    Instrument(InstrumentId),
    /// A pattern's placements.
    PatternLayout(PatternId),
    /// The song's placements and audio lanes.
    Arrangement,
    /// One clip's notes, length and start.
    Clip(ClipRef),
}

/// What an instrument had on one timeline when it was removed.
pub type RemovedClips = Vec<(Container, Vec<Clip>, Vec<Placement>)>;

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Adds (`before: None`), changes, or removes (`after: None`) one clip.
    ReplaceClip { container: Container, before: Option<Clip>, after: Option<Clip> },
    ReplacePlacements { container: Container, before: Vec<Placement>, after: Vec<Placement> },
    ReplacePatternPlacements { before: Vec<PatternPlacement>, after: Vec<PatternPlacement> },
    ReplaceAudioPlacements { before: Vec<AudioPlacement>, after: Vec<AudioPlacement> },
    AddAudioLane { lane: AudioLane, index: usize },
    RemoveAudioLane { lane: AudioLane, index: usize },
    UpdateAudioLane { before: AudioLane, after: AudioLane },
    // Instruments are boxed: a sampler's settings are large, and every undo step
    // (including note edits) would otherwise be sized for them.
    AddInstrument { instrument: Box<Instrument>, index: usize },
    RemoveInstrument { instrument: Box<Instrument>, index: usize, removed: RemovedClips },
    /// Inverse of `RemoveInstrument`: re-adds the instrument with its clips and placements.
    RestoreInstrument { instrument: Box<Instrument>, index: usize, removed: RemovedClips },
    UpdateInstrument { before: Box<Instrument>, after: Box<Instrument> },
    AddPattern { pattern: Pattern, index: usize },
    RemovePattern { pattern: Pattern, index: usize },
    RenamePattern { id: PatternId, before: String, after: String },
    SetTempo { before: f64, after: f64 },
}

/// Removes one instance per item of `items` (by equality), then inserts `add` and sorts.
fn replace_sorted<T: PartialEq + Clone, K: Ord>(list: &mut Vec<T>, remove: &[T], add: &[T], key: impl Fn(&T) -> K) {
    for r in remove {
        if let Some(i) = list.iter().position(|x| x == r) {
            list.remove(i);
        }
    }
    list.extend(add.iter().cloned());
    list.sort_by_key(key);
}

fn replace_clip(timeline: &mut Timeline, before: &Option<Clip>, after: &Option<Clip>) {
    if let Some(b) = before {
        timeline.clips.retain(|c| c.key != b.key);
    }
    if let Some(a) = after {
        timeline.clips.retain(|c| c.key != a.key);
        let mut a = a.clone();
        a.notes.sort_by_key(|n| (n.start, n.pitch));
        timeline.clips.push(a);
        timeline.clips.sort_by(|x, y| x.key.cmp(&y.key));
    }
}

fn replace_placements(timeline: &mut Timeline, before: &[Placement], after: &[Placement]) {
    replace_sorted(&mut timeline.placements, before, after, |p| (p.at, p.clip.clone()));
}

fn insert_instrument(project: &mut Project, instrument: &Instrument, index: usize, removed: &RemovedClips) {
    let index = index.min(project.instruments.len());
    project.instruments.insert(index, instrument.clone());
    for (container, clips, placements) in removed {
        if let Some(t) = project.timeline_mut(container) {
            for c in clips {
                replace_clip(t, &None, &Some(c.clone()));
            }
            replace_placements(t, &[], placements);
        }
    }
}

impl Edit {
    pub fn apply(&self, project: &mut Project) {
        match self {
            Edit::ReplaceClip { container, before, after } => {
                if let Some(t) = project.timeline_mut(container) {
                    replace_clip(t, before, after);
                }
            }
            Edit::ReplacePlacements { container, before, after } => {
                if let Some(t) = project.timeline_mut(container) {
                    replace_placements(t, before, after);
                }
            }
            Edit::ReplacePatternPlacements { before, after } => {
                replace_sorted(&mut project.song.patterns, before, after, |p| (p.at, p.pattern.clone()))
            }
            Edit::ReplaceAudioPlacements { before, after } => {
                replace_sorted(&mut project.song.audio, before, after, |p| (p.at, p.lane.clone()))
            }
            Edit::AddAudioLane { lane, index } => {
                let index = (*index).min(project.song.audio_lanes.len());
                project.song.audio_lanes.insert(index, lane.clone());
            }
            Edit::RemoveAudioLane { lane, .. } => project.song.audio_lanes.retain(|l| l.id != lane.id),
            Edit::UpdateAudioLane { before, after } => {
                if let Some(l) = project.song.audio_lanes.iter_mut().find(|l| l.id == before.id) {
                    *l = after.clone();
                }
            }
            Edit::AddInstrument { instrument, index } => insert_instrument(project, instrument, *index, &Vec::new()),
            Edit::RestoreInstrument { instrument, index, removed } => {
                insert_instrument(project, instrument, *index, removed)
            }
            Edit::RemoveInstrument { instrument, .. } => {
                project.instruments.retain(|i| i.id != instrument.id);
                let id = &instrument.id;
                let timelines =
                    project.patterns.iter_mut().map(|p| &mut p.timeline).chain([&mut project.song.timeline]);
                for t in timelines {
                    t.clips.retain(|c| &c.key.instrument != id);
                    t.placements.retain(|p| &p.clip.instrument != id);
                }
            }
            Edit::UpdateInstrument { before, after } => {
                if let Some(i) = project.instrument_mut(&before.id) {
                    *i = (**after).clone();
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
            Edit::ReplaceClip { container, before, after } => Edit::ReplaceClip { container, before: after, after: before },
            Edit::ReplacePlacements { container, before, after } => {
                Edit::ReplacePlacements { container, before: after, after: before }
            }
            Edit::ReplacePatternPlacements { before, after } => {
                Edit::ReplacePatternPlacements { before: after, after: before }
            }
            Edit::ReplaceAudioPlacements { before, after } => Edit::ReplaceAudioPlacements { before: after, after: before },
            Edit::AddAudioLane { lane, index } => Edit::RemoveAudioLane { lane, index },
            Edit::RemoveAudioLane { lane, index } => Edit::AddAudioLane { lane, index },
            Edit::UpdateAudioLane { before, after } => Edit::UpdateAudioLane { before: after, after: before },
            Edit::AddInstrument { instrument, index } => {
                Edit::RemoveInstrument { instrument, index, removed: Vec::new() }
            }
            Edit::RemoveInstrument { instrument, index, removed } => {
                Edit::RestoreInstrument { instrument, index, removed }
            }
            Edit::RestoreInstrument { instrument, index, removed } => {
                Edit::RemoveInstrument { instrument, index, removed }
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
        let layout = |c: &Container| match c {
            Container::Pattern(p) => DocPath::PatternLayout(p.clone()),
            Container::Song => DocPath::Arrangement,
        };
        let clip_doc = |c: &Container, clip: &Clip| {
            DocPath::Clip(ClipRef { container: c.clone(), key: clip.key.clone() })
        };
        match self {
            Edit::ReplaceClip { container, before, after } => {
                let mut v: Vec<DocPath> =
                    before.iter().chain(after.iter()).map(|c| clip_doc(container, c)).collect();
                v.dedup();
                v
            }
            Edit::ReplacePlacements { container, .. } => vec![layout(container)],
            Edit::ReplacePatternPlacements { .. }
            | Edit::ReplaceAudioPlacements { .. }
            | Edit::AddAudioLane { .. }
            | Edit::RemoveAudioLane { .. }
            | Edit::UpdateAudioLane { .. } => vec![DocPath::Arrangement],
            Edit::AddInstrument { instrument, .. } => {
                vec![DocPath::Song, DocPath::Instrument(instrument.id.clone())]
            }
            Edit::RemoveInstrument { instrument, removed, .. }
            | Edit::RestoreInstrument { instrument, removed, .. } => {
                let mut v = vec![DocPath::Song, DocPath::Instrument(instrument.id.clone())];
                let mut rest: Vec<DocPath> = Vec::new();
                for (container, clips, placements) in removed {
                    if !placements.is_empty() {
                        rest.push(layout(container));
                    }
                    rest.extend(clips.iter().map(|c| clip_doc(container, c)));
                }
                rest.sort();
                rest.dedup();
                v.extend(rest);
                v
            }
            Edit::UpdateInstrument { before, .. } => vec![DocPath::Instrument(before.id.clone())],
            Edit::AddPattern { pattern, .. } | Edit::RemovePattern { pattern, .. } => {
                let container = Container::Pattern(pattern.id.clone());
                let mut v = vec![DocPath::Song, DocPath::PatternLayout(pattern.id.clone())];
                v.extend(pattern.timeline.clips.iter().map(|c| clip_doc(&container, c)));
                v
            }
            Edit::RenamePattern { .. } | Edit::SetTempo { .. } => vec![DocPath::Song],
        }
    }
}

/// Builds a `RemoveInstrument` capturing the instrument's clips and placements in every
/// pattern and the song.
pub fn remove_instrument_edit(project: &Project, id: &InstrumentId) -> Option<Edit> {
    let index = project.instrument_index(id)?;
    let containers =
        project.patterns.iter().map(|p| Container::Pattern(p.id.clone())).chain([Container::Song]);
    let removed = containers
        .filter_map(|c| {
            let t = project.timeline(&c)?;
            let clips: Vec<Clip> = t.clips.iter().filter(|x| &x.key.instrument == id).cloned().collect();
            let placements: Vec<Placement> =
                t.placements.iter().filter(|x| &x.clip.instrument == id).cloned().collect();
            (!clips.is_empty() || !placements.is_empty()).then_some((c, clips, placements))
        })
        .collect();
    Some(Edit::RemoveInstrument { instrument: Box::new(project.instruments[index].clone()), index, removed })
}
