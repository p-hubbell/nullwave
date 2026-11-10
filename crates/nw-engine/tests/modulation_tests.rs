//! Envelopes, filter, LFOs and velocity mapping, heard through the engine core.

use basedrop::{Collector, Owned, Shared};
use nw_core::{Envelope, Filter, FilterKind, Lfo, LfoRate, LfoShape, ModEnvelope, Tick, VelocityMap};
use nw_engine::core::{Command, EngineCore};
use nw_engine::process::PlayRegion;
use nw_engine::sample::SampleBuffer;
use nw_engine::snapshot::{InstrumentSnapshot, NoteEvent, Snapshot};

#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

const RATE: u32 = 48_000;

fn shared(c: &Collector, left: Vec<f32>, right: Vec<f32>) -> Shared<SampleBuffer> {
    Shared::new(&c.handle(), SampleBuffer { sample_rate: RATE, left, right })
}

fn dc(c: &Collector, frames: usize) -> Shared<SampleBuffer> {
    shared(c, vec![1.0; frames], vec![1.0; frames])
}

/// Frame i holds 1 + i.
fn ramp(c: &Collector, frames: usize) -> Shared<SampleBuffer> {
    let left: Vec<f32> = (0..frames).map(|i| 1.0 + i as f32).collect();
    shared(c, left.clone(), left)
}

fn sine(c: &Collector, freq: f32, frames: usize) -> Shared<SampleBuffer> {
    let left: Vec<f32> = (0..frames)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin())
        .collect();
    shared(c, left.clone(), left)
}

fn inst(sample: Shared<SampleBuffer>) -> InstrumentSnapshot {
    let region = PlayRegion::whole(sample.len());
    InstrumentSnapshot::new(Some(sample), region)
}

/// Plays one note per entry (tick 0, pitch 60) and returns (left, right).
fn play(instrument: InstrumentSnapshot, velocity: u8, length: Tick, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let c = Collector::new();
    let event = NoteEvent { tick: 0, instrument: 0, pitch: 60, velocity, length };
    let snapshot = Snapshot { tempo: 120.0, length: 3840 * 16, looping: true, instruments: vec![instrument], events: vec![event] };
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), snapshot));
    core.handle(Command::Play);
    let mut buf = vec![0.0f32; frames * 2];
    core.process(&mut buf);
    (buf.iter().step_by(2).copied().collect(), buf.iter().skip(1).step_by(2).copied().collect())
}

