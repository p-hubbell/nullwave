use nw_core::*;

fn key(inst: &str, number: ClipNumber) -> ClipKey {
    ClipKey { instrument: InstrumentId(inst.into()), number }
}

fn note(start: Tick, length: Tick) -> Note {
    Note { pitch: 60, start, length, velocity: DEFAULT_VELOCITY }
}

fn clip(start: Tick, length: Tick, notes: Vec<Note>) -> Clip {
    Clip { key: key("bell", 1), start, length, notes }
}

#[test]
fn new_clip_is_empty_from_zero() {
    let c = Clip::new(key("bell", 2), 3840);
    assert_eq!((c.start, c.length, c.notes.len()), (0, 3840, 0));
    assert_eq!(c.key, key("bell", 2));
}

#[test]
fn audible_cuts_notes_at_window_end() {
    let c = clip(0, 960, vec![note(480, 960)]);
    let heard: Vec<Note> = c.audible().collect();
    assert_eq!(heard, vec![note(480, 480)]);
}

#[test]
fn audible_skips_notes_before_window() {
    let c = clip(480, 960, vec![note(0, 240), note(600, 100)]);
    let heard: Vec<Note> = c.audible().collect();
    assert_eq!(heard, vec![note(120, 100)]);
}

#[test]
fn audible_skips_notes_at_or_after_window_end() {
    let c = clip(0, 960, vec![note(960, 10), note(2000, 10)]);
    assert_eq!(c.audible().count(), 0);
}

#[test]
fn trim_start_keeps_notes_in_time() {
    let c = clip(0, 3840, vec![note(960, 100)]);
    let (t, applied) = trim_start(&c, 960);
    assert_eq!(applied, 960);
    assert_eq!((t.start, t.length), (960, 2880));
    assert_eq!(t.notes, vec![note(960, 100)]);
}

#[test]
fn trim_start_past_beginning_rebases() {
    let c = clip(0, 3840, vec![note(0, 100)]);
    let (t, applied) = trim_start(&c, -480);
    assert_eq!(applied, -480);
    assert_eq!((t.start, t.length), (0, 4320));
    assert_eq!(t.notes, vec![note(480, 100)]);
    // The note still sounds 480 ticks after the (new) window start.
    assert_eq!(t.audible().next().unwrap().start, 480);
}

#[test]
fn trim_start_partly_past_beginning() {
    let c = clip(240, 960, vec![note(300, 10)]);
    let (t, applied) = trim_start(&c, -480);
    assert_eq!(applied, -480);
    assert_eq!((t.start, t.length), (0, 1440));
    assert_eq!(t.notes, vec![note(540, 10)]);
}

#[test]
fn trim_start_never_empties() {
    let c = clip(0, 960, vec![]);
    let (t, applied) = trim_start(&c, 5000);
    assert_eq!((t.start, t.length, applied), (959, 1, 959));
    assert_eq!(resize_end(&c, 0).length, 1);
    assert_eq!(resize_end(&c, 7680).length, 7680);
}

#[test]
fn next_number_uses_and_end() {
    let t = Timeline {
        clips: vec![
            Clip::new(key("bell", 1), 960),
            Clip::new(key("bell", 3), 1920),
        ],
        placements: vec![
            Placement { clip: key("bell", 1), at: 0 },
            Placement { clip: key("bell", 1), at: 3840 },
            Placement { clip: key("bell", 3), at: 7680 },
        ],
    };
    assert_eq!(t.uses(&key("bell", 1)), 2);
    assert_eq!(t.uses(&key("bell", 2)), 0);
    assert_eq!(t.next_number(&InstrumentId("bell".into())), 4);
    assert_eq!(t.next_number(&InstrumentId("kick".into())), 1);
    assert_eq!(t.end(), 7680 + 1920);
    assert!(t.clip(&key("bell", 3)).is_some());
    assert_eq!(Timeline::default().end(), 0);
}
