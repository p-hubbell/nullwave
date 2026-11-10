//! Voice behaviour: fade on steal, polyphony limit, mono with glide, cut groups.

use basedrop::{Collector, Owned, Shared};
use nw_core::{Envelope, Tick, VoiceSettings};
use nw_engine::core::{Command, EngineCore, FADE_RESERVE, MAX_VOICES};
use nw_engine::process::PlayRegion;
use nw_engine::sample::SampleBuffer;
use nw_engine::snapshot::{InstrumentSnapshot, NoteEvent, Snapshot};

#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

const RATE: u32 = 48_000;
/// 48 kHz at 120 BPM: 25 frames per tick.
const SPT: usize = 25;
const CENTRE: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn dc(c: &Collector, frames: usize) -> Shared<SampleBuffer> {
    Shared::new(&c.handle(), SampleBuffer { sample_rate: RATE, left: vec![1.0; frames], right: vec![1.0; frames] })
}

fn ramp(c: &Collector, frames: usize) -> Shared<SampleBuffer> {
    let left: Vec<f32> = (0..frames).map(|i| 1.0 + i as f32).collect();
    Shared::new(&c.handle(), SampleBuffer { sample_rate: RATE, right: left.clone(), left })
}

fn inst(sample: Shared<SampleBuffer>, voice: VoiceSettings) -> InstrumentSnapshot {
    let region = PlayRegion::whole(sample.len());
    let mut i = InstrumentSnapshot::new(Some(sample), region);
    i.voice = voice;
    i
}

fn note(tick: Tick, instrument: usize, pitch: u8, length: Tick) -> NoteEvent {
    NoteEvent { tick, instrument, pitch, velocity: 127, length }
}

fn core_with(c: &Collector, instruments: Vec<InstrumentSnapshot>, mut events: Vec<NoteEvent>) -> EngineCore {
    events.sort_by_key(|e| e.tick);
    let snapshot = Snapshot { tempo: 120.0, length: 3840 * 16, looping: true, instruments, events };
    let mut core = EngineCore::new(RATE, Owned::new(&c.handle(), snapshot));
    core.handle(Command::Play);
    core
}

fn render(core: &mut EngineCore, frames: usize) -> Vec<f32> {
    let mut buf = vec![0.0f32; frames * 2];
    core.process(&mut buf);
    buf.iter().step_by(2).copied().collect()
}

