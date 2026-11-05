use basedrop::{Collector, Shared};
use nw_engine::process::PlayRegion;
use nw_engine::sample::SampleBuffer;
use nw_engine::voice::{pan_gains, playback_rate, Voice, VoiceStart};

fn buf(c: &Collector, left: Vec<f32>) -> Shared<SampleBuffer> {
    let right = left.clone();
    Shared::new(&c.handle(), SampleBuffer { sample_rate: 48000, left, right })
}

fn start(sample: Shared<SampleBuffer>) -> VoiceStart {
    let region = PlayRegion::whole(sample.len());
    VoiceStart {
        sample,
        region,
        rate: 1.0,
        gain: 0.5,
        pan: 0.0,
        attack_samples: 0,
        release_samples: 0,
        duration_samples: None,
        age: 7,
    }
}

#[test]
fn center_pan_is_equal_power() {
    let (l, r) = pan_gains(0.0);
    assert!((l - r).abs() < 1e-6 && (l * l + r * r - 1.0).abs() < 1e-6);
}

#[test]
fn hard_left() {
    assert!(pan_gains(-1.0).1.abs() < 1e-6);
    assert!((pan_gains(-1.0).0 - 1.0).abs() < 1e-6);
}

#[test]
fn octave_up_doubles_rate() {
    assert!((playback_rate(72, 60, 48000, 48000) - 2.0).abs() < 1e-9);
}

#[test]
fn sample_rate_mismatch_corrects_pitch() {
    assert!((playback_rate(60, 60, 44100, 48000) - 0.91875).abs() < 1e-9);
}

#[test]
fn duration_triggers_release_then_silence() {
    let c = Collector::new();
    let mut v = Voice::default();
    assert!(!v.is_active());
    let mut s = start(buf(&c, vec![1.0; 1000]));
    s.release_samples = 50;
    s.duration_samples = Some(100);
    v.start(s);
    assert!(v.is_active());
    assert_eq!(v.age(), 7);
    let (gl, gr) = pan_gains(0.0);
    let mut l = vec![0.0; 200];
    let mut r = vec![0.0; 200];
    v.render(&mut l, &mut r);
    for i in 0..100 {
        assert!((l[i] - 0.5 * gl).abs() < 1e-5, "l[{i}]={}", l[i]);
        assert!((r[i] - 0.5 * gr).abs() < 1e-5);
    }
    for i in 100..150 {
        let e = 1.0 - (i - 100) as f32 / 50.0;
        assert!((l[i] - 0.5 * gl * e).abs() < 1e-5, "l[{i}]={}", l[i]);
    }
    assert!(l[100] > l[149] && l[149] > 0.0);
    assert!(l[150..].iter().all(|&x| x == 0.0));
    assert!(r[150..].iter().all(|&x| x == 0.0));
    assert!(!v.is_active());
}

#[test]
fn attack_ramps_from_zero() {
    let c = Collector::new();
    let mut v = Voice::default();
    let mut s = start(buf(&c, vec![1.0; 1000]));
    s.attack_samples = 10;
    s.gain = 1.0;
    s.pan = -1.0;
    v.start(s);
    let mut l = vec![0.0; 20];
    let mut r = vec![0.0; 20];
    v.render(&mut l, &mut r);
    assert_eq!(l[0], 0.0);
    for i in 1..10 {
        assert!(l[i] > l[i - 1]);
    }
    assert!((l[5] - 0.5).abs() < 1e-5);
    assert!((l[10] - 1.0).abs() < 1e-5 && (l[19] - 1.0).abs() < 1e-5);
}

#[test]
fn release_during_attack_starts_from_current_level() {
    let c = Collector::new();
    let mut v = Voice::default();
    let mut s = start(buf(&c, vec![1.0; 1000]));
    s.attack_samples = 100;
    s.release_samples = 10;
    s.gain = 1.0;
    s.pan = -1.0;
    v.start(s);
    let mut l = vec![0.0; 50];
    let mut r = vec![0.0; 50];
    v.render(&mut l, &mut r);
    v.release();
    let mut l2 = vec![0.0; 20];
    let mut r2 = vec![0.0; 20];
    v.render(&mut l2, &mut r2);
    assert!((l2[0] - l[49]).abs() < 0.03, "jump {} -> {}", l[49], l2[0]);
    assert!(l2[0] <= 0.5 + 1e-5);
    assert!(l2[10..].iter().all(|&x| x == 0.0));
    assert!(!v.is_active());
}

#[test]
fn zero_release_stops_immediately() {
    let c = Collector::new();
    let mut v = Voice::default();
    v.start(start(buf(&c, vec![1.0; 100])));
    v.release();
    assert!(!v.is_active());
    let mut l = vec![0.0; 4];
    let mut r = vec![0.0; 4];
    v.render(&mut l, &mut r);
    assert!(l.iter().all(|&x| x == 0.0));
}

#[test]
fn voice_ends_at_sample_end() {
    let c = Collector::new();
    let mut v = Voice::default();
    let mut s = start(buf(&c, vec![1.0; 10]));
    s.gain = 1.0;
    v.start(s);
    let mut l = vec![0.0; 30];
    let mut r = vec![0.0; 30];
    v.render(&mut l, &mut r);
    assert!(!v.is_active());
    assert!(l[0] > 0.0);
    assert!(l[20..].iter().all(|&x| x == 0.0));
}

#[test]
fn interpolates_linearly() {
    let c = Collector::new();
    let mut v = Voice::default();
    let mut s = start(buf(&c, vec![0.0, 1.0, 0.0, 0.0]));
    s.rate = 0.5;
    s.gain = 1.0;
    s.pan = -1.0;
    v.start(s);
    let mut l = vec![0.0; 3];
    let mut r = vec![0.0; 3];
    v.render(&mut l, &mut r);
    assert!((l[0] - 0.0).abs() < 1e-6);
    assert!((l[1] - 0.5).abs() < 1e-6);
    assert!((l[2] - 1.0).abs() < 1e-6);
}

#[test]
fn render_adds_into_output() {
    let c = Collector::new();
    let mut v = Voice::default();
    v.start(start(buf(&c, vec![1.0; 100])));
    let (gl, gr) = pan_gains(0.0);
    let mut l = vec![1.0; 8];
    let mut r = vec![2.0; 8];
    v.render(&mut l, &mut r);
    assert!((l[3] - (1.0 + 0.5 * gl)).abs() < 1e-5);
    assert!((r[3] - (2.0 + 0.5 * gr)).abs() < 1e-5);
}
