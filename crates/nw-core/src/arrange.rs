//! Arrangement operations. Each returns the edits for one undo step (empty when nothing
//! would change); the caller commits them as one group.

use crate::clip::{resize_end, trim_start};
use crate::edit::Edit;
use crate::model::{
    AudioLane, AudioLaneId, AudioPlacement, Clip, ClipKey, ClipRef, Container, InstrumentId, Note, Pattern,
    PatternId, PatternPlacement, Placement, Project, Timeline,
};
use crate::naming::{new_audio_lane, next_pattern};
use crate::time::Tick;
use std::collections::BTreeMap;
use std::path::Path;

const NEW_AUDIO_VOLUME: f32 = 0.78;

fn timeline<'a>(p: &'a Project, c: &Container) -> &'a Timeline {
    static EMPTY: Timeline = Timeline { clips: Vec::new(), placements: Vec::new() };
    p.timeline(c).unwrap_or(&EMPTY)
}

fn clip_length(t: &Timeline, key: &ClipKey) -> Tick {
    t.clip(key).map_or(0, |c| c.length)
}

/// A new empty clip for `instrument` with the next free number, placed once at `at`.
pub fn new_clip(p: &Project, c: &Container, instrument: &InstrumentId, at: Tick, length: Tick) -> (Vec<Edit>, ClipKey) {
    let key = ClipKey { instrument: instrument.clone(), number: timeline(p, c).next_number(instrument) };
    let edits = vec![
        Edit::ReplaceClip { container: c.clone(), before: None, after: Some(Clip::new(key.clone(), length.max(1))) },
        Edit::ReplacePlacements {
            container: c.clone(),
            before: Vec::new(),
            after: vec![Placement { clip: key.clone(), at }],
        },
    ];
    (edits, key)
}

pub fn move_placements(_p: &Project, c: &Container, moves: &[(Placement, Tick)]) -> Vec<Edit> {
    let moves: Vec<&(Placement, Tick)> = moves.iter().filter(|(pl, to)| pl.at != *to).collect();
    if moves.is_empty() {
        return Vec::new();
    }
    vec![Edit::ReplacePlacements {
        container: c.clone(),
        before: moves.iter().map(|(pl, _)| pl.clone()).collect(),
        after: moves.iter().map(|(pl, to)| Placement { clip: pl.clip.clone(), at: *to }).collect(),
    }]
}

/// Removes the blocks, and any clip left with none.
pub fn delete_placements(p: &Project, c: &Container, targets: &[Placement]) -> Vec<Edit> {
    if targets.is_empty() {
        return Vec::new();
    }
    let t = timeline(p, c);
    let mut removed: BTreeMap<&ClipKey, usize> = BTreeMap::new();
    for pl in targets {
        *removed.entry(&pl.clip).or_default() += 1;
    }
    let mut edits =
        vec![Edit::ReplacePlacements { container: c.clone(), before: targets.to_vec(), after: Vec::new() }];
    for (key, n) in removed {
        if t.uses(key) <= n
            && let Some(clip) = t.clip(key)
        {
            edits.push(Edit::ReplaceClip { container: c.clone(), before: Some(clip.clone()), after: None });
        }
    }
    edits
}

/// Copies of the blocks (same clips) placed right after the selection's span.
pub fn duplicate_placements(p: &Project, c: &Container, targets: &[Placement]) -> (Vec<Edit>, Vec<Placement>) {
    let t = timeline(p, c);
    let Some(start) = targets.iter().map(|pl| pl.at).min() else { return (Vec::new(), Vec::new()) };
    let end = targets.iter().map(|pl| pl.at + clip_length(t, &pl.clip)).max().unwrap_or(start);
    let span = (end - start).max(1);
    let mut copies: Vec<Placement> =
        targets.iter().map(|pl| Placement { clip: pl.clip.clone(), at: pl.at + span }).collect();
    copies.sort_by_key(|pl| (pl.at, pl.clip.clone()));
    let edits = vec![Edit::ReplacePlacements { container: c.clone(), before: Vec::new(), after: copies.clone() }];
    (edits, copies)
}

