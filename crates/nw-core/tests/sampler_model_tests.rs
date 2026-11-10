//! The FL-style sampler model: defaults, legacy files, round trips and validation.

use std::fs;
use std::path::Path;

use nw_core::*;

fn id(s: &str) -> InstrumentId {
    InstrumentId(s.into())
}

/// A saved one-instrument project; returns the temp dir and the instrument file path.
fn saved_with(inst: Instrument) -> (tempfile::TempDir, std::path::PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let project = Project { instruments: vec![inst], ..Project::default() };
    save_project(t.path(), &project).unwrap();
    let file = t.path().join(DocPath::Instrument(id("kick")).relative_path());
    (t, file)
}

fn load_kick(dir: &Path) -> Result<Instrument, LoadError> {
    load_project(dir).map(|(p, _)| p.instruments[0].clone())
}

#[test]
fn default_sampler_matches_old_behaviour() {
    let s = Sampler::default();
    assert_eq!(s.root_note, 60);
    assert_eq!(s.region, Region { start: 0.0, end: 1.0 });
    assert_eq!(s.looping.mode, LoopMode::Off);
    assert!(!s.one_shot);
    assert!(s.processing.declick);
    assert!(!s.processing.reverse && !s.processing.normalize);
    assert_eq!(s.tune, Tune { coarse: 0, fine_cents: 0.0 });
    let env = &s.volume_env;
    assert_eq!((env.delay_ms, env.attack_ms, env.hold_ms, env.decay_ms), (0.0, 0.0, 0.0, 0.0));
    assert_eq!((env.sustain, env.release_ms), (1.0, 20.0));
    assert_eq!(s.filter.kind, FilterKind::Off);
    for m in [&s.cutoff_env, &s.pitch_env, &s.pan_env] {
        assert_eq!(m.amount, 0.0);
    }
    for l in [&s.volume_lfo, &s.pan_lfo, &s.cutoff_lfo, &s.pitch_lfo] {
        assert_eq!(l.amount, 0.0);
    }
    assert_eq!(s.velocity, VelocityMap { to_volume: 1.0, to_cutoff_octaves: 0.0 });
    assert_eq!(s.voice, VoiceSettings { polyphony: 64, mono: false, glide_ms: 0.0, cut: 0, cut_by: 0 });
    assert_eq!(s.validate(), Ok(()));
}

#[test]
fn old_instrument_file_loads_attack_release_into_volume_env() {
    let (t, file) = saved_with(Instrument::new(id("kick"), "Kick"));
    fs::write(
        &file,
        r#"(
    id: "kick",
    name: "Kick",
    volume: 0.78,
    pan: 0.0,
    sampler: (
        sample_path: Some("/Volumes/USB/kick.wav"),
        root_note: 48,
        attack_ms: 12.0,
        release_ms: 300.0,
    ),
)
"#,
    )
    .unwrap();
    let kick = load_kick(t.path()).unwrap();
    assert_eq!(kick.sampler.sample_path.as_deref(), Some(Path::new("/Volumes/USB/kick.wav")));
    assert_eq!(kick.sampler.root_note, 48);
    assert_eq!(kick.sampler.volume_env.attack_ms, 12.0);
    assert_eq!(kick.sampler.volume_env.release_ms, 300.0);
    // Everything else takes the defaults.
    let mut expected = Sampler {
        sample_path: Some("/Volumes/USB/kick.wav".into()),
        root_note: 48,
        ..Sampler::default()
    };
    expected.volume_env.attack_ms = 12.0;
    expected.volume_env.release_ms = 300.0;
    assert_eq!(kick.sampler, expected);
}

