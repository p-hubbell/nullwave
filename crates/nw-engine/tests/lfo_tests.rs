//! LFO shapes, rates (free and tempo-synced), sample-and-hold and delay.

use nw_core::{Lfo, LfoRate, LfoShape};
use nw_engine::lfo::{LfoParams, LfoState, lfo_hz};

const SR: u32 = 1000;

fn params(shape: LfoShape, hz: f32) -> LfoParams {
    LfoParams { shape, hz, delay: 0 }
}

fn run(p: LfoParams, n: usize) -> Vec<f32> {
    let mut s = LfoState::default();
    s.start(p, SR, 1);
    (0..n).map(|_| s.tick()).collect()
}

fn sign_changes(v: &[f32]) -> usize {
    v.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count()
}

#[test]
fn lfo_shapes_and_rate() {
    for shape in [LfoShape::Sine, LfoShape::Triangle, LfoShape::Square] {
        let out = run(params(shape, 5.0), SR as usize);
        let changes = sign_changes(&out);
        assert!((9..=11).contains(&changes), "{shape:?}: {changes}");
        assert!(out.iter().all(|v| (-1.0..=1.0).contains(v)));
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.99, "{shape:?} reaches full depth");
    }
    // A saw rises steadily, then drops once per cycle.
    let saw = run(params(LfoShape::Saw, 5.0), SR as usize);
    let drops = saw.windows(2).filter(|w| w[1] < w[0]).count();
    assert!((4..=5).contains(&drops), "{drops}");
}

#[test]
fn square_is_only_full_values() {
    let out = run(params(LfoShape::Square, 3.0), 1000);
    assert!(out.iter().all(|v| *v == 1.0 || *v == -1.0));
}

#[test]
fn sine_starts_at_zero() {
    let out = run(params(LfoShape::Sine, 2.0), 3);
    assert!(out[0].abs() < 1e-6);
}

#[test]
fn synced_lfo_follows_tempo() {
    // A quarter note at 120 BPM lasts 0.5 s: 2 Hz. At 60 BPM: 1 Hz.
    let quarter = LfoRate::Sync { numerator: 1, denominator: 4 };
    assert!((lfo_hz(quarter, 120.0) - 2.0).abs() < 1e-6);
    assert!((lfo_hz(quarter, 60.0) - 1.0).abs() < 1e-6);
    // One bar of 4/4 (a whole note) at 120 BPM: 0.5 Hz.
    assert!((lfo_hz(LfoRate::Sync { numerator: 1, denominator: 1 }, 120.0) - 0.5).abs() < 1e-6);
    assert_eq!(lfo_hz(LfoRate::Hz(7.5), 120.0), 7.5);
    let lfo = Lfo { shape: LfoShape::Triangle, rate: quarter, amount: 1.0, delay_ms: 250.0 };
    let p = LfoParams::from_lfo(&lfo, 120.0, 48_000);
    assert!((p.hz - 2.0).abs() < 1e-6);
    assert_eq!(p.delay, 12_000);
    assert_eq!(p.shape, LfoShape::Triangle);
}

#[test]
fn random_lfo_holds_between_steps() {
    let out = run(params(LfoShape::Random, 10.0), 1000);
    // Ten cycles of ~100 samples, each holding one value.
    let mut runs = Vec::new();
    let mut len = 1;
    for w in out.windows(2) {
        if w[1] == w[0] {
            len += 1;
        } else {
            runs.push(len);
            len = 1;
        }
    }
    assert!((9..=10).contains(&runs.len()), "{runs:?}");
    assert!(runs.iter().all(|r| (99..=101).contains(r)), "{runs:?}");
    assert!(out.iter().all(|v| (-1.0..=1.0).contains(v)));
}

#[test]
fn random_depends_on_seed() {
    let mut a = LfoState::default();
    let mut b = LfoState::default();
    a.start(params(LfoShape::Random, 10.0), SR, 1);
    b.start(params(LfoShape::Random, 10.0), SR, 2);
    let va: Vec<f32> = (0..1000).map(|_| a.tick()).collect();
    let vb: Vec<f32> = (0..1000).map(|_| b.tick()).collect();
    assert_ne!(va, vb);
}

#[test]
fn lfo_delay() {
    let out = run(LfoParams { shape: LfoShape::Square, hz: 5.0, delay: 300 }, 400);
    assert!(out[..300].iter().all(|v| *v == 0.0));
    assert!(out[300..].iter().all(|v| v.abs() == 1.0));
}

#[test]
fn step_matches_next() {
    let p = params(LfoShape::Saw, 3.0);
    let mut a = LfoState::default();
    let mut b = LfoState::default();
    a.start(p, SR, 1);
    b.start(p, SR, 1);
    for _ in 0..50 {
        let stepped = a.step(16);
        let walked = b.tick();
        for _ in 1..16 {
            b.tick();
        }
        assert!((stepped - walked).abs() < 1e-4);
    }
}