/// Gives one block its own copy of the clip under the next free number.
pub fn make_unique(p: &Project, c: &Container, target: &Placement) -> (Vec<Edit>, ClipKey) {
    let t = timeline(p, c);
    let Some(old) = t.clip(&target.clip) else { return (Vec::new(), target.clip.clone()) };
    let key = ClipKey { instrument: target.clip.instrument.clone(), number: t.next_number(&target.clip.instrument) };
    let mut copy = old.clone();
    copy.key = key.clone();
    let mut edits = vec![
        Edit::ReplaceClip { container: c.clone(), before: None, after: Some(copy) },
        Edit::ReplacePlacements {
            container: c.clone(),
            before: vec![target.clone()],
            after: vec![Placement { clip: key.clone(), at: target.at }],
        },
    ];
    if t.uses(&target.clip) <= 1 {
        edits.push(Edit::ReplaceClip { container: c.clone(), before: Some(old.clone()), after: None });
    }
    (edits, key)
}

fn replace_clip(r: &ClipRef, before: &Clip, after: Clip) -> Vec<Edit> {
    if &after == before {
        return Vec::new();
    }
    vec![Edit::ReplaceClip { container: r.container.clone(), before: Some(before.clone()), after: Some(after) }]
}

pub fn resize_clip(p: &Project, r: &ClipRef, length: Tick) -> Vec<Edit> {
    match timeline(p, &r.container).clip(&r.key) {
        Some(clip) => replace_clip(r, clip, resize_end(clip, length)),
        None => Vec::new(),
    }
}

/// Trims the clip's start by `delta`; every block of it moves by the applied amount so
/// the notes stay put. Blocks never move before tick 0.
pub fn trim_clip_start(p: &Project, r: &ClipRef, delta: i64) -> Vec<Edit> {
    let t = timeline(p, &r.container);
    let Some(clip) = t.clip(&r.key) else { return Vec::new() };
    let blocks: Vec<Placement> = t.placements.iter().filter(|pl| pl.clip == r.key).cloned().collect();
    let earliest = blocks.iter().map(|pl| i64::from(pl.at)).min().unwrap_or(0);
    let (trimmed, applied) = trim_start(clip, delta.max(-earliest));
    if applied == 0 {
        return Vec::new();
    }
    let moved = blocks.iter().map(|pl| Placement { clip: pl.clip.clone(), at: (i64::from(pl.at) + applied) as Tick });
    let mut edits = replace_clip(r, clip, trimmed);
    edits.push(Edit::ReplacePlacements { container: r.container.clone(), before: blocks.clone(), after: moved.collect() });
    edits
}

pub fn edit_clip_notes(p: &Project, r: &ClipRef, mut notes: Vec<Note>) -> Vec<Edit> {
    let Some(clip) = timeline(p, &r.container).clip(&r.key) else { return Vec::new() };
    notes.sort_by_key(|n| (n.start, n.pitch));
    replace_clip(r, clip, Clip { notes, ..clip.clone() })
}

/// An independent copy of the pattern, named "Pattern N", added at the end.
pub fn duplicate_pattern(p: &Project, id: &PatternId) -> (Vec<Edit>, PatternId) {
    let Some(source) = p.pattern(id) else { return (Vec::new(), id.clone()) };
    let (new_id, name) = next_pattern(p);
    let pattern = Pattern { id: new_id.clone(), name, timeline: source.timeline.clone() };
    (vec![Edit::AddPattern { pattern, index: p.patterns.len() }], new_id)
}

pub fn place_pattern(_p: &Project, id: &PatternId, at: Tick) -> Vec<Edit> {
    vec![Edit::ReplacePatternPlacements {
        before: Vec::new(),
        after: vec![PatternPlacement { pattern: id.clone(), at }],
    }]
}

pub fn move_pattern_placements(_p: &Project, moves: &[(PatternPlacement, Tick)]) -> Vec<Edit> {
    let moves: Vec<&(PatternPlacement, Tick)> = moves.iter().filter(|(pp, to)| pp.at != *to).collect();
    if moves.is_empty() {
        return Vec::new();
    }
    vec![Edit::ReplacePatternPlacements {
        before: moves.iter().map(|(pp, _)| pp.clone()).collect(),
        after: moves.iter().map(|(pp, to)| PatternPlacement { pattern: pp.pattern.clone(), at: *to }).collect(),
    }]
}