fn max_step(v: &[f32]) -> f32 {
    v.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

#[test]
fn steal_fades_instead_of_cutting() {
    let c = Collector::new();
    let sustained = inst(dc(&c, 200_000), VoiceSettings::default());
    // The newcomer fades in slowly, so a hard cut of the stolen voice would show as a step.
    let mut slow = inst(dc(&c, 200_000), VoiceSettings::default());
    slow.volume_env = Envelope { attack_ms: 50.0, ..Envelope::default() };
    let mut events: Vec<NoteEvent> = (0..MAX_VOICES).map(|_| note(0, 0, 60, 3840)).collect();
    events.push(note(10, 1, 60, 3840));
    let mut core = core_with(&c, vec![sustained, slow], events);
    let out = render(&mut core, 1000);
    assert!((out[9 * SPT] - MAX_VOICES as f32 * CENTRE).abs() < 1e-3);
    assert!(max_step(&out[5..]) < 0.05, "a stolen voice must fade, not click: {}", max_step(&out[5..]));
    assert_eq!(core.active_voices(), MAX_VOICES, "the faded voice is gone after 5 ms");
}

#[test]
fn voice_stolen_before_it_sounded_is_replaced_outright() {
    // 80 notes in the same frame: the 16 extra steal voices that never rendered a frame,
    // so there is nothing to fade (and nothing extra is heard).
    let c = Collector::new();
    let events = (0..80).map(|_| note(0, 0, 60, 960)).collect();
    let mut core = core_with(&c, vec![inst(dc(&c, 100_000), VoiceSettings::default())], events);
    let out = render(&mut core, 64);
    assert_eq!(core.active_voices(), MAX_VOICES);
    assert!((out[10] - MAX_VOICES as f32 * CENTRE).abs() < 1e-3);
}

#[test]
fn full_pool_and_full_reserve_still_never_panics() {
    let c = Collector::new();
    let events = (0..400).map(|i| note((i % 40) as Tick, 0, 40 + (i % 40) as u8, 2000)).collect();
    let mut core = core_with(&c, vec![inst(dc(&c, 200_000), VoiceSettings::default())], events);
    for _ in 0..20 {
        let out = render(&mut core, 512);
        assert!(out.iter().all(|x| x.is_finite()));
        assert!(core.active_voices() <= MAX_VOICES + FADE_RESERVE);
    }
}

#[test]
fn polyphony_limit_fades_oldest_of_that_instrument_only() {
    let c = Collector::new();
    let limited = inst(dc(&c, 200_000), VoiceSettings { polyphony: 2, ..VoiceSettings::default() });
    let free = inst(dc(&c, 200_000), VoiceSettings::default());
    let events = vec![
        note(0, 1, 50, 3840),
        note(0, 0, 60, 3840),
        note(1, 1, 51, 3840),
        note(10, 0, 62, 3840),
        note(20, 0, 64, 3840),
    ];
    let mut core = core_with(&c, vec![limited, free], events);
    render(&mut core, 20 * SPT + 10);
    // Just after the third note: the oldest is fading, not yet gone.
    assert_eq!(core.active_voices(), 5);
    render(&mut core, 400);
    assert_eq!(core.sounding_notes(), vec![(0, 62), (0, 64), (1, 50), (1, 51)]);
}

#[test]
fn mono_without_glide_retriggers() {
    let c = Collector::new();
    let mut mono = inst(dc(&c, 200_000), VoiceSettings { mono: true, ..VoiceSettings::default() });
    // A short attack, so the newcomer ramps in and a hard cut of the old note would show.
    mono.volume_env.attack_ms = 2.0;
    let mut core = core_with(&c, vec![mono], vec![note(0, 0, 60, 3840), note(10, 0, 64, 3840)]);
    let out = render(&mut core, 10 * SPT + 400);
    assert_eq!(core.sounding_notes(), vec![(0, 64)]);
    assert!(max_step(&out[5..]) < 0.05, "the old note fades out");
}

#[test]
fn mono_glide_bends_pitch_and_keeps_envelope() {
    let c = Collector::new();
    let glide = VoiceSettings { mono: true, glide_ms: 10.0, ..VoiceSettings::default() };
    // Rate: on a ramp the step between frames is the read rate (times the pan gain).
    let mut core = core_with(&c, vec![inst(ramp(&c, 400_000), glide)], vec![note(0, 0, 60, 3840), note(4, 0, 72, 3840)]);
    let out = render(&mut core, 2000);
    let rate_at = |n: usize| (out[n + 1] - out[n]) / CENTRE;
    assert!((rate_at(50) - 1.0).abs() < 1e-3);
    let mid = rate_at(100 + 240);
    assert!(mid > 1.2 && mid < 1.8, "half-way through the glide: {mid}");
    assert!((rate_at(100 + 480 + 50) - 2.0).abs() < 1e-3, "an octave up once the glide is done");
    assert_eq!(core.active_voices(), 1, "legato: the same voice carries on");
    assert_eq!(core.sounding_notes(), vec![(0, 72)]);

    // The envelope is not restarted: a slow attack keeps rising through the second note.
    let mut slow = inst(dc(&c, 400_000), glide);
    slow.volume_env = Envelope { attack_ms: 30.0, ..Envelope::default() };
    let mut core = core_with(&c, vec![slow], vec![note(0, 0, 60, 3840), note(4, 0, 72, 3840)]);
    let out = render(&mut core, 400);
    // Restarting the envelope would drop the level to 0 at frame 100.
    assert!(out[101] >= out[99] && out[101] > 0.04, "{} → {}", out[99], out[101]);
}

#[test]
fn mono_glide_takes_the_new_note_length() {
    let c = Collector::new();
    let glide = VoiceSettings { mono: true, glide_ms: 5.0, ..VoiceSettings::default() };
    let mut i = inst(dc(&c, 400_000), glide);
    i.volume_env.release_ms = 0.0;
    let mut core = core_with(&c, vec![i], vec![note(0, 0, 60, 3840), note(10, 0, 62, 10)]);
    let out = render(&mut core, 30 * SPT);
    assert!(out[20 * SPT - 1] != 0.0);
    assert!(out[20 * SPT + 1..].iter().all(|x| *x == 0.0), "ends with the second note");
}

#[test]
fn open_hat_cut_by_closed_hat() {
    let c = Collector::new();
    let mut closed = inst(dc(&c, 200_000), VoiceSettings { cut: 1, ..VoiceSettings::default() });
    // A short attack so the closed hat ramps in and a hard cut of the open hat would show.
    closed.volume_env.attack_ms = 2.0;
    let open = inst(dc(&c, 200_000), VoiceSettings { cut_by: 1, ..VoiceSettings::default() });
    let mut core = core_with(&c, vec![closed, open], vec![note(0, 1, 60, 3840), note(10, 0, 60, 3840)]);
    let out = render(&mut core, 10 * SPT + 400);
    assert_eq!(core.sounding_notes(), vec![(0, 60)]);
    assert!(max_step(&out[5..]) < 0.05, "the open hat fades rather than clicks");
}

#[test]
fn cut_group_chokes_its_own_instrument_when_it_cuts_itself() {
    let c = Collector::new();
    let open = inst(dc(&c, 200_000), VoiceSettings { cut: 1, cut_by: 1, ..VoiceSettings::default() });
    let mut core = core_with(&c, vec![open], vec![note(0, 0, 60, 3840), note(10, 0, 60, 3840)]);
    render(&mut core, 10 * SPT + 400);
    assert_eq!(core.active_voices(), 1);
}

#[test]
fn cut_group_does_not_touch_other_groups() {
    let c = Collector::new();
    let cutter = inst(dc(&c, 200_000), VoiceSettings { cut: 1, ..VoiceSettings::default() });
    let other = inst(dc(&c, 200_000), VoiceSettings { cut_by: 2, ..VoiceSettings::default() });
    let none = inst(dc(&c, 200_000), VoiceSettings::default());
    let events = vec![note(0, 1, 60, 3840), note(0, 2, 60, 3840), note(10, 0, 60, 3840)];
    let mut core = core_with(&c, vec![cutter, other, none], events);
    render(&mut core, 10 * SPT + 400);
    assert_eq!(core.sounding_notes(), vec![(0, 60), (1, 60), (2, 60)]);
}

#[test]
fn preview_note_chokes_like_a_real_note() {
    let c = Collector::new();
    let closed = inst(dc(&c, 200_000), VoiceSettings { cut: 1, ..VoiceSettings::default() });
    let open = inst(dc(&c, 200_000), VoiceSettings { cut_by: 1, ..VoiceSettings::default() });
    let mut core = core_with(&c, vec![closed, open], vec![note(0, 1, 60, 3840)]);
    render(&mut core, 100);
    core.handle(Command::PreviewNote { instrument: 0, pitch: 60, velocity: 100, length: 3840 });
    render(&mut core, 400);
    assert_eq!(core.sounding_notes(), vec![(0, 60)]);
}

#[test]
fn stop_still_releases_everything() {
    let c = Collector::new();
    let glide = VoiceSettings { mono: true, glide_ms: 50.0, cut: 1, cut_by: 1, polyphony: 1 };
    let mut events: Vec<NoteEvent> = (0..100).map(|i| note(i as Tick, 1, 40 + (i % 30) as u8, 3840)).collect();
    events.extend((0..20).map(|i| note(i * 5, 0, 60, 3840)));
    let mut core = core_with(&c, vec![inst(dc(&c, 400_000), glide), inst(dc(&c, 400_000), VoiceSettings::default())], events);
    render(&mut core, 2000);
    core.handle(Command::Stop);
    // Default release is 20 ms; fades are 5 ms.
    render(&mut core, RATE as usize / 20);
    assert_eq!(core.active_voices(), 0);
    assert!(render(&mut core, 100).iter().all(|x| *x == 0.0));
}

#[test]
fn process_does_not_allocate_under_cuts_and_steals() {
    let c = Collector::new();
    let instruments = vec![
        inst(dc(&c, 400_000), VoiceSettings { mono: true, glide_ms: 20.0, ..VoiceSettings::default() }),
        inst(dc(&c, 400_000), VoiceSettings { polyphony: 3, cut: 1, ..VoiceSettings::default() }),
        inst(dc(&c, 400_000), VoiceSettings { cut_by: 1, ..VoiceSettings::default() }),
        inst(dc(&c, 400_000), VoiceSettings::default()),
    ];
    let events = (0..300).map(|i| note((i * 7 % 3840) as Tick, i % 4, 36 + (i % 48) as u8, 400)).collect();
    let mut core = core_with(&c, instruments, events);
    let mut buf = vec![0.0f32; 4096 * 2];
    assert_no_alloc::assert_no_alloc(|| {
        for _ in 0..30 {
            core.process(&mut buf);
        }
        core.handle(Command::PreviewNote { instrument: 1, pitch: 60, velocity: 100, length: 500 });
        core.process(&mut buf);
        core.handle(Command::Stop);
        core.process(&mut buf);
    });
    assert!(buf.iter().all(|x| x.is_finite()));
}
