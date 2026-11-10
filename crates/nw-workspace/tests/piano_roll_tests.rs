use nw_core::{Note, DEFAULT_VELOCITY};
use nw_workspace::piano_roll::*;

fn n(pitch: u8, start: u32, length: u32) -> Note {
    Note { pitch, start, length, velocity: 100 }
}

fn view() -> PianoRollView {
    PianoRollView::default()
}

#[test]
fn defaults() {
    let v = view();
    assert_eq!((v.px_per_tick, v.row_height, v.top_pitch, v.scroll_ticks), (0.1, 14.0, 84, 0.0));
}

#[test]
fn tick_pitch_and_rect() {
    let v = PianoRollView { scroll_ticks: 100.0, ..view() };
    assert_eq!(v.tick_at(10.0), 200.0);
    assert_eq!(v.pitch_at(0.0), Some(84));
    assert_eq!(v.pitch_at(13.9), Some(84));
    assert_eq!(v.pitch_at(14.0), Some(83));
    assert_eq!(v.pitch_at(-1.0), None);
    assert_eq!(v.pitch_at(14.0 * 85.0), None);
    assert_eq!(v.note_rect(&n(82, 200, 480)), (10.0, 28.0, 48.0, 14.0));
    assert_eq!(v.note_rect(&n(86, 100, 10)).1, -28.0);
}

#[test]
fn snap_floor_rules() {
    assert_eq!(snap_floor(300.0, 240), 240);
    assert_eq!(snap_floor(-5.0, 240), 0);
    assert_eq!(snap_floor(300.7, 0), 300);
}

#[test]
fn hit_body_edge_empty() {
    let v = view();
    let notes = vec![n(84, 0, 960)]; // rect x 0..96, y 0..14
    assert_eq!(hit_test(&notes, &v, 50.0, 5.0, 240), Hit::Body(0));
    assert_eq!(hit_test(&notes, &v, 91.0, 5.0, 240), Hit::RightEdge(0));
    assert_eq!(hit_test(&notes, &v, 96.0, 5.0, 240), Hit::RightEdge(0));
    assert_eq!(hit_test(&notes, &v, 89.0, 5.0, 240), Hit::Body(0));
    assert!(matches!(hit_test(&notes, &v, 50.0, 20.0, 240), Hit::Empty { pitch: 83, .. }));
    assert!(matches!(hit_test(&notes, &v, 97.0, 5.0, 240), Hit::Empty { .. }));
}

#[test]
fn narrow_note_edge_zone_is_a_third() {
    let v = view();
    let notes = vec![n(84, 0, 90)]; // w = 9, edge zone 3px
    assert_eq!(hit_test(&notes, &v, 5.0, 5.0, 240), Hit::Body(0));
    assert_eq!(hit_test(&notes, &v, 6.5, 5.0, 240), Hit::RightEdge(0));
}

#[test]
fn empty_click_snaps_down() {
    let v = view();
    assert_eq!(hit_test(&[], &v, 30.0, 0.0, 240), Hit::Empty { tick: 240, pitch: 84 });
}

#[test]
fn empty_click_clamps_pitch_and_tick() {
    let v = view();
    assert_eq!(hit_test(&[], &v, -50.0, -30.0, 240), Hit::Empty { tick: 0, pitch: 87 });
    let v = PianoRollView { top_pitch: 127, ..view() };
    assert_eq!(hit_test(&[], &v, 0.0, -30.0, 240), Hit::Empty { tick: 0, pitch: 127 });
    assert_eq!(hit_test(&[], &v, 0.0, 14.0 * 200.0, 240), Hit::Empty { tick: 0, pitch: 0 });
}

#[test]
fn topmost_note_wins() {
    let v = view();
    let notes = vec![n(84, 0, 960), n(84, 0, 960)];
    assert_eq!(hit_test(&notes, &v, 50.0, 5.0, 240), Hit::Body(1));
}

#[test]
fn place_note_defaults() {
    let p = place_note(480, 60, 240);
    assert_eq!(p, Note { pitch: 60, start: 480, length: 240, velocity: DEFAULT_VELOCITY });
}

#[test]
fn group_move_clamps_at_pitch_127_preserving_shape() {
    let notes = vec![n(120, 0, 100), n(126, 0, 100)];
    let out = moved(&notes, &[0, 1], 0, 5);
    assert_eq!((out[0].pitch, out[1].pitch), (121, 127));
    let out = moved(&notes, &[0, 1], 0, -500);
    assert_eq!((out[0].pitch, out[1].pitch), (0, 6));
}

#[test]
fn group_move_clamps_at_tick_zero() {
    let notes = vec![n(60, 100, 50), n(60, 400, 50), n(61, 400, 50)];
    let out = moved(&notes, &[0, 1], -1000, 0);
    assert_eq!((out[0].start, out[1].start), (0, 300));
    assert_eq!(out[2], notes[2]);
    let out = moved(&notes, &[0], 50, 0);
    assert_eq!(out[0].start, 150);
}

#[test]
fn move_ignores_out_of_range_and_huge_deltas() {
    let notes = vec![n(60, 100, 50)];
    assert_eq!(moved(&notes, &[7], 10, 1), notes);
    let out = moved(&notes, &[0], i64::MAX, 0);
    assert_eq!(out[0].start, u32::MAX);
}

#[test]
fn resize_min_is_snap() {
    let notes = vec![n(60, 0, 480), n(61, 0, 480)];
    let out = resized(&notes, &[0], -10_000, 240);
    assert_eq!(out[0].length, 240);
    assert_eq!(out[1].length, 480);
    assert_eq!(resized(&notes, &[0], 240, 240)[0].length, 720);
    assert_eq!(resized(&notes, &[0], -10_000, 0)[0].length, 1);
}

#[test]
fn duplicate_places_after_span() {
    let notes = vec![n(60, 0, 480), n(62, 480, 480), n(64, 2000, 10)];
    let (out, idx) = duplicated(&notes, &[0, 1]);
    assert_eq!(out.len(), 5);
    assert_eq!(idx, vec![3, 4]);
    assert_eq!(out[3], n(60, 960, 480));
    assert_eq!(out[4], n(62, 1440, 480));
    assert_eq!(&out[..3], &notes[..]);
}

#[test]
fn duplicate_empty_selection() {
    let notes = vec![n(60, 0, 480)];
    let (out, idx) = duplicated(&notes, &[]);
    assert_eq!(out, notes);
    assert!(idx.is_empty());
}

#[test]
fn velocity_clamped() {
    let notes = vec![n(60, 0, 480)];
    assert_eq!(with_velocity(&notes, &[0], 0)[0].velocity, 1);
    assert_eq!(with_velocity(&notes, &[0], 200)[0].velocity, 127);
    assert_eq!(with_velocity(&notes, &[0], 64)[0].velocity, 64);
}
