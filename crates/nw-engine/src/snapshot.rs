//! Immutable, audio-thread-ready view of a project: per-instrument playback
//! parameters plus the note events of one pattern, sorted by tick.

use basedrop::Shared;
use nw_core::{
    Envelope, Filter, FlatNote, Lfo, ModEnvelope, PPQ, PlayTarget, Project, Sampler, Tick, VelocityMap, VoiceSettings,
    pattern_notes, song_length, song_notes,
};

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
    /// Audio clips: when playback starts inside one of its notes, it is heard from there.
    pub chase: bool,
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
            chase: false,
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
    /// Where playback loops back to the start (`looping`) or stops.
    pub length: Tick,
    pub looping: bool,
    pub instruments: Vec<InstrumentSnapshot>,
    /// Sorted by `tick`.
    pub events: Vec<NoteEvent>,
}

fn instrument_snapshot(s: &Sampler, volume: f32, pan: f32, samples: &mut SampleCache) -> InstrumentSnapshot {
    let settings = ProcessSettings { processing: s.processing, region: s.region, looping: s.looping };
    let processed = s.sample_path.as_deref().and_then(|p| samples.processed(p, &settings).ok());
    let (sample, region) = match processed {
        Some((buffer, region)) => (Some(buffer), region),
        None => (None, PlayRegion::whole(0)),
    };
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
        volume: volume.clamp(0.0, 1.0),
        pan: pan.clamp(-1.0, 1.0),
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
        chase: false,
    }
}

/// The audio lane's file, played once at its own speed.
fn audio_sampler(path: &std::path::Path) -> Sampler {
    Sampler { sample_path: Some(path.to_path_buf()), one_shot: true, root_note: AUDIO_PITCH, ..Sampler::default() }
}

const AUDIO_PITCH: u8 = 60;

/// Length of `inst`'s region in ticks at `tempo` (at least 1).
fn region_ticks(inst: &InstrumentSnapshot, tempo: f64) -> Tick {
    let Some(sample) = &inst.sample else { return 1 };
    let frames = inst.region.end.saturating_sub(inst.region.start) as f64;
    let seconds = frames / f64::from(sample.sample_rate.max(1));
    ((seconds * tempo / 60.0 * f64::from(PPQ)).ceil() as Tick).max(1)
}

/// Build a snapshot of `project` that plays `target` (nothing if `None`). Instruments are
/// the project's, then one per song audio lane. Loads samples through `samples`; not for
/// use on the audio thread.
pub fn compile_snapshot(project: &Project, target: Option<&PlayTarget>, samples: &mut SampleCache) -> Snapshot {
    samples.begin_pass();
    let mut instruments: Vec<InstrumentSnapshot> = project
        .instruments
        .iter()
        .map(|inst| instrument_snapshot(&inst.sampler, inst.volume, inst.pan, samples))
        .collect();
    let first_audio = instruments.len();
    for lane in &project.song.audio_lanes {
        let mut inst = instrument_snapshot(&audio_sampler(&lane.path), lane.volume, 0.0, samples);
        inst.chase = true;
        instruments.push(inst);
    }
    let tempo = if project.tempo > 0.0 { project.tempo } else { 120.0 };

    let to_event = |n: FlatNote| {
        project.instrument_index(&n.instrument).map(|instrument| NoteEvent {
            tick: n.tick,
            instrument,
            pitch: n.pitch,
            velocity: n.velocity,
            length: n.length,
        })
    };
    let mut events: Vec<NoteEvent> = Vec::new();
    let (length, looping) = match target {
        Some(PlayTarget::Pattern(id)) => {
            events.extend(pattern_notes(project, id).into_iter().filter_map(to_event));
            (project.pattern_length(id), true)
        }
        Some(PlayTarget::Song) => {
            events.extend(song_notes(project).into_iter().filter_map(to_event));
            let lane_index = |id: &nw_core::AudioLaneId| project.song.audio_lanes.iter().position(|l| &l.id == id);
            for a in &project.song.audio {
                let Some(i) = lane_index(&a.lane) else { continue };
                let instrument = first_audio + i;
                let length = region_ticks(&instruments[instrument], tempo);
                events.push(NoteEvent { tick: a.at, instrument, pitch: AUDIO_PITCH, velocity: 127, length });
            }
            let audio_ticks =
                |id: &nw_core::AudioLaneId| lane_index(id).map_or(0, |i| region_ticks(&instruments[first_audio + i], tempo));
            (song_length(project, &audio_ticks), false)
        }
        None => (project.time_signature.ticks_per_bar().max(1), true),
    };
    events.sort_by_key(|e| (e.tick, e.instrument, e.pitch));
    samples.end_pass();

    Snapshot { tempo: project.tempo, length, looping, instruments, events }
}
