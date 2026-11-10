//! Immutable, audio-thread-ready view of a project: per-instrument playback
//! parameters plus the note events of one pattern, sorted by tick.

use basedrop::Shared;
use nw_core::{Envelope, Filter, Lfo, ModEnvelope, PatternId, Project, Tick, VelocityMap, VoiceSettings};

use crate::process::{PlayRegion, ProcessSettings};
use crate::sample::{SampleBuffer, SampleCache};

#[derive(Clone)]
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
    pub volume: f32,
    pub pan: f32,
    pub volume_env: Envelope,
    pub filter: Filter,
    pub cutoff_env: ModEnvelope,
    pub pitch_env: ModEnvelope,
    pub pan_env: ModEnvelope,
    pub volume_lfo: Lfo,
    pub pan_lfo: Lfo,
    pub cutoff_lfo: Lfo,
    pub pitch_lfo: Lfo,
    pub velocity: VelocityMap,
    pub voice: VoiceSettings,
}

impl InstrumentSnapshot {
    /// Full volume, centred, root C4, and the model's default sampler settings.
    pub fn new(sample: Option<Shared<SampleBuffer>>, region: PlayRegion) -> InstrumentSnapshot {
        InstrumentSnapshot {
            sample,
            region,
            one_shot: false,
            root_note: 60,
            coarse: 0,
            fine_cents: 0.0,
            volume: 1.0,
            pan: 0.0,
            volume_env: Envelope::default(),
            filter: Filter::default(),
            cutoff_env: ModEnvelope::default(),
            pitch_env: ModEnvelope::default(),
            pan_env: ModEnvelope::default(),
            volume_lfo: Lfo::default(),
            pan_lfo: Lfo::default(),
            cutoff_lfo: Lfo::default(),
            pitch_lfo: Lfo::default(),
            velocity: VelocityMap::default(),
            voice: VoiceSettings::default(),
        }
    }
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
        .map(|(inst, sample, region)| {
            let s = &inst.sampler;
            let mut volume_env = s.volume_env;
            // Hand-edited files are validated on load, but keep the audio thread safe anyway.
            volume_env.attack_ms = volume_env.attack_ms.max(0.0);
            volume_env.release_ms = volume_env.release_ms.max(0.0);
            InstrumentSnapshot {
                sample,
                region,
                one_shot: s.one_shot,
                root_note: s.root_note,
                coarse: s.tune.coarse,
                fine_cents: s.tune.fine_cents,
                volume: inst.volume.clamp(0.0, 1.0),
                pan: inst.pan.clamp(-1.0, 1.0),
                volume_env,
                filter: s.filter,
                cutoff_env: s.cutoff_env,
                pitch_env: s.pitch_env,
                pan_env: s.pan_env,
                volume_lfo: s.volume_lfo,
                pan_lfo: s.pan_lfo,
                cutoff_lfo: s.cutoff_lfo,
                pitch_lfo: s.pitch_lfo,
                velocity: s.velocity,
                voice: s.voice,
            }
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
