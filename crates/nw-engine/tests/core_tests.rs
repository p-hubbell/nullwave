use std::collections::BTreeMap;
use std::path::PathBuf;

use basedrop::{Collector, Owned, Shared};
use nw_core::{Instrument, InstrumentId, Note, Pattern, PatternId, Project, Tick};
use nw_engine::core::{Command, EngineCore, Feedback, MAX_VOICES, run_block};
use nw_engine::process::PlayRegion;
use nw_engine::sample::{SampleBuffer, SampleCache};
use nw_engine::snapshot::{InstrumentSnapshot, NoteEvent, Snapshot, compile_snapshot};
use nw_engine::voice::pan_gains;

#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

const RATE: u32 = 48_000;
/// 48 kHz at 120 BPM and PPQ 960.
const SPT: usize = 25;

fn dc(c: &Collector, frames: usize) -> Shared<SampleBuffer> {
    Shared::new(
        &c.handle(),
        SampleBuffer { sample_rate: RATE, left: vec![1.0; frames], right: vec![1.0; frames] },
    )
}

fn inst(sample: Option<Shared<SampleBuffer>>, release_ms: f32) -> InstrumentSnapshot {
    let region = PlayRegion::whole(sample.as_ref().map_or(0, |b| b.len()));
    InstrumentSnapshot {
        sample,
        region,
        one_shot: false,
        root_note: 60,
        coarse: 0,
        fine_cents: 0.0,
        attack_ms: 0.0,
        release_ms,
        volume: 1.0,
        pan: 0.0,
    }
}

fn ev(tick: Tick, length: Tick) -> NoteEvent {
    NoteEvent { tick, instrument: 0, pitch: 60, velocity: 127, length }
}

fn snap(
    c: &Collector,
    tempo: f64,
    loop_length: Tick,
    instruments: Vec<InstrumentSnapshot>,
    events: Vec<NoteEvent>,
) -> Owned<Snapshot> {
    Owned::new(&c.handle(), Snapshot { tempo, loop_length, instruments, events })
}

/// Process `frames` frames in blocks of `block` and return the left channel.
fn render(core: &mut EngineCore, frames: usize, block: usize) -> Vec<f32> {
    let mut left = Vec::with_capacity(frames);
    let mut buf = vec![0.0f32; block * 2];
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        core.process(&mut buf[..n * 2]);
        left.extend(buf[..n * 2].iter().step_by(2).copied());
        done += n;
    }
    left
}

fn first_nonzero(v: &[f32]) -> Option<usize> {
    v.iter().position(|x| *x != 0.0)
}