fn rms(v: &[f32]) -> f32 {
    (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
}

#[test]
fn snapshot_defaults_match_the_model_defaults() {
    let c = Collector::new();
    let i = inst(dc(&c, 10));
    assert_eq!(i.volume_env, Envelope::default());
    assert_eq!(i.filter, Filter::default());
    assert_eq!(i.velocity, VelocityMap::default());
    assert_eq!((i.volume, i.pan, i.root_note, i.one_shot), (1.0, 0.0, 60, false));
}

#[test]
fn volume_envelope_decays_to_sustain() {
    let c = Collector::new();
    let mut i = inst(dc(&c, 100_000));
    i.volume_env = Envelope { decay_ms: 10.0, sustain: 0.25, ..Envelope::default() };
    let (l, _) = play(i, 127, 3840, 2000);
    let centre = std::f32::consts::FRAC_1_SQRT_2;
    assert!((l[0] - centre).abs() < 1e-4, "starts at full level");
    assert!((l[1000] - 0.25 * centre).abs() < 1e-4, "settles at sustain after 480 frames");
}

#[test]
fn pitch_env_bends_rate() {
    let c = Collector::new();
    let mut i = inst(ramp(&c, 100_000));
    i.pitch_env = ModEnvelope { env: Envelope::default(), amount: 12.0 };
    let (l, _) = play(i, 127, 3840, 400);
    // A full-level envelope at +12 semitones reads twice as fast.
    let frame = (l[320] / std::f32::consts::FRAC_1_SQRT_2).round() - 1.0;
    assert!((frame - 640.0).abs() <= 2.0, "{frame}");
}

#[test]
fn pitch_lfo_wobbles_around_the_note() {
    let c = Collector::new();
    let mut plain = inst(ramp(&c, 100_000));
    plain.velocity.to_volume = 0.0;
    let mut wobbly = inst(ramp(&c, 100_000));
    wobbly.velocity.to_volume = 0.0;
    wobbly.pitch_lfo = Lfo { shape: LfoShape::Square, rate: LfoRate::Hz(20.0), amount: 12.0, delay_ms: 0.0 };
    let (a, _) = play(plain, 127, 3840, 1200);
    let (b, _) = play(wobbly, 127, 3840, 1200);
    // Square at +1 for the first half cycle (1200 frames): octave up, twice as far along.
    assert!(b[1000] > 1.8 * a[1000], "{} vs {}", b[1000], a[1000]);
}

#[test]
fn pan_lfo_moves_balance() {
    let c = Collector::new();
    let mut i = inst(dc(&c, 100_000));
    i.pan_lfo = Lfo { shape: LfoShape::Square, rate: LfoRate::Hz(10.0), amount: 1.0, delay_ms: 0.0 };
    let (l, r) = play(i, 127, 3840 * 4, 4800);
    // First half cycle (2400 frames) the square is +1: hard right; then hard left.
    assert!(l[1000].abs() < 1e-3 && r[1000] > 0.9, "{} {}", l[1000], r[1000]);
    assert!(r[3600].abs() < 1e-3 && l[3600] > 0.9, "{} {}", l[3600], r[3600]);
}

#[test]
fn pan_env_offsets_pan() {
    let c = Collector::new();
    let mut i = inst(dc(&c, 100_000));
    i.pan_env = ModEnvelope { env: Envelope::default(), amount: -1.0 };
    let (l, r) = play(i, 127, 3840, 400);
    assert!(r[200].abs() < 1e-3 && l[200] > 0.99);
}

#[test]
fn volume_lfo_dips_by_its_depth() {
    let c = Collector::new();
    let mut i = inst(dc(&c, 100_000));
    i.volume_lfo = Lfo { shape: LfoShape::Square, rate: LfoRate::Hz(10.0), amount: 1.0, delay_ms: 0.0 };
    let (l, _) = play(i, 127, 3840 * 4, 4800);
    let centre = std::f32::consts::FRAC_1_SQRT_2;
    assert!((l[1000] - centre).abs() < 1e-4, "LFO at +1: full volume");
    assert!(l[3600].abs() < 1e-4, "LFO at −1 with depth 1: silent");
}

#[test]
fn lowpass_filter_darkens() {
    let c = Collector::new();
    let mut open = inst(sine(&c, 8000.0, 48_000));
    open.filter = Filter { kind: FilterKind::LowPass, cutoff_hz: 20_000.0, resonance: 0.0 };
    let mut closed = inst(sine(&c, 8000.0, 48_000));
    closed.filter = Filter { kind: FilterKind::LowPass, cutoff_hz: 300.0, resonance: 0.0 };
    let (a, _) = play(open, 127, 3840 * 4, 9600);
    let (b, _) = play(closed, 127, 3840 * 4, 9600);
    assert!(rms(&b[4800..]) < rms(&a[4800..]) / 20.0);
}

#[test]
fn cutoff_env_opens_the_filter() {
    let c = Collector::new();
    let mut i = inst(sine(&c, 8000.0, 48_000));
    i.filter = Filter { kind: FilterKind::LowPass, cutoff_hz: 300.0, resonance: 0.0 };
    i.cutoff_env = ModEnvelope { env: Envelope { decay_ms: 50.0, sustain: 0.0, ..Envelope::default() }, amount: 6.0 };
    let (l, _) = play(i, 127, 3840 * 4, 9600);
    // A pluck: bright at the start, dark once the envelope has decayed.
    assert!(rms(&l[100..600]) > 5.0 * rms(&l[6000..9000]));
}

#[test]
fn velocity_to_cutoff_darkens_soft_notes() {
    let c = Collector::new();
    let mut i = inst(sine(&c, 5000.0, 48_000));
    i.filter = Filter { kind: FilterKind::LowPass, cutoff_hz: 200.0, resonance: 0.0 };
    i.velocity = VelocityMap { to_volume: 0.0, to_cutoff_octaves: 6.0 };
    let soft_inst = i.clone();
    let (loud, _) = play(i, 127, 3840 * 4, 9600);
    let (soft, _) = play(soft_inst, 10, 3840 * 4, 9600);
    assert!(rms(&loud[4800..]) > 3.0 * rms(&soft[4800..]));
}

#[test]
fn velocity_to_volume_zero_ignores_velocity() {
    let c = Collector::new();
    let mut i = inst(dc(&c, 10_000));
    i.velocity.to_volume = 0.0;
    let soft_inst = i.clone();
    let (loud, _) = play(i, 127, 3840, 100);
    let (soft, _) = play(soft_inst, 1, 3840, 100);
    assert_eq!(loud[50], soft[50]);
    // Default mapping scales by velocity, as before.
    let j = inst(dc(&c, 10_000));
    let half = j.clone();
    let (full, _) = play(j, 127, 3840, 100);
    let (part, _) = play(half, 64, 3840, 100);
    assert!((part[50] / full[50] - 64.0 / 127.0).abs() < 1e-5);
}

#[test]
fn synced_lfo_uses_snapshot_tempo() {
    let c = Collector::new();
    let mut i = inst(dc(&c, 100_000));
    // 1/8 note at 120 BPM = 0.25 s = 12000 frames per cycle; square +1 for the first half.
    i.pan_lfo = Lfo { shape: LfoShape::Square, rate: LfoRate::Sync { numerator: 1, denominator: 8 }, amount: 1.0, delay_ms: 0.0 };
    let (l, r) = play(i, 127, 3840 * 4, 13_000);
    assert!(l[5000].abs() < 1e-3 && r[5000] > 0.9);
    assert!(r[7000].abs() < 1e-3 && l[7000] > 0.9);
}

#[test]
fn process_does_not_allocate_with_all_modulation_on() {
    let c = Collector::new();
    let lfo = |shape| Lfo { shape, rate: LfoRate::Hz(3.0), amount: 0.5, delay_ms: 5.0 };
    let env = ModEnvelope { env: Envelope { attack_ms: 3.0, decay_ms: 40.0, sustain: 0.3, ..Envelope::default() }, amount: 2.0 };
    let mut instruments = Vec::new();
    for kind in [FilterKind::LowPass, FilterKind::HighPass, FilterKind::BandPass, FilterKind::Notch] {
        let mut i = inst(sine(&c, 440.0, 40_000));
        i.volume_env = Envelope { delay_ms: 1.0, attack_ms: 5.0, hold_ms: 2.0, decay_ms: 30.0, sustain: 0.6, release_ms: 50.0, attack_curve: -0.5, decay_curve: 0.5 };
        i.filter = Filter { kind, cutoff_hz: 2000.0, resonance: 0.7 };
        i.cutoff_env = env;
        i.pitch_env = ModEnvelope { amount: 1.0, ..env };
        i.pan_env = ModEnvelope { amount: 0.5, ..env };
        i.volume_lfo = lfo(LfoShape::Sine);
        i.pan_lfo = lfo(LfoShape::Triangle);
        i.cutoff_lfo = lfo(LfoShape::Random);
        i.pitch_lfo = Lfo { rate: LfoRate::Sync { numerator: 1, denominator: 16 }, ..lfo(LfoShape::Saw) };
        i.velocity = VelocityMap { to_volume: 0.5, to_cutoff_octaves: 2.0 };
        instruments.push(i);
    }
    let events = (0..64)
        .map(|n| NoteEvent { tick: (n * 40) as Tick, instrument: n % 4, pitch: 40 + (n % 30) as u8, velocity: 1 + (n * 2) as u8, length: 300 })
        .collect();
    let snapshot = Snapshot { tempo: 133.0, length: 3840, looping: true, instruments, events };
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), snapshot));
    core.handle(Command::Play);
    core.handle(Command::PreviewNote { instrument: 2, pitch: 60, velocity: 90, length: 900 });
    let mut buf = vec![0.0f32; 4096 * 2];
    assert_no_alloc::assert_no_alloc(|| {
        for _ in 0..30 {
            core.process(&mut buf);
        }
    });
    assert!(buf.iter().all(|x| x.is_finite()));
    core.handle(Command::Stop);
    assert_no_alloc::assert_no_alloc(|| core.process(&mut buf));
}

