use nw_core::*;
use std::path::Path;

fn iid(s: &str) -> InstrumentId { InstrumentId(s.into()) }
fn pid(s: &str) -> PatternId { PatternId(s.into()) }
fn key(inst: &str, number: ClipNumber) -> ClipKey { ClipKey { instrument: iid(inst), number } }
fn note(pitch: u8, start: Tick) -> Note { Note { pitch, start, length: 240, velocity: 100 } }
fn at(inst: &str, number: ClipNumber, at: Tick) -> Placement { Placement { clip: key(inst, number), at } }
fn hook() -> Container { Container::Pattern(pid("pattern-1")) }

/// bell 1 placed at 0 and 3840 in pattern-1; pattern-1 placed twice in the song.
fn project() -> Project {
    let mut p = Project::default();
    p.instruments.push(Instrument::new(iid("bell"), "Bell"));
    p.instruments.push(Instrument::new(iid("kick"), "Kick"));
    let mut tl = Timeline::default();
    tl.clips.push(Clip { key: key("bell", 1), start: 0, length: 3840, notes: vec![note(72, 0)] });
    tl.placements = vec![at("bell", 1, 0), at("bell", 1, 3840)];
    p.patterns.push(Pattern { id: pid("pattern-1"), name: "Pattern 1".into(), timeline: tl });
    p.song.patterns = vec![
        PatternPlacement { pattern: pid("pattern-1"), at: 0 },
        PatternPlacement { pattern: pid("pattern-1"), at: 7680 },
    ];
    p
}

/// Commits the edits `op` builds from the current project as one step; returns the
/// history for undoing it.
fn commit(p: &mut Project, op: impl FnOnce(&Project) -> Vec<Edit>) -> History {
    let edits = op(p);
    let mut h = History::new();
    h.commit(p, edits);
    h
}

fn tl<'a>(p: &'a Project, c: &Container) -> &'a Timeline {
    p.timeline(c).unwrap()
}

#[test]
fn new_clip_takes_next_number_and_is_placed() {
    let mut p = project();
    let (edits, k) = new_clip(&p, &hook(), &iid("bell"), 960, 3840);
    assert_eq!(k, key("bell", 2));
    commit(&mut p, |_| edits);
    assert_eq!(tl(&p, &hook()).clip(&k).unwrap().length, 3840);
    assert_eq!(tl(&p, &hook()).uses(&k), 1);
    assert!(tl(&p, &hook()).placements.contains(&at("bell", 2, 960)));
}

#[test]
fn move_placements_moves_only_that_block() {
    let mut p = project();
    commit(&mut p, |p| move_placements(p, &hook(), &[(at("bell", 1, 3840), 5760)]));
    assert_eq!(tl(&p, &hook()).placements, vec![at("bell", 1, 0), at("bell", 1, 5760)]);
}

#[test]
fn delete_last_placement_removes_clip() {
    let mut p = project();
    let before = p.clone();
    let mut h = commit(&mut p, |p| delete_placements(p, &hook(), &[at("bell", 1, 0), at("bell", 1, 3840)]));
    assert!(tl(&p, &hook()).clips.is_empty());
    assert!(tl(&p, &hook()).placements.is_empty());
    h.undo(&mut p).unwrap();
    assert_eq!(p, before);
}

#[test]
fn delete_one_of_two_keeps_clip() {
    let mut p = project();
    commit(&mut p, |p| delete_placements(p, &hook(), &[at("bell", 1, 0)]));
    assert_eq!(tl(&p, &hook()).clips.len(), 1);
}

#[test]
fn duplicate_lands_after_selection_as_same_clip() {
    let mut p = project();
    let (edits, copies) = duplicate_placements(&p, &hook(), &[at("bell", 1, 0), at("bell", 1, 3840)]);
    assert_eq!(copies, vec![at("bell", 1, 7680), at("bell", 1, 11520)]);
    commit(&mut p, |_| edits);
    assert_eq!(tl(&p, &hook()).uses(&key("bell", 1)), 4);
    assert_eq!(tl(&p, &hook()).clips.len(), 1);
}

