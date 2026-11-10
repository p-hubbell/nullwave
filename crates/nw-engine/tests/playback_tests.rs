//! Voice playback of a region: loops, ping-pong, one-shot vs. held, tuning.

use basedrop::{Collector, Owned, Shared};
use nw_core::{LoopMode, Tick};
use nw_engine::core::{Command, EngineCore};
use nw_engine::process::PlayRegion;
use nw_engine::sample::SampleBuffer;
use nw_engine::snapshot::{InstrumentSnapshot, NoteEvent, Snapshot};
use nw_engine::voice::{Voice, VoiceParams, VoiceStart, tuned_rate};

#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

const RATE: u32 = 48_000;
/// 48 kHz at 120 BPM and PPQ 960.
const SPT: usize = 25;

/// Frame i holds 1 + i, so the output shows exactly which frame was read.
fn ramp(c: &Collector, frames: usize) -> Shared<SampleBuffer> {
    let left: Vec<f32> = (0..frames).map(|i| 1.0 + i as f32).collect();
    Shared::new(&c.handle(), SampleBuffer { sample_rate: RATE, right: left.clone(), left })
}

fn region(start: usize, end: usize, mode: LoopMode, loop_start: usize, loop_end: usize) -> PlayRegion {
    PlayRegion { start, end, loop_mode: mode, loop_start, loop_end }
}

fn voice_start(sample: Shared<SampleBuffer>, region: PlayRegion, duration: Option<u64>) -> VoiceStart {
    VoiceStart {
        sample,
        instrument: 0,
        pitch: 60,
        region,
        rate: 1.0,
        // Undo the centre-pan gain so left output equals the sample value.
        params: VoiceParams::simple(std::f32::consts::SQRT_2, 0.0, 0, 0),
        duration_samples: duration,
        age: 0,
    }
}

fn run(voice: &mut Voice, frames: usize) -> Vec<f32> {
    let mut l = vec![0.0; frames];
    let mut r = vec![0.0; frames];
    voice.render(&mut l, &mut r);
    l.iter().map(|v| v.round()).collect()
}

/// The source frame each output frame came from (value − 1), or None for silence.
fn frames_read(out: &[f32]) -> Vec<Option<usize>> {
    out.iter().map(|v| if *v == 0.0 { None } else { Some(*v as usize - 1) }).collect()
}

#[test]
fn region_plays_only_between_start_and_end() {
    let c = Collector::new();
    let mut v = Voice::default();
    v.start(voice_start(ramp(&c, 1000), region(100, 200, LoopMode::Off, 0, 0), None));
    let read = frames_read(&run(&mut v, 300));
    assert_eq!(read[0], Some(100));
    assert_eq!(read[98], Some(198));
    assert!(read[99..].iter().all(Option::is_none), "stops at the region end");
    assert!(!v.is_active());
}

#[test]
fn forward_loop_repeats_until_note_end() {
    let c = Collector::new();
    let mut v = Voice::default();
    v.start(voice_start(ramp(&c, 1000), region(0, 1000, LoopMode::Forward, 100, 150), Some(400)));
    let read = frames_read(&run(&mut v, 600));
    assert_eq!(read[99], Some(99));
    assert_eq!(read[149], Some(149));
    assert_eq!(read[150], Some(100), "wraps to the loop start");
    assert_eq!(read[399], Some(100 + (399 - 100) % 50));
    assert!(read[400..].iter().all(Option::is_none), "the note's end stops it");
}

#[test]
fn ping_pong_reverses_at_edges() {
    let c = Collector::new();
    let mut v = Voice::default();
    v.start(voice_start(ramp(&c, 1000), region(0, 1000, LoopMode::PingPong, 100, 110), Some(140)));
    let read: Vec<usize> = frames_read(&run(&mut v, 140)).into_iter().map(Option::unwrap).collect();
    assert_eq!(&read[95..112], &[95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 109]);
    // Turns again at the loop start and climbs.
    let low = read[100..].iter().copied().min().unwrap();
    assert_eq!(low, 100);
    let turn = 100 + read[100..].iter().position(|&f| f == 100).unwrap();
    let after = read[turn..].iter().find(|&&f| f != 100).copied();
    assert_eq!(after, Some(101));
    assert!(read.iter().all(|&f| f <= 110));
}

#[test]
fn loop_keeps_running_through_release() {
    let c = Collector::new();
    let mut v = Voice::default();
    let mut s = voice_start(ramp(&c, 1000), region(0, 1000, LoopMode::Forward, 10, 20), Some(30));
    s.params.volume_env.release = 100;
    v.start(s);
    let out = run(&mut v, 200);
    assert!(out[60] != 0.0, "still sounding during release, past the loop end");
    assert!(out[150] == 0.0);
}

#[test]
fn empty_region_is_silent_and_inactive() {
    let c = Collector::new();
    let mut v = Voice::default();
    v.start(voice_start(ramp(&c, 10), region(5, 5, LoopMode::Off, 0, 0), None));
    assert!(run(&mut v, 10).iter().all(|x| *x == 0.0));
    assert!(!v.is_active());
}

