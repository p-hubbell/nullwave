//! DAHDSR envelope with curves.

use nw_core::Envelope;
use nw_engine::env::{EnvParams, EnvState, shape};

fn run(state: &mut EnvState, n: usize) -> Vec<f32> {
    (0..n).map(|_| state.tick()).collect()
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn dahdsr_stage_lengths() {
    let p = EnvParams { delay: 3, attack: 4, hold: 2, decay: 5, sustain: 0.5, release: 6, attack_curve: 0.0, decay_curve: 0.0 };
    let mut e = EnvState::default();
    e.start(p);
    let out = run(&mut e, 3 + 4 + 2 + 5 + 3);
    let expected = [
        0.0, 0.0, 0.0, // delay
        0.0, 0.25, 0.5, 0.75, // attack
        1.0, 1.0, // hold
        1.0, 0.9, 0.8, 0.7, 0.6, // decay
        0.5, 0.5, 0.5, // sustain
    ];
    assert!(close(&out, &expected), "{out:?}");
    e.release();
    let out = run(&mut e, 6);
    let expected: Vec<f32> = (0..6).map(|k| 0.5 * (1.0 - k as f32 / 6.0)).collect();
    assert!(close(&out, &expected), "{out:?}");
    assert!(e.is_done());
}

#[test]
fn sustain_holds_until_release() {
    let p = EnvParams { attack: 2, decay: 2, sustain: 0.3, release: 4, ..EnvParams::default() };
    let mut e = EnvState::default();
    e.start(p);
    let out = run(&mut e, 10_000);
    assert!(out[100..].iter().all(|v| (v - 0.3).abs() < 1e-6));
    assert!(!e.is_done());
}

#[test]
fn zero_sustain_after_decay_is_silent_but_not_done() {
    // FL keeps the voice alive until note-off even at zero sustain.
    let p = EnvParams { decay: 3, sustain: 0.0, release: 4, ..EnvParams::default() };
    let mut e = EnvState::default();
    e.start(p);
    let out = run(&mut e, 10);
    assert_eq!(out[5], 0.0);
    assert!(!e.is_done());
}

#[test]
fn release_from_mid_attack_starts_at_current_level() {
    let p = EnvParams { attack: 10, release: 4, ..EnvParams::attack_release(0, 0) };
    let mut e = EnvState::default();
    e.start(p);
    let out = run(&mut e, 5);
    assert!((out[4] - 0.4).abs() < 1e-6);
    e.release();
    let out = run(&mut e, 4);
    assert!(close(&out, &[0.4, 0.3, 0.2, 0.1]), "{out:?}");
    assert!(e.is_done());
}

#[test]
fn release_during_delay_is_silent() {
    let p = EnvParams { delay: 100, release: 3, ..EnvParams::attack_release(0, 0) };
    let mut e = EnvState::default();
    e.start(p);
    run(&mut e, 10);
    e.release();
    assert!(run(&mut e, 3).iter().all(|v| *v == 0.0));
    assert!(e.is_done());
}

#[test]
fn zero_release_is_done_at_once() {
    let mut e = EnvState::default();
    e.start(EnvParams::attack_release(0, 0));
    e.tick();
    e.release();
    assert!(e.is_done());
}

#[test]
fn attack_release_matches_the_original_linear_envelope() {
    // The pre-17c voice: attack e = k/attack; then 1.0; release from the last level,
    // e = from * (1 - k/release).
    let mut e = EnvState::default();
    e.start(EnvParams::attack_release(8, 5));
    let out = run(&mut e, 12);
    let mut expected: Vec<f32> = (0..8).map(|k| k as f32 / 8.0).collect();
    expected.extend([1.0; 4]);
    assert!(close(&out, &expected));
    e.release();
    let out = run(&mut e, 5);
    let expected: Vec<f32> = (0..5).map(|k| 1.0 - k as f32 / 5.0).collect();
    assert!(close(&out, &expected));
}

#[test]
fn from_envelope_converts_ms_to_samples() {
    let env = Envelope { delay_ms: 1.0, attack_ms: 2.0, hold_ms: 3.0, decay_ms: 4.0, sustain: 0.25, release_ms: 5.0, attack_curve: 0.5, decay_curve: -0.5 };
    let p = EnvParams::from_envelope(&env, 48_000);
    assert_eq!((p.delay, p.attack, p.hold, p.decay, p.release), (48, 96, 144, 192, 240));
    assert_eq!((p.sustain, p.attack_curve, p.decay_curve), (0.25, 0.5, -0.5));
    // Out-of-range input is clamped rather than trusted.
    let bad = Envelope { attack_ms: -3.0, sustain: 2.0, ..Envelope::default() };
    let p = EnvParams::from_envelope(&bad, 48_000);
    assert_eq!((p.attack, p.sustain), (0, 1.0));
}

#[test]
fn curve_zero_is_linear() {
    for t in [0.0, 0.1, 0.5, 0.9, 1.0] {
        assert!((shape(t, 0.0) - t).abs() < 1e-6);
    }
}

#[test]
fn curves_bend_monotonically_and_keep_endpoints() {
    for c in [-1.0, -0.5, 0.5, 1.0] {
        assert!(shape(0.0, c).abs() < 1e-6 && (shape(1.0, c) - 1.0).abs() < 1e-6);
        let mut last = -1.0;
        for k in 0..=100 {
            let v = shape(k as f32 / 100.0, c);
            assert!(v >= last, "curve {c} not monotonic at {k}");
            last = v;
        }
    }
    // Negative is fast-then-slow (above the line), positive is slow-then-fast.
    assert!(shape(0.25, -1.0) > 0.25);
    assert!(shape(0.25, 1.0) < 0.25);
}

#[test]
fn curved_attack_still_reaches_full_level() {
    let p = EnvParams { attack: 50, attack_curve: 0.8, ..EnvParams::attack_release(0, 0) };
    let mut e = EnvState::default();
    e.start(p);
    let out = run(&mut e, 60);
    assert!(out[25] < 0.5, "slow start");
    assert_eq!(out[55], 1.0);
}
