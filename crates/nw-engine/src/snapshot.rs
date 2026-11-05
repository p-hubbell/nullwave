//! Immutable, audio-thread-ready view of a project: per-instrument playback
//! parameters plus the note events of one pattern, sorted by tick.

use basedrop::Shared;
use nw_core::{PatternId, Project, Tick};

use crate::process::{PlayRegion, ProcessSettings};
use crate::sample::{SampleBuffer, SampleCache};

pub struct InstrumentSnapshot {
    /// The processed sample; `None` when there is no sample or it failed to load.
    pub sample: Option<Shared<SampleBuffer>>,
    /// The frames of `sample` to play and loop.
    pub region: PlayRegion,
    /// Play the whole region regardless of note length (unless looping).
    pub one_shot: bool,
    pub root_note: u8,
    pub coarse: i8,
    pub fine_cents: f32,
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
    samples.begin_pass();
    let instruments = project
        .instruments
        .iter()
        .map(|inst| {
            let s = &inst.sampler;
            let settings = ProcessSettings { processing: s.processing, region: s.region, looping: s.looping };
            let processed = s.sample_path.as_deref().and_then(|p| samples.processed(p, &settings).ok());
            let (sample, region) = match processed {
                Some((buffer, region)) => (Some(buffer), region),
                None => (None, PlayRegion::whole(0)),
            };
            (inst, sample, region)
        })
        .map(|(inst, sample, region)| InstrumentSnapshot {
            sample,
            region,
            one_shot: inst.sampler.one_shot,
            root_note: inst.sampler.root_note,
            coarse: inst.sampler.tune.coarse,
            fine_cents: inst.sampler.tune.fine_cents,
            attack_ms: inst.sampler.volume_env.attack_ms.max(0.0),
            release_ms: inst.sampler.volume_env.release_ms.max(0.0),
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
    samples.end_pass();

    Snapshot { tempo: project.tempo, loop_length, instruments, events }
}