#[test]
fn coarse_and_fine_tune_change_rate() {
    let base = tuned_rate(60, 60, 0, 0.0, RATE, RATE);
    assert!((base - 1.0).abs() < 1e-12);
    assert!((tuned_rate(60, 60, 12, 0.0, RATE, RATE) - 2.0).abs() < 1e-12);
    assert!((tuned_rate(60, 60, -12, 0.0, RATE, RATE) - 0.5).abs() < 1e-12);
    let semitone = 2f64.powf(1.0 / 12.0);
    assert!((tuned_rate(60, 60, 0, 100.0, RATE, RATE) - semitone).abs() < 1e-9);
    assert!((tuned_rate(61, 60, 0, -100.0, RATE, RATE) - 1.0).abs() < 1e-9);
    // Sample-rate conversion still applies.
    assert!((tuned_rate(60, 60, 0, 0.0, 24_000, 48_000) - 0.5).abs() < 1e-12);
}

fn inst(sample: Shared<SampleBuffer>, region: PlayRegion, one_shot: bool) -> InstrumentSnapshot {
    let mut i = InstrumentSnapshot::new(Some(sample), region);
    i.one_shot = one_shot;
    i.volume_env.release_ms = 0.0;
    i
}

fn play_one_note(instrument: InstrumentSnapshot, length: Tick, frames: usize) -> Vec<f32> {
    let c = Collector::new();
    let event = NoteEvent { tick: 0, instrument: 0, pitch: 60, velocity: 127, length };
    let snapshot = Snapshot { tempo: 120.0, loop_length: 3840 * 4, instruments: vec![instrument], events: vec![event] };
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), snapshot));
    core.handle(Command::Play);
    let mut buf = vec![0.0f32; frames * 2];
    core.process(&mut buf);
    buf.iter().step_by(2).copied().collect()
}

#[test]
fn held_note_stops_at_note_end() {
    let c = Collector::new();
    let out = play_one_note(inst(ramp(&c, 2000), PlayRegion::whole(2000), false), 10, 1000);
    assert!(out[10 * SPT - 1] != 0.0);
    assert!(out[10 * SPT..].iter().all(|x| *x == 0.0));
}

#[test]
fn one_shot_ignores_note_length() {
    let c = Collector::new();
    let out = play_one_note(inst(ramp(&c, 800), PlayRegion::whole(800), true), 10, 1000);
    assert!(out[10 * SPT + 100] != 0.0, "keeps playing past the note end");
    assert!(out[798] != 0.0);
    assert!(out[800..].iter().all(|x| *x == 0.0), "until the region ends");
}

#[test]
fn one_shot_with_loop_still_ends_at_note_end() {
    let c = Collector::new();
    let r = region(0, 2000, LoopMode::Forward, 10, 60);
    let out = play_one_note(inst(ramp(&c, 2000), r, true), 10, 1000);
    assert!(out[10 * SPT - 1] != 0.0);
    assert!(out[10 * SPT..].iter().all(|x| *x == 0.0));
}

#[test]
fn snapshot_tuning_reaches_the_voice() {
    let c = Collector::new();
    let mut i = inst(ramp(&c, 4000), PlayRegion::whole(4000), false);
    i.coarse = 12;
    let out = play_one_note(i, 40, 100);
    // An octave up reads every other frame (undo the centre-pan gain to see the value).
    assert_eq!((out[10] / std::f32::consts::FRAC_1_SQRT_2).round(), 1.0 + 20.0);
}

#[test]
fn process_does_not_allocate_with_loops_and_one_shots() {
    let c = Collector::new();
    let sample = ramp(&c, 20_000);
    let instruments = vec![
        inst(sample.clone(), region(100, 19_000, LoopMode::Forward, 500, 900), false),
        inst(sample.clone(), region(0, 20_000, LoopMode::PingPong, 300, 340), true),
        inst(sample.clone(), region(5_000, 6_000, LoopMode::Off, 0, 0), true),
        inst(sample, region(10, 10, LoopMode::Off, 0, 0), false),
    ];
    let events = (0..64)
        .map(|i| NoteEvent { tick: (i * 30) as Tick, instrument: i % 4, pitch: 48 + (i % 24) as u8, velocity: 100, length: 400 })
        .collect();
    let snapshot = Snapshot { tempo: 120.0, loop_length: 3840, instruments, events };
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), snapshot));
    core.handle(Command::Play);
    core.handle(Command::PreviewNote { instrument: 1, pitch: 60, velocity: 100, length: 2000 });
    let mut buf = vec![0.0f32; 4096 * 2];
    assert_no_alloc::assert_no_alloc(|| {
        for _ in 0..40 {
            core.process(&mut buf);
        }
    });
    core.handle(Command::Stop);
    assert_no_alloc::assert_no_alloc(|| core.process(&mut buf));
}
