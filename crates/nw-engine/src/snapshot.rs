//! Immutable, audio-thread-ready view of a project: per-instrument playback
//! parameters plus the note events of one pattern, sorted by tick.

use basedrop::Shared;
use nw_core::{PatternId, Project, Tick};

use crate::sample::{SampleBuffer, SampleCache};

pub struct InstrumentSnapshot {
    /// `None` when the instrument has no sample or it failed to load.
    pub sample: Option<Shared<SampleBuffer>>,
    pub root_note: u8,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub volume: f32,
    pub pan: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteEvent {
    pub tick: Tick,
    /// Index into `Snapshot::instruments`.
    pub instrument: usize,
    pub pitch: u8,
    pub velocity: u8,
    pub length: Tick,
}

pub struct Snapshot {
    pub tempo: f64,
    pub loop_length: Tick,
    pub instruments: Vec<InstrumentSnapshot>,
    /// Sorted by `tick`.
    pub events: Vec<NoteEvent>,
}

/// Build a snapshot of `project`, with the events of `pattern` (none if `None`).
/// Loads samples through `samples`; not for use on the audio thread.
pub fn compile_snapshot(
    project: &Project,
    pattern: Option<&PatternId>,
    samples: &mut SampleCache,
) -> Snapshot {
    let instruments = project
        .instruments
        .iter()
        .map(|inst| InstrumentSnapshot {
            sample: inst.sampler.sample_path.as_deref().and_then(|p| samples.get(p).ok()),
            root_note: inst.sampler.root_note,
            attack_ms: inst.sampler.attack_ms.max(0.0),
            release_ms: inst.sampler.release_ms.max(0.0),
            volume: inst.volume.clamp(0.0, 1.0),
            pan: inst.pan.clamp(-1.0, 1.0),
        })
        .collect();

    let mut events = Vec::new();
    let loop_length = match pattern {
        Some(id) => {
            if let Some(p) = project.pattern(id) {
                for (inst_id, notes) in &p.notes {
                    let Some(instrument) = project.instrument_index(inst_id) else { continue };
                    events.extend(notes.iter().map(|n| NoteEvent {
                        tick: n.start,
                        instrument,
                        pitch: n.pitch,
                        velocity: n.velocity,
                        length: n.length,
                    }));
                }
            }
            project.pattern_length(id)
        }
        None => project.time_signature.ticks_per_bar().max(1),
    };
    events.sort_by_key(|e| (e.tick, e.instrument, e.pitch));

    Snapshot { tempo: project.tempo, loop_length, instruments, events }
}