#[test]
fn make_unique_repoints_one_placement() {
    let mut p = project();
    let (edits, k) = make_unique(&p, &hook(), &at("bell", 1, 3840));
    assert_eq!(k, key("bell", 2));
    commit(&mut p, |_| edits);
    let t = tl(&p, &hook());
    assert_eq!(t.placements, vec![at("bell", 1, 0), at("bell", 2, 3840)]);
    assert_eq!(t.clip(&key("bell", 2)).unwrap().notes, t.clip(&key("bell", 1)).unwrap().notes);
}

#[test]
fn make_unique_on_only_block_drops_old_clip() {
    let mut p = project();
    commit(&mut p, |p| delete_placements(p, &hook(), &[at("bell", 1, 0)]));
    commit(&mut p, |p| make_unique(p, &hook(), &at("bell", 1, 3840)).0);
    let keys: Vec<ClipKey> = tl(&p, &hook()).clips.iter().map(|c| c.key.clone()).collect();
    assert_eq!(keys, vec![key("bell", 2)]);
}

#[test]
fn resize_changes_every_block() {
    let mut p = project();
    let r = ClipRef { container: hook(), key: key("bell", 1) };
    commit(&mut p, |p| resize_clip(p, &r, 1920));
    assert_eq!(tl(&p, &hook()).clip(&key("bell", 1)).unwrap().length, 1920);
    assert!(resize_clip(&p, &r, 1920).is_empty(), "no-op resize makes no edits");
}

#[test]
fn trim_moves_every_placement() {
    let mut p = project();
    let r = ClipRef { container: hook(), key: key("bell", 1) };
    commit(&mut p, |p| trim_clip_start(p, &r, 960));
    let t = tl(&p, &hook());
    assert_eq!(t.placements, vec![at("bell", 1, 960), at("bell", 1, 4800)]);
    let c = t.clip(&key("bell", 1)).unwrap();
    assert_eq!((c.start, c.length), (960, 2880));
}

#[test]
fn trim_clamps_at_zero() {
    let p = project();
    let r = ClipRef { container: hook(), key: key("bell", 1) };
    assert!(trim_clip_start(&p, &r, -960).is_empty(), "a block at 0 cannot move left");
    let mut p = project();
    commit(&mut p, |p| move_placements(p, &hook(), &[(at("bell", 1, 0), 1920)]));
    commit(&mut p, |p| trim_clip_start(p, &r, -960));
    let t = tl(&p, &hook());
    assert_eq!(t.placements, vec![at("bell", 1, 960), at("bell", 1, 2880)]);
    let c = t.clip(&key("bell", 1)).unwrap();
    assert_eq!((c.start, c.length, c.notes[0].start), (0, 4800, 960));
}

#[test]
fn trim_left_is_limited_by_the_earliest_block() {
    let mut p = project();
    commit(&mut p, |p| move_placements(p, &hook(), &[(at("bell", 1, 0), 480)]));
    let r = ClipRef { container: hook(), key: key("bell", 1) };
    commit(&mut p, |p| trim_clip_start(p, &r, -960));
    assert_eq!(tl(&p, &hook()).placements, vec![at("bell", 1, 0), at("bell", 1, 3360)]);
}

#[test]
fn edit_clip_notes_replaces_notes() {
    let mut p = project();
    let r = ClipRef { container: hook(), key: key("bell", 1) };
    commit(&mut p, |p| edit_clip_notes(p, &r, vec![note(60, 480), note(64, 0)]));
    let starts: Vec<Tick> = tl(&p, &hook()).clip(&key("bell", 1)).unwrap().notes.iter().map(|n| n.start).collect();
    assert_eq!(starts, vec![0, 480], "notes are kept sorted");
}

