use nw_core::*;
use std::path::Path;

fn iid(s: &str) -> InstrumentId { InstrumentId(s.into()) }
fn pid(s: &str) -> PatternId { PatternId(s.into()) }
fn key(inst: &str, number: ClipNumber) -> ClipKey { ClipKey { instrument: iid(inst), number } }
fn note(pitch: u8, start: Tick, length: Tick) -> Note { Note { pitch, start, length, velocity: 100 } }
fn flat(tick: Tick, inst: &str, pitch: u8, length: Tick) -> FlatNote {
    FlatNote { tick, instrument: iid(inst), pitch, velocity: 100, length }
}

/// Pattern 1 (2 bars): bell 1 (one bar, a note running past its end) placed at 0 and 3840.
fn project() -> Project {
    let mut p = Project::default();
    p.instruments.push(Instrument::new(iid("bell"), "Bell"));
    p.instruments.push(Instrument::new(iid("kick"), "Kick"));
    let mut t = Timeline::default();
    t.clips.push(Clip { key: key("bell", 1), start: 0, length: 3840, notes: vec![note(72, 0, 240), note(74, 3000, 2000)] });
    t.placements = vec![Placement { clip: key("bell", 1), at: 0 }, Placement { clip: key("bell", 1), at: 3840 }];
    p.patterns.push(Pattern { id: pid("pattern-1"), name: "Pattern 1".into(), timeline: t });
    p
}

#[test]
fn repeated_clip_fires_twice_and_notes_stop_at_clip_end() {
    let p = project();
    assert_eq!(pattern_notes(&p, &pid("pattern-1")), vec![
        flat(0, "bell", 72, 240),
        flat(3000, "bell", 74, 840),
        flat(3840, "bell", 72, 240),
        flat(6840, "bell", 74, 840),
    ]);
    assert!(pattern_notes(&p, &pid("nope")).is_empty());
}

#[test]
fn trimmed_window_shifts_notes() {
    let mut p = project();
    let c = &mut p.patterns[0].timeline.clips[0];
    c.start = 3000;
    c.length = 840;
    assert_eq!(pattern_notes(&p, &pid("pattern-1"))[0], flat(0, "bell", 74, 840));
}

#[test]
fn song_places_patterns_and_loose_clips() {
    let mut p = project();
    p.song.patterns = vec![
        PatternPlacement { pattern: pid("pattern-1"), at: 0 },
        PatternPlacement { pattern: pid("pattern-1"), at: 15360 },
    ];
    p.song.timeline.clips.push(Clip { key: key("bell", 1), start: 0, length: 960, notes: vec![note(60, 0, 240)] });
    p.song.timeline.placements.push(Placement { clip: key("bell", 1), at: 0 });
    let notes = song_notes(&p);
    assert_eq!(notes.len(), 9, "8 from two pattern placements + 1 loose");
    assert_eq!(&notes[..2], &[flat(0, "bell", 60, 240), flat(0, "bell", 72, 240)], "overlaps both play");
    assert_eq!(notes.last().unwrap(), &flat(15360 + 6840, "bell", 74, 840));
    let sorted = notes.windows(2).all(|w| (w[0].tick, &w[0].instrument, w[0].pitch) <= (w[1].tick, &w[1].instrument, w[1].pitch));
    assert!(sorted);
}

#[test]
fn song_length_covers_everything() {
    let mut p = project();
    let no_audio = |_: &AudioLaneId| 0;
    assert_eq!(song_length(&p, &no_audio), 3840, "empty song is one bar");
    p.song.patterns.push(PatternPlacement { pattern: pid("pattern-1"), at: 3840 });
    assert_eq!(song_length(&p, &no_audio), 3840 * 3);
    p.song.audio_lanes.push(AudioLane { id: AudioLaneId("tag".into()), path: Path::new("/t.wav").into(), volume: 1.0 });
    p.song.audio.push(AudioPlacement { lane: AudioLaneId("tag".into()), at: 3840 * 3 });
    assert_eq!(song_length(&p, &|_| 100), 3840 * 4, "audio past the last pattern extends to the next bar");
}

#[test]
fn audio_only_song_length() {
    let mut p = Project::default();
    p.song.audio_lanes.push(AudioLane { id: AudioLaneId("tag".into()), path: Path::new("/t.wav").into(), volume: 1.0 });
    p.song.audio.push(AudioPlacement { lane: AudioLaneId("tag".into()), at: 0 });
    assert_eq!(song_length(&p, &|_| 5000), 7680);
}