#[test]
fn new_shape_round_trips_and_drops_legacy_keys() {
    let mut kick = Instrument::new(id("kick"), "Kick");
    let s = &mut kick.sampler;
    s.region = Region { start: 0.1, end: 0.9 };
    s.looping = Loop { mode: LoopMode::PingPong, start: 0.2, end: 0.8, crossfade_ms: 15.0 };
    s.one_shot = true;
    s.processing = Processing { reverse: true, normalize: true, invert_polarity: true, swap_stereo: true, declick: false };
    s.tune = Tune { coarse: -7, fine_cents: 33.0 };
    s.volume_env.decay_ms = 400.0;
    s.volume_env.sustain = 0.5;
    s.volume_env.attack_curve = -0.4;
    s.filter = Filter { kind: FilterKind::BandPass, cutoff_hz: 1200.0, resonance: 0.6 };
    s.cutoff_env.amount = -3.0;
    s.pitch_env.amount = 12.0;
    s.pan_env.amount = 0.5;
    s.cutoff_lfo = Lfo { shape: LfoShape::Random, rate: LfoRate::Sync { numerator: 1, denominator: 8 }, amount: 2.0, delay_ms: 100.0 };
    s.pitch_lfo = Lfo { shape: LfoShape::Triangle, rate: LfoRate::Hz(5.5), amount: 0.3, delay_ms: 0.0 };
    s.velocity = VelocityMap { to_volume: 0.25, to_cutoff_octaves: 4.0 };
    s.voice = VoiceSettings { polyphony: 1, mono: true, glide_ms: 80.0, cut: 3, cut_by: 3 };
    let (t, file) = saved_with(kick.clone());
    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("attack_ms: 0.0,\n        release_ms"), "legacy flat keys are not written");
    assert_eq!(load_kick(t.path()).unwrap(), kick);
}