#[test]
fn duplicate_pattern_is_independent() {
    let mut p = project();
    let (edits, id) = duplicate_pattern(&p, &pid("pattern-1"));
    assert_eq!(id, pid("pattern-2"));
    commit(&mut p, |_| edits);
    assert_eq!(p.patterns.last().unwrap().name, "Pattern 2");
    assert_eq!(p.patterns.last().unwrap().timeline, p.patterns[0].timeline);
    let copy = ClipRef { container: Container::Pattern(id.clone()), key: key("bell", 1) };
    commit(&mut p, |p| edit_clip_notes(p, &copy, vec![note(40, 0)]));
    assert_eq!(p.patterns[0].timeline.clips[0].notes, vec![note(72, 0)], "the original is untouched");
    assert_eq!(p.song.patterns.len(), 2, "the copy is not placed in the song");
}

#[test]
fn place_move_duplicate_delete_pattern_placements() {
    let mut p = project();
    commit(&mut p, |p| place_pattern(p, &pid("pattern-1"), 3840));
    assert_eq!(p.song.patterns.iter().map(|pp| pp.at).collect::<Vec<_>>(), vec![0, 3840, 7680]);
    let pp = |at| PatternPlacement { pattern: pid("pattern-1"), at };
    commit(&mut p, |p| move_pattern_placements(p, &[(pp(3840), 15360)]));
    let (edits, copies) = duplicate_pattern_placements(&p, &[pp(15360)]);
    assert_eq!(copies, vec![pp(15360 + 7680)], "a 2-bar pattern repeats right after itself");
    commit(&mut p, |_| edits);
    commit(&mut p, |p| delete_pattern_placements(p, &[pp(0)]));
    assert_eq!(p.song.patterns.iter().map(|pp| pp.at).collect::<Vec<_>>(), vec![7680, 15360, 23040]);
}

#[test]
fn delete_pattern_removes_song_placements() {
    let mut p = project();
    let before = p.clone();
    let mut h = commit(&mut p, |p| delete_pattern(p, &pid("pattern-1")));
    assert!(p.patterns.is_empty());
    assert!(p.song.patterns.is_empty());
    h.undo(&mut p).unwrap();
    assert_eq!(p, before);
}

#[test]
fn remove_instrument_restores_everywhere() {
    let mut p = project();
    p.patterns.push(Pattern { id: pid("pattern-2"), name: "Pattern 2".into(), timeline: p.patterns[0].timeline.clone() });
    let (edits, _) = new_clip(&p, &Container::Song, &iid("bell"), 0, 960);
    commit(&mut p, |_| edits);
    let before = p.clone();
    let mut h = commit(&mut p, |p| vec![remove_instrument_edit(p, &iid("bell")).unwrap()]);
    assert!(p.patterns.iter().all(|pt| pt.timeline.clips.is_empty() && pt.timeline.placements.is_empty()));
    assert!(p.song.timeline.clips.is_empty() && p.song.timeline.placements.is_empty());
    h.undo(&mut p).unwrap();
    assert_eq!(p, before);
}

#[test]
fn add_audio_reuses_lane() {
    let mut p = project();
    commit(&mut p, |p| add_audio(p, Path::new("/samples/My Tag.wav"), 0));
    commit(&mut p, |p| add_audio(p, Path::new("/samples/My Tag.wav"), 7680));
    assert_eq!(p.song.audio_lanes.len(), 1);
    let lane = p.song.audio_lanes[0].clone();
    assert_eq!(lane.id, AudioLaneId("my-tag".into()));
    assert_eq!(lane.volume, 0.78);
    assert_eq!(p.song.audio.len(), 2);
    commit(&mut p, |p| set_audio_volume(p, &lane.id, 0.5));
    assert_eq!(p.song.audio_lanes[0].volume, 0.5);
    let first = p.song.audio[0].clone();
    commit(&mut p, |p| move_audio(p, &[(first, 960)]));
    assert_eq!(p.song.audio.iter().map(|a| a.at).collect::<Vec<_>>(), vec![960, 7680]);
    let all = p.song.audio.clone();
    let before_delete = p.clone();
    let mut h = commit(&mut p, |p| delete_audio(p, &all));
    assert!(p.song.audio_lanes.is_empty(), "a lane left with no clips goes too");
    h.undo(&mut p).unwrap();
    assert_eq!(p, before_delete);
}
