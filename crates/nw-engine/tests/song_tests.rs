//! Song playback: play-once, seeking, audio lanes.

use std::path::{Path, PathBuf};

use basedrop::{Collector, Owned};
use nw_core::{
    AudioLane, AudioLaneId, AudioPlacement, Clip, ClipKey, Instrument, InstrumentId, Note, Pattern, PatternId,
    PatternPlacement, Placement, PlayTarget, Project, Tick, Timeline,
};
use nw_engine::core::{Command, EngineCore};
use nw_engine::process::PlayRegion;
use nw_engine::sample::{SampleBuffer, SampleCache};
use nw_engine::snapshot::{InstrumentSnapshot, NoteEvent, Snapshot, compile_snapshot};

#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

const RATE: u32 = 48_000;
/// 48 kHz at 120 BPM and PPQ 960.
const SPT: usize = 25;
const CENTRE: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn render(core: &mut EngineCore, frames: usize) -> Vec<f32> {
    let mut buf = vec![0.0f32; frames * 2];
    for chunk in buf.chunks_mut(512) {
        core.process(chunk);
    }
    buf.iter().step_by(2).copied().collect()
}

fn dc_inst(c: &Collector) -> InstrumentSnapshot {
    let b = SampleBuffer { sample_rate: RATE, left: vec![1.0; 400_000], right: vec![1.0; 400_000] };
    let mut i = InstrumentSnapshot::new(Some(basedrop::Shared::new(&c.handle(), b)), PlayRegion::whole(400_000));
    i.volume_env.release_ms = 0.0;
    i
}

fn ev(tick: Tick, length: Tick) -> NoteEvent {
    NoteEvent { tick, instrument: 0, pitch: 60, velocity: 127, length }
}

fn core_with(c: &Collector, length: Tick, looping: bool, events: Vec<NoteEvent>) -> EngineCore {
    let s = Snapshot { tempo: 120.0, length, looping, instruments: vec![dc_inst(c)], events };
    EngineCore::new(RATE, Owned::new(&c.handle(), s))
}

fn onsets(out: &[f32]) -> Vec<usize> {
    (0..out.len()).filter(|&i| out[i] != 0.0 && (i == 0 || out[i - 1] == 0.0)).collect()
}

#[test]
fn song_plays_once_and_stops() {
    let c = Collector::new();
    let mut core = core_with(&c, 7680, false, vec![ev(3840, 960)]);
    core.handle(Command::Play);
    let out = render(&mut core, 7680 * SPT * 2);
    assert_eq!(onsets(&out), vec![3840 * SPT], "fires once, never wraps");
    assert!(!core.is_playing());
    assert_eq!(core.playhead(), 7680);
    assert!(out[7680 * SPT + 10..].iter().all(|x| *x == 0.0));
}

#[test]
fn seek_then_play_starts_there_and_loops() {
    let c = Collector::new();
    let mut core = core_with(&c, 15360, true, vec![ev(0, 960), ev(7680, 960)]);
    core.handle(Command::Seek(7680));
    assert!(!core.is_playing());
    assert_eq!(core.playhead(), 7680);
    core.handle(Command::Play);
    let out = render(&mut core, 15360 * SPT);
    assert_eq!(onsets(&out), vec![0, 7680 * SPT], "bar 3 first, then the loop start");
}

#[test]
fn seek_while_playing_moves_the_playhead() {
    let c = Collector::new();
    let mut core = core_with(&c, 15360, true, vec![ev(0, 240), ev(7680, 240)]);
    core.handle(Command::Play);
    render(&mut core, 2000);
    core.handle(Command::Seek(7000));
    let out = render(&mut core, 1000 * SPT);
    assert_eq!(onsets(&out), vec![680 * SPT]);
}

fn write_ramp(dir: &Path, frames: usize) -> PathBuf {
    let path = dir.join("tag.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..frames {
        w.write_sample(((i % 20_000) as i16) - 10_000).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn ramp_value(i: usize) -> f32 {
    (((i % 20_000) as i16) - 10_000) as f32 / 32768.0
}

fn song_with_audio(path: PathBuf, at: Tick) -> Project {
    let mut p = Project { tempo: 120.0, ..Project::default() };
    p.song.audio_lanes.push(AudioLane { id: AudioLaneId("tag".into()), path, volume: 0.5 });
    p.song.audio.push(AudioPlacement { lane: AudioLaneId("tag".into()), at });
    p
}

#[test]
fn audio_lane_plays_file_at_its_own_speed() {
    let dir = tempfile::tempdir().unwrap();
    let c = Collector::new();
    let mut cache = SampleCache::new(c.handle());
    let p = song_with_audio(write_ramp(dir.path(), 48_000), 960);
    let s = compile_snapshot(&p, Some(&PlayTarget::Song), &mut cache);
    assert!(!s.looping);
    assert_eq!(s.instruments.len(), 1, "one instrument per audio lane");
    assert_eq!(s.events, vec![NoteEvent { tick: 960, instrument: 0, pitch: 60, velocity: 127, length: 1920 }]);
    assert_eq!(s.length, 3840);
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), s));
    core.handle(Command::Play);
    let out = render(&mut core, 960 * SPT + 30_000);
    for i in [10, 1000, 25_000] {
        let want = ramp_value(i) * 0.5 * CENTRE;
        assert!((out[960 * SPT + i] - want).abs() < 1e-3, "frame {i}: {} vs {want}", out[960 * SPT + i]);
    }
}

