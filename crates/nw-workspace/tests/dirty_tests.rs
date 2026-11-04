use nw_core::{DocPath, InstrumentId, PatternId};
use nw_workspace::{DirtyTracker, TabKind};

fn p(s: &str) -> PatternId { PatternId(s.into()) }
fn i(s: &str) -> InstrumentId { InstrumentId(s.into()) }

#[test]
fn pattern_tab_dirty_when_any_of_its_notes_dirty() {
    let mut d = DirtyTracker::default();
    d.mark([DocPath::Notes(p("a"), i("x"))]);
    assert!(d.tab_is_dirty(&TabKind::Pattern(p("a"))));
    assert!(!d.tab_is_dirty(&TabKind::Pattern(p("b"))));
    d.mark([DocPath::Notes(p("b"), i("y"))]);
    assert!(d.tab_is_dirty(&TabKind::Pattern(p("b"))));
}

#[test]
fn piano_roll_only_its_own_notes() {
    let mut d = DirtyTracker::default();
    d.mark([DocPath::Notes(p("a"), i("x"))]);
    assert!(d.tab_is_dirty(&TabKind::PianoRoll(p("a"), i("x"))));
    assert!(!d.tab_is_dirty(&TabKind::PianoRoll(p("a"), i("y"))));
    assert!(!d.tab_is_dirty(&TabKind::Instrument(i("x"))));
    d.mark([DocPath::Instrument(i("x"))]);
    assert!(d.tab_is_dirty(&TabKind::Instrument(i("x"))));
}

#[test]
fn song_edit_dirties_project_not_tabs() {
    let mut d = DirtyTracker::default();
    assert!(!d.is_dirty());
    d.mark([DocPath::Song]);
    assert!(d.is_dirty());
    assert!(!d.tab_is_dirty(&TabKind::Pattern(p("a"))));
    assert!(!d.tab_is_dirty(&TabKind::PianoRoll(p("a"), i("x"))));
    assert!(!d.tab_is_dirty(&TabKind::Instrument(i("x"))));
}

#[test]
fn clear_resets() {
    let mut d = DirtyTracker::default();
    d.mark([DocPath::Song, DocPath::Notes(p("a"), i("x"))]);
    d.clear();
    assert!(!d.is_dirty());
    assert!(!d.tab_is_dirty(&TabKind::Pattern(p("a"))));
}
