//! Turns timelines into the flat, sorted note lists the engine plays.

use crate::model::{AudioLaneId, InstrumentId, PatternId, Project, Timeline};
use crate::time::Tick;

/// One note to play, in timeline ticks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatNote {
    pub tick: Tick,
    pub instrument: InstrumentId,
    pub pitch: u8,
    pub velocity: u8,
    pub length: Tick,
}

fn timeline_notes(t: &Timeline, offset: Tick, out: &mut Vec<FlatNote>) {
    for placement in &t.placements {
        let Some(clip) = t.clip(&placement.clip) else { continue };
        out.extend(clip.audible().map(|n| FlatNote {
            tick: offset + placement.at + n.start,
            instrument: clip.key.instrument.clone(),
            pitch: n.pitch,
            velocity: n.velocity,
            length: n.length,
        }));
    }
}

fn sorted(mut notes: Vec<FlatNote>) -> Vec<FlatNote> {
    notes.sort_by(|a, b| (a.tick, &a.instrument, a.pitch).cmp(&(b.tick, &b.instrument, b.pitch)));
    notes
}

/// Every note the pattern plays, once through.
pub fn pattern_notes(p: &Project, id: &PatternId) -> Vec<FlatNote> {
    let mut out = Vec::new();
    if let Some(pattern) = p.pattern(id) {
        timeline_notes(&pattern.timeline, 0, &mut out);
    }
    sorted(out)
}

/// Every note the song plays: its loose clips and each placed pattern.
pub fn song_notes(p: &Project) -> Vec<FlatNote> {
    let mut out = Vec::new();
    timeline_notes(&p.song.timeline, 0, &mut out);
    for placed in &p.song.patterns {
        if let Some(pattern) = p.pattern(&placed.pattern) {
            timeline_notes(&pattern.timeline, placed.at, &mut out);
        }
    }
    sorted(out)
}

/// Where the song ends: after its last loose clip, placed pattern or audio clip (whose
/// length in ticks `audio_ticks` gives), rounded up to a bar, minimum one bar.
pub fn song_length(p: &Project, audio_ticks: &dyn Fn(&AudioLaneId) -> Tick) -> Tick {
    let bar = p.time_signature.ticks_per_bar();
    let patterns = p.song.patterns.iter().map(|pp| pp.at + p.pattern_length(&pp.pattern));
    let audio = p.song.audio.iter().map(|a| a.at + audio_ticks(&a.lane));
    let end = patterns.chain(audio).chain([p.song.timeline.end()]).max().unwrap_or(0);
    end.div_ceil(bar).max(1) * bar
}