#[test]
fn audio_chases_mid_file() {
    let dir = tempfile::tempdir().unwrap();
    let c = Collector::new();
    let mut cache = SampleCache::new(c.handle());
    let p = song_with_audio(write_ramp(dir.path(), 48_000), 0);
    let s = compile_snapshot(&p, Some(&PlayTarget::Song), &mut cache);
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), s));
    core.handle(Command::Seek(960));
    core.handle(Command::Play);
    let out = render(&mut core, 100);
    let want = ramp_value(960 * SPT + 50) * 0.5 * CENTRE;
    assert!((out[50] - want).abs() < 1e-3, "{} vs {want}", out[50]);
}

#[test]
fn missing_audio_is_silent() {
    let c = Collector::new();
    let mut cache = SampleCache::new(c.handle());
    let p = song_with_audio("/nonexistent/tag.wav".into(), 0);
    let s = compile_snapshot(&p, Some(&PlayTarget::Song), &mut cache);
    assert!(s.instruments[0].sample.is_none());
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), s));
    core.handle(Command::Play);
    assert!(render(&mut core, 5000).iter().all(|x| *x == 0.0));
}

#[test]
fn empty_song_stops_after_one_bar() {
    let c = Collector::new();
    let mut cache = SampleCache::new(c.handle());
    let s = compile_snapshot(&Project::default(), Some(&PlayTarget::Song), &mut cache);
    assert_eq!((s.length, s.looping, s.events.len()), (3840, false, 0));
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), s));
    core.handle(Command::Play);
    render(&mut core, 3840 * SPT + 100);
    assert!(!core.is_playing());
}

fn key(inst: &str, number: u32) -> ClipKey {
    ClipKey { instrument: InstrumentId(inst.into()), number }
}

#[test]
fn compile_song_offsets_placed_patterns() {
    let c = Collector::new();
    let mut cache = SampleCache::new(c.handle());
    let mut p = Project { tempo: 120.0, ..Project::default() };
    p.instruments.push(Instrument::new(InstrumentId("kick".into()), "Kick"));
    let mut t = Timeline::default();
    t.clips.push(Clip { key: key("kick", 1), start: 0, length: 3840, notes: vec![Note { pitch: 36, start: 0, length: 240, velocity: 100 }] });
    t.placements.push(Placement { clip: key("kick", 1), at: 0 });
    p.patterns.push(Pattern { id: PatternId("pattern-1".into()), name: "Pattern 1".into(), timeline: t });
    p.song.patterns.push(PatternPlacement { pattern: PatternId("pattern-1".into()), at: 3840 });
    let s = compile_snapshot(&p, Some(&PlayTarget::Song), &mut cache);
    assert_eq!((s.length, s.looping), (7680, false));
    assert_eq!(s.events, vec![NoteEvent { tick: 3840, instrument: 0, pitch: 36, velocity: 100, length: 240 }]);
    let s = compile_snapshot(&p, Some(&PlayTarget::Pattern(PatternId("pattern-1".into()))), &mut cache);
    assert_eq!((s.length, s.looping), (3840, true));
    assert_eq!(s.events[0].tick, 0);
}

#[test]
fn song_processing_does_not_allocate() {
    let dir = tempfile::tempdir().unwrap();
    let c = Collector::new();
    let mut cache = SampleCache::new(c.handle());
    let p = song_with_audio(write_ramp(dir.path(), 48_000), 0);
    let s = compile_snapshot(&p, Some(&PlayTarget::Song), &mut cache);
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), s));
    let mut buf = vec![0.0f32; 1024];
    assert_no_alloc::assert_no_alloc(|| {
        core.handle(Command::Seek(480));
        core.handle(Command::Play);
        for _ in 0..200 {
            core.process(&mut buf);
        }
        core.handle(Command::Seek(100));
        core.process(&mut buf);
    });
}