/// Release-mode CPU check: `cargo test -p nw-engine --release -- --ignored full_load_block_time`.
#[test]
#[ignore = "benchmark; run in release"]
fn full_load_block_time() {
    use nw_engine::core::MAX_VOICES;
    let c = Collector::new();
    let lfo = |shape| Lfo { shape, rate: LfoRate::Hz(5.0), amount: 0.5, delay_ms: 0.0 };
    let mut i = inst(sine(&c, 220.0, 480_000));
    i.filter = Filter { kind: FilterKind::LowPass, cutoff_hz: 1500.0, resonance: 0.6 };
    i.cutoff_env = ModEnvelope { env: Envelope { decay_ms: 200.0, sustain: 0.4, ..Envelope::default() }, amount: 3.0 };
    i.pitch_env = ModEnvelope { amount: 0.5, ..i.cutoff_env };
    i.pan_env = ModEnvelope { amount: 0.3, ..i.cutoff_env };
    i.volume_lfo = lfo(LfoShape::Sine);
    i.pan_lfo = lfo(LfoShape::Triangle);
    i.cutoff_lfo = lfo(LfoShape::Random);
    i.pitch_lfo = lfo(LfoShape::Saw);
    // Loop the sample so high notes (read up to ~10x faster) never run out.
    i.region = PlayRegion { loop_mode: nw_core::LoopMode::Forward, loop_start: 1000, loop_end: 470_000, ..i.region };
    // All voices start at tick 0 and last the whole run.
    let events = (0..MAX_VOICES)
        .map(|n| NoteEvent { tick: 0, instrument: 0, pitch: 36 + n as u8, velocity: 100, length: 3840 * 8 })
        .collect();
    let snapshot = Snapshot { tempo: 120.0, length: 3840 * 16, looping: true, instruments: vec![i], events };
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), snapshot));
    core.handle(Command::Play);
    let mut buf = vec![0.0f32; 512 * 2];
    core.process(&mut buf); // start all voices
    assert_eq!(core.active_voices(), MAX_VOICES);
    let blocks = 400;
    let start = std::time::Instant::now();
    for _ in 0..blocks {
        core.process(&mut buf);
    }
    let per_block = start.elapsed().as_secs_f64() / f64::from(blocks);
    let budget = 512.0 / f64::from(RATE);
    let share = per_block / budget;
    println!("64 voices, full modulation: {:.3} ms per 512-frame block = {:.1} % of real time", per_block * 1000.0, share * 100.0);
    assert_eq!(core.active_voices(), MAX_VOICES, "all voices still sounding");
    assert!(share < 0.25, "{:.1} % of real time", share * 100.0);
}