pub fn delete_pattern_placements(_p: &Project, targets: &[PatternPlacement]) -> Vec<Edit> {
    if targets.is_empty() {
        return Vec::new();
    }
    vec![Edit::ReplacePatternPlacements { before: targets.to_vec(), after: Vec::new() }]
}

/// Copies of the placed patterns right after the selection's span.
pub fn duplicate_pattern_placements(p: &Project, targets: &[PatternPlacement]) -> (Vec<Edit>, Vec<PatternPlacement>) {
    let Some(start) = targets.iter().map(|pp| pp.at).min() else { return (Vec::new(), Vec::new()) };
    let end = targets.iter().map(|pp| pp.at + p.pattern_length(&pp.pattern)).max().unwrap_or(start);
    let span = end - start;
    let mut copies: Vec<PatternPlacement> =
        targets.iter().map(|pp| PatternPlacement { pattern: pp.pattern.clone(), at: pp.at + span }).collect();
    copies.sort_by_key(|pp| (pp.at, pp.pattern.clone()));
    (vec![Edit::ReplacePatternPlacements { before: Vec::new(), after: copies.clone() }], copies)
}

/// Removes the pattern and every place it is used in the song.
pub fn delete_pattern(p: &Project, id: &PatternId) -> Vec<Edit> {
    let Some(index) = p.patterns.iter().position(|pt| &pt.id == id) else { return Vec::new() };
    let placed: Vec<PatternPlacement> = p.song.patterns.iter().filter(|pp| &pp.pattern == id).cloned().collect();
    let mut edits = Vec::new();
    if !placed.is_empty() {
        edits.push(Edit::ReplacePatternPlacements { before: placed, after: Vec::new() });
    }
    edits.push(Edit::RemovePattern { pattern: p.patterns[index].clone(), index });
    edits
}

/// Places the file at `at`, on its existing lane or a new one.
pub fn add_audio(p: &Project, path: &Path, at: Tick) -> Vec<Edit> {
    let mut edits = Vec::new();
    let lane = match p.song.audio_lanes.iter().find(|l| l.path == path) {
        Some(l) => l.id.clone(),
        None => {
            let id = new_audio_lane(p, path);
            let lane = AudioLane { id: id.clone(), path: path.to_path_buf(), volume: NEW_AUDIO_VOLUME };
            edits.push(Edit::AddAudioLane { lane, index: p.song.audio_lanes.len() });
            id
        }
    };
    edits.push(Edit::ReplaceAudioPlacements { before: Vec::new(), after: vec![AudioPlacement { lane, at }] });
    edits
}

pub fn move_audio(_p: &Project, moves: &[(AudioPlacement, Tick)]) -> Vec<Edit> {
    let moves: Vec<&(AudioPlacement, Tick)> = moves.iter().filter(|(a, to)| a.at != *to).collect();
    if moves.is_empty() {
        return Vec::new();
    }
    vec![Edit::ReplaceAudioPlacements {
        before: moves.iter().map(|(a, _)| a.clone()).collect(),
        after: moves.iter().map(|(a, to)| AudioPlacement { lane: a.lane.clone(), at: *to }).collect(),
    }]
}

/// Removes the audio blocks, and any lane left with none.
pub fn delete_audio(p: &Project, targets: &[AudioPlacement]) -> Vec<Edit> {
    if targets.is_empty() {
        return Vec::new();
    }
    let mut edits = vec![Edit::ReplaceAudioPlacements { before: targets.to_vec(), after: Vec::new() }];
    // Highest index first, so undo re-inserts each lane at its original index.
    for (index, lane) in p.song.audio_lanes.iter().enumerate().rev() {
        let used = p.song.audio.iter().filter(|a| a.lane == lane.id).count();
        let removed = targets.iter().filter(|a| a.lane == lane.id).count();
        if removed > 0 && used <= removed {
            edits.push(Edit::RemoveAudioLane { lane: lane.clone(), index });
        }
    }
    edits
}

pub fn set_audio_volume(p: &Project, lane: &AudioLaneId, volume: f32) -> Vec<Edit> {
    let Some(before) = p.song.audio_lanes.iter().find(|l| &l.id == lane) else { return Vec::new() };
    let after = AudioLane { volume: volume.clamp(0.0, 1.0), ..before.clone() };
    if after == *before {
        return Vec::new();
    }
    vec![Edit::UpdateAudioLane { before: before.clone(), after }]
}