#[test]
fn note_starts_at_exact_sample_offset() {
    let c = Collector::new();
    let s = snap(&c, 120.0, 3840, vec![inst(Some(dc(&c, 100_000)), 0.0)], vec![ev(10, 96)]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    // 250 frames in, then a block that straddles 256: the note must start at 250.
    let out = render(&mut core, 512, 200);
    assert_eq!(first_nonzero(&out), Some(10 * SPT));

    let s = snap(&c, 120.0, 3840, vec![inst(Some(dc(&c, 100_000)), 0.0)], vec![ev(11, 96)]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    // Tick 11 = frame 275, across the first 256-frame block boundary.
    let out = render(&mut core, 512, 256);
    assert_eq!(first_nonzero(&out), Some(11 * SPT));
}

#[test]
fn loop_wraps_and_retriggers() {
    let c = Collector::new();
    let s = snap(&c, 120.0, 3840, vec![inst(Some(dc(&c, 200_000)), 0.0)], vec![ev(0, 960)]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    let out = render(&mut core, 96_000 + 1000, 256);
    assert_ne!(out[0], 0.0);
    assert_ne!(out[24_000 - 1], 0.0);
    assert!(out[24_000..96_000].iter().all(|x| *x == 0.0), "silent between notes");
    assert_ne!(out[96_000], 0.0, "retriggered at the loop point");
}

#[test]
fn playhead_wraps() {
    let c = Collector::new();
    let s = snap(&c, 120.0, 3840, vec![], vec![]);
    let mut core = EngineCore::new(RATE, s);
    assert!(!core.is_playing());
    core.handle(Command::Play);
    assert!(core.is_playing());
    render(&mut core, 250, 250);
    assert_eq!(core.playhead(), 10);
    render(&mut core, 96_000 - 250, 1000);
    assert_eq!(core.playhead(), 0);
    render(&mut core, 100, 100);
    assert_eq!(core.playhead(), 4);
}

#[test]
fn instrument_without_sample_is_silent_not_panic() {
    let c = Collector::new();
    let s = snap(&c, 120.0, 3840, vec![inst(None, 20.0)], vec![ev(0, 960)]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    let out = render(&mut core, 4096, 512);
    assert!(out.iter().all(|x| *x == 0.0));
    assert_eq!(core.active_voices(), 0);
    // An event referring to a missing instrument index is skipped too.
    let bad = NoteEvent { instrument: 5, ..ev(0, 960) };
    let s = snap(&c, 120.0, 3840, vec![], vec![bad]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    assert!(render(&mut core, 512, 512).iter().all(|x| *x == 0.0));
}

fn project_with(path: Option<PathBuf>, notes: Vec<Note>) -> (Project, PatternId) {
    let id = InstrumentId("kick".into());
    let mut instrument = Instrument::new(id.clone(), "Kick");
    instrument.sampler.sample_path = path;
    instrument.pan = 3.0;
    instrument.volume = 2.0;
    instrument.sampler.volume_env.attack_ms = -5.0;
    instrument.sampler.volume_env.release_ms = -1.0;
    let pid = PatternId("p1".into());
    let mut map = BTreeMap::new();
    map.insert(id, notes);
    let project = Project {
        tempo: 128.0,
        instruments: vec![instrument],
        patterns: vec![Pattern { id: pid.clone(), name: "P1".into(), notes: map }],
        ..Project::default()
    };
    (project, pid)
}

#[test]
fn compile_marks_missing_sample_as_none() {
    let c = Collector::new();
    let mut cache = SampleCache::new(c.handle());
    let notes = vec![
        Note { pitch: 64, start: 960, length: 240, velocity: 90 },
        Note { pitch: 60, start: 0, length: 480, velocity: 100 },
    ];
    let (project, pid) = project_with(Some("/nonexistent.wav".into()), notes);
    let s = compile_snapshot(&project, Some(&pid), &mut cache);
    assert_eq!(s.tempo, 128.0);
    assert_eq!(s.loop_length, project.pattern_length(&pid));
    assert_eq!(s.instruments.len(), 1);
    assert!(s.instruments[0].sample.is_none());
    // Clamped inputs.
    assert_eq!(s.instruments[0].pan, 1.0);
    assert_eq!(s.instruments[0].volume, 1.0);
    assert_eq!(s.instruments[0].attack_ms, 0.0);
    assert_eq!(s.instruments[0].release_ms, 0.0);
    // Notes are still emitted, sorted by tick.
    assert_eq!(s.events.len(), 2);
    assert_eq!(s.events[0], NoteEvent { tick: 0, instrument: 0, pitch: 60, velocity: 100, length: 480 });
    assert_eq!(s.events[1].tick, 960);

    let (project, _) = project_with(None, vec![]);
    let s = compile_snapshot(&project, None, &mut cache);
    assert!(s.events.is_empty());
    assert!(s.instruments[0].sample.is_none());
    assert_eq!(s.loop_length, project.time_signature.ticks_per_bar());
}

#[test]
fn removing_note_mid_play_does_not_hang() {
    let mut c = Collector::new();
    let sample = dc(&c, 200_000);
    let s = snap(&c, 120.0, 7680, vec![inst(Some(sample.clone()), 20.0)], vec![ev(0, 960)]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    render(&mut core, 1000, 256);
    assert_eq!(core.active_voices(), 1);
    core.handle(Command::SetSnapshot(snap(&c, 120.0, 7680, vec![inst(Some(sample), 20.0)], vec![])));
    let out = render(&mut core, 960 * SPT - 1000 + 960, 256);
    assert_ne!(out[0], 0.0, "voice keeps sounding after swap");
    let tail = render(&mut core, 2000, 256);
    assert!(tail.iter().all(|x| *x == 0.0));
    assert_eq!(core.active_voices(), 0);
    c.collect();
}

#[test]
fn stop_releases_everything() {
    let c = Collector::new();
    let sample = dc(&c, 200_000);
    let s = snap(&c, 120.0, 3840, vec![inst(Some(sample.clone()), 20.0)], vec![ev(0, 3000)]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    core.handle(Command::Preview(sample));
    render(&mut core, 1000, 256);
    assert_eq!(core.active_voices(), 2);
    core.handle(Command::Stop);
    assert!(!core.is_playing());
    assert_eq!(core.playhead(), 0);
    render(&mut core, 960, 256);
    let out = render(&mut core, 1000, 256);
    assert!(out.iter().all(|x| *x == 0.0));
    assert_eq!(core.active_voices(), 0);
    assert_eq!(core.playhead(), 0);
}

#[test]
fn tempo_change_keeps_position() {
    let mut c = Collector::new();
    let mut core = EngineCore::new(RATE, snap(&c, 120.0, 3840, vec![], vec![]));
    core.handle(Command::Play);
    render(&mut core, 100 * SPT, 256);
    assert_eq!(core.playhead(), 100);
    // 60 BPM: 50 samples per tick.
    core.handle(Command::SetSnapshot(snap(&c, 60.0, 3840, vec![], vec![])));
    assert_eq!(core.playhead(), 100);
    render(&mut core, 50 * 10, 256);
    assert_eq!(core.playhead(), 110);
    // Shorter loop: position wraps modulo the new length.
    core.handle(Command::SetSnapshot(snap(&c, 60.0, 100, vec![], vec![])));
    assert_eq!(core.playhead(), 10);
    c.collect();
}

#[test]
fn steals_oldest_when_pool_full() {
    let c = Collector::new();
    let events = (0..80).map(|_| ev(0, 960)).collect();
    let s = snap(&c, 120.0, 3840, vec![inst(Some(dc(&c, 100_000)), 0.0)], events);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    let out = render(&mut core, 256, 256);
    assert_eq!(core.active_voices(), MAX_VOICES);
    let (gl, _) = pan_gains(0.0);
    assert!((out[10] - MAX_VOICES as f32 * gl).abs() < 1e-3);
}

#[test]
fn process_does_not_allocate() {
    let mut c = Collector::new();
    let sample = dc(&c, 200_000);
    let events: Vec<NoteEvent> = (0..80).map(|i| ev(i % 4, 960)).collect();
    let s = snap(&c, 120.0, 3840, vec![inst(Some(sample.clone()), 20.0)], events.clone());
    let mut core = EngineCore::new(RATE, s);
    let (mut cmd_tx, mut cmd_rx) = rtrb::RingBuffer::<Command>::new(16);
    let (mut fb_tx, mut fb_rx) = rtrb::RingBuffer::<Feedback>::new(2);
    let mut buf = vec![0.0f32; 8192 * 2];

    cmd_tx.push(Command::Play).ok().unwrap();
    cmd_tx.push(Command::Preview(sample.clone())).ok().unwrap();
    assert_no_alloc::assert_no_alloc(|| {
        run_block(&mut core, &mut cmd_rx, &mut fb_tx, &mut buf[..512]);
    });
    assert_eq!(core.active_voices(), MAX_VOICES);

    let new = snap(&c, 100.0, 1920, vec![inst(Some(sample.clone()), 20.0)], events);
    cmd_tx.push(Command::SetSnapshot(new)).ok().unwrap();
    cmd_tx.push(Command::Preview(sample.clone())).ok().unwrap();
    assert_no_alloc::assert_no_alloc(|| {
        // Larger than MAX_BLOCK_FRAMES, wraps the loop, and overflows the feedback queue.
        run_block(&mut core, &mut cmd_rx, &mut fb_tx, &mut buf);
        run_block(&mut core, &mut cmd_rx, &mut fb_tx, &mut buf);
        run_block(&mut core, &mut cmd_rx, &mut fb_tx, &mut buf);
    });
    assert!(core.active_voices() > 0);
    cmd_tx.push(Command::StopPreview).ok().unwrap();
    cmd_tx.push(Command::Stop).ok().unwrap();
    assert_no_alloc::assert_no_alloc(|| {
        run_block(&mut core, &mut cmd_rx, &mut fb_tx, &mut buf[..512]);
    });
    assert!(!core.is_playing());
    match fb_rx.pop().unwrap() {
        Feedback::Playhead { playing, .. } => assert!(playing),
        Feedback::Load(_) => panic!("unexpected load"),
    }
    c.collect();
}

#[test]
fn run_block_reports_playhead() {
    let c = Collector::new();
    let mut core = EngineCore::new(RATE, snap(&c, 120.0, 3840, vec![], vec![]));
    let (mut cmd_tx, mut cmd_rx) = rtrb::RingBuffer::<Command>::new(4);
    let (mut fb_tx, mut fb_rx) = rtrb::RingBuffer::<Feedback>::new(4);
    cmd_tx.push(Command::Play).ok().unwrap();
    let mut buf = vec![0.0f32; 260 * 2];
    run_block(&mut core, &mut cmd_rx, &mut fb_tx, &mut buf);
    match fb_rx.pop().unwrap() {
        Feedback::Playhead { tick, playing } => {
            assert_eq!(tick, 10);
            assert!(playing);
        }
        Feedback::Load(_) => panic!("unexpected load"),
    }
    assert!(fb_rx.pop().is_err());
}

#[test]
fn velocity_and_volume_scale_gain() {
    let c = Collector::new();
    let mut i = inst(Some(dc(&c, 100_000)), 0.0);
    i.volume = 0.5;
    let s = snap(&c, 120.0, 3840, vec![i], vec![ev(0, 960)]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    let out = render(&mut core, 64, 64);
    let expected = 0.5 * std::f32::consts::FRAC_PI_4.cos();
    assert!((out[10] - expected).abs() < 1e-5, "{} vs {expected}", out[10]);

    let mut i = inst(Some(dc(&c, 100_000)), 0.0);
    i.volume = 0.5;
    let s = snap(&c, 120.0, 3840, vec![i], vec![NoteEvent { velocity: 64, ..ev(0, 960) }]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::Play);
    let out = render(&mut core, 64, 64);
    assert!((out[10] - 64.0 / 127.0 * expected).abs() < 1e-5);
}

#[test]
fn preview_plays_and_stops() {
    let c = Collector::new();
    let mut core = EngineCore::new(RATE, snap(&c, 120.0, 3840, vec![], vec![]));
    let sample = dc(&c, 100_000);
    core.handle(Command::Preview(sample.clone()));
    let out = render(&mut core, 1000, 256);
    let expected = 0.78 * std::f32::consts::FRAC_PI_4.cos();
    assert!((out[0] - expected).abs() < 1e-5);
    assert!((out[999] - expected).abs() < 1e-5);
    assert_eq!(core.active_voices(), 1);
    assert!(!core.is_playing());
    assert_eq!(core.playhead(), 0);

    // A new preview releases the old one (two voices briefly), then only one remains.
    core.handle(Command::Preview(sample));
    assert_eq!(core.active_voices(), 2);
    render(&mut core, 960 + 10, 256);
    assert_eq!(core.active_voices(), 1);

    core.handle(Command::StopPreview);
    let out = render(&mut core, 960, 256);
    assert!(out[0] > 0.0 && out[0] <= expected);
    let out = render(&mut core, 500, 256);
    assert!(out.iter().all(|x| *x == 0.0));
    assert_eq!(core.active_voices(), 0);
}

#[test]
fn snapshot_swap_keeps_pending_event_with_fractional_spt() {
    // 128 BPM at 48 kHz: 23.4375 samples per tick. Tick 100 fires at frame
    // ceil(2343.75) = 2344, so a 2344-frame block ends with the position just
    // past tick 100 but the event not yet fired.
    let c = Collector::new();
    let sample = dc(&c, 100_000);
    let make = || snap(&c, 128.0, 3840, vec![inst(Some(sample.clone()), 0.0)], vec![ev(100, 96)]);
    let mut core = EngineCore::new(RATE, make());
    core.handle(Command::Play);
    let out = render(&mut core, 2344, 2344);
    assert!(out.iter().all(|x| *x == 0.0));
    core.handle(Command::SetSnapshot(make()));
    let out = render(&mut core, 64, 64);
    assert_ne!(out[0], 0.0, "pending note must fire at frame 0 of the next block");

    // Without a swap, the same note fires at the same frame.
    let mut core = EngineCore::new(RATE, make());
    core.handle(Command::Play);
    let out = render(&mut core, 2400, 2400);
    assert_eq!(first_nonzero(&out), Some(2344));
}

#[test]
fn preview_note_plays_through_instrument_for_its_length() {
    let c = Collector::new();
    let s = snap(&c, 120.0, 3840, vec![inst(Some(dc(&c, 100_000)), 0.0)], vec![]);
    let mut core = EngineCore::new(RATE, s);
    core.handle(Command::PreviewNote { instrument: 0, pitch: 60, velocity: 127, length: 10 });
    let out = render(&mut core, 1000, 256);
    assert_eq!(first_nonzero(&out), Some(0));
    // Sounds for 10 ticks (250 frames) with no release, then is silent.
    assert!(out[249] != 0.0);
    assert!(out[300..].iter().all(|x| *x == 0.0));
}

#[test]
fn preview_note_replaces_previous_and_ignores_bad_instrument() {
    let c = Collector::new();
    let s = snap(&c, 120.0, 3840, vec![inst(Some(dc(&c, 100_000)), 0.0)], vec![]);
    let mut core = EngineCore::new(RATE, s);
    // Unknown instrument: nothing sounds, nothing panics.
    core.handle(Command::PreviewNote { instrument: 7, pitch: 60, velocity: 127, length: 1000 });
    assert!(render(&mut core, 100, 100).iter().all(|x| *x == 0.0));
    core.handle(Command::PreviewNote { instrument: 0, pitch: 60, velocity: 127, length: 1000 });
    core.handle(Command::PreviewNote { instrument: 0, pitch: 60, velocity: 127, length: 1000 });
    // The second preview releases the first (release 0 ms), leaving one voice.
    let out = render(&mut core, 100, 100);
    assert!(out[50] > 0.0);
    assert_eq!(core.active_voices(), 1);
    core.handle(Command::StopPreview);
    let out = render(&mut core, 100, 100);
    assert_eq!(out[99], 0.0);
}