#[test]
fn missing_fields_take_defaults() {
    let (t, file) = saved_with(Instrument::new(id("kick"), "Kick"));
    fs::write(&file, r#"(id: "kick", name: "Kick", volume: 0.5, pan: 0.0, sampler: (filter: (kind: LowPass)))"#)
        .unwrap();
    let kick = load_kick(t.path()).unwrap();
    let mut expected = Sampler::default();
    expected.filter.kind = FilterKind::LowPass;
    assert_eq!(kick.sampler, expected);
}

fn expect_invalid(edit: impl FnOnce(&mut Sampler), needle: &str) {
    let mut kick = Instrument::new(id("kick"), "Kick");
    edit(&mut kick.sampler);
    let err = kick.sampler.validate().expect_err(needle);
    assert!(err.contains(needle), "{err:?} should mention {needle}");
    // The same problem in a file fails to load, naming the file.
    let (t, _) = saved_with(kick);
    match load_kick(t.path()) {
        Err(LoadError::Invalid { path, message }) => {
            assert_eq!(path, DocPath::Instrument(id("kick")).relative_path());
            assert!(message.contains(needle), "{message:?} should mention {needle}");
        }
        other => panic!("expected Invalid for {needle}, got {other:?}"),
    }
}

#[test]
fn region_start_after_end_is_invalid_naming_file() {
    expect_invalid(|s| s.region = Region { start: 0.6, end: 0.4 }, "region");
}

#[test]
fn loop_outside_region_is_invalid() {
    expect_invalid(
        |s| {
            s.region = Region { start: 0.2, end: 0.8 };
            s.looping = Loop { mode: LoopMode::Forward, start: 0.1, end: 0.5, crossfade_ms: 0.0 };
        },
        "loop",
    );
    expect_invalid(
        |s| s.looping = Loop { mode: LoopMode::Forward, start: 0.5, end: 0.5, crossfade_ms: 0.0 },
        "loop",
    );
}

#[test]
fn loop_points_are_not_checked_while_loop_is_off() {
    let s = Sampler {
        region: Region { start: 0.5, end: 1.0 },
        looping: Loop { mode: LoopMode::Off, start: 0.0, end: 0.1, crossfade_ms: 0.0 },
        ..Sampler::default()
    };
    assert_eq!(s.validate(), Ok(()));
}

type Case = (Box<dyn FnOnce(&mut Sampler)>, &'static str);

#[test]
fn out_of_range_values_are_invalid() {
    let cases: Vec<Case> = vec![
        (Box::new(|s| s.root_note = 128), "root_note"),
        (Box::new(|s| s.region.end = 1.5), "region"),
        (Box::new(|s| s.looping.crossfade_ms = 600.0), "crossfade_ms"),
        (Box::new(|s| s.tune.coarse = 25), "coarse"),
        (Box::new(|s| s.tune.fine_cents = -101.0), "fine_cents"),
        (Box::new(|s| s.volume_env.delay_ms = 2001.0), "volume_env.delay_ms"),
        (Box::new(|s| s.volume_env.attack_ms = 5001.0), "volume_env.attack_ms"),
        (Box::new(|s| s.volume_env.hold_ms = -1.0), "volume_env.hold_ms"),
        (Box::new(|s| s.volume_env.decay_ms = f32::NAN), "volume_env.decay_ms"),
        (Box::new(|s| s.volume_env.sustain = 1.1), "volume_env.sustain"),
        (Box::new(|s| s.volume_env.release_ms = 5001.0), "volume_env.release_ms"),
        (Box::new(|s| s.volume_env.attack_curve = 1.5), "volume_env.attack_curve"),
        (Box::new(|s| s.filter.cutoff_hz = 10.0), "cutoff_hz"),
        (Box::new(|s| s.filter.resonance = 2.0), "resonance"),
        (Box::new(|s| s.cutoff_env.amount = 9.0), "cutoff_env.amount"),
        (Box::new(|s| s.pitch_env.amount = -25.0), "pitch_env.amount"),
        (Box::new(|s| s.pan_env.amount = 1.5), "pan_env.amount"),
        (Box::new(|s| s.pitch_env.env.sustain = -0.1), "pitch_env.env.sustain"),
        (Box::new(|s| s.volume_lfo.amount = 1.2), "volume_lfo.amount"),
        (Box::new(|s| s.pan_lfo.amount = -0.1), "pan_lfo.amount"),
        (Box::new(|s| s.cutoff_lfo.amount = 8.5), "cutoff_lfo.amount"),
        (Box::new(|s| s.pitch_lfo.amount = 25.0), "pitch_lfo.amount"),
        (Box::new(|s| s.pitch_lfo.delay_ms = 6000.0), "pitch_lfo.delay_ms"),
        (Box::new(|s| s.pitch_lfo.rate = LfoRate::Hz(60.0)), "pitch_lfo.rate"),
        (Box::new(|s| s.pitch_lfo.rate = LfoRate::Hz(0.001)), "pitch_lfo.rate"),
        (Box::new(|s| s.pitch_lfo.rate = LfoRate::Sync { numerator: 1, denominator: 3 }), "pitch_lfo.rate"),
        (Box::new(|s| s.pitch_lfo.rate = LfoRate::Sync { numerator: 1, denominator: 128 }), "pitch_lfo.rate"),
        (Box::new(|s| s.pitch_lfo.rate = LfoRate::Sync { numerator: 9, denominator: 1 }), "pitch_lfo.rate"),
        (Box::new(|s| s.pitch_lfo.rate = LfoRate::Sync { numerator: 0, denominator: 4 }), "pitch_lfo.rate"),
        (Box::new(|s| s.velocity.to_volume = 1.5), "to_volume"),
        (Box::new(|s| s.velocity.to_cutoff_octaves = -9.0), "to_cutoff_octaves"),
        (Box::new(|s| s.voice.polyphony = 0), "polyphony"),
        (Box::new(|s| s.voice.polyphony = 65), "polyphony"),
        (Box::new(|s| s.voice.glide_ms = 2500.0), "glide_ms"),
        (Box::new(|s| s.voice.cut = 17), "voice.cut"),
        (Box::new(|s| s.voice.cut_by = 17), "cut_by"),
    ];
    for (edit, needle) in cases {
        expect_invalid(edit, needle);
    }
}

#[test]
fn edge_values_are_valid() {
    let mut s = Sampler { root_note: 127, tune: Tune { coarse: -24, fine_cents: 100.0 }, ..Sampler::default() };
    s.volume_env.attack_ms = 5000.0;
    s.volume_env.attack_curve = -1.0;
    s.filter = Filter { kind: FilterKind::Notch, cutoff_hz: 20_000.0, resonance: 1.0 };
    s.cutoff_env.amount = -8.0;
    s.pitch_lfo.rate = LfoRate::Sync { numerator: 1, denominator: 64 };
    s.cutoff_lfo.rate = LfoRate::Sync { numerator: 8, denominator: 1 };
    s.volume_lfo.rate = LfoRate::Hz(50.0);
    s.voice = VoiceSettings { polyphony: 1, mono: true, glide_ms: 2000.0, cut: 16, cut_by: 16 };
    assert_eq!(s.validate(), Ok(()));
}
