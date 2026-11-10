use nw_core::{ClipKey, ClipRef, Container, DocPath, InstrumentId, PatternId};
use nw_workspace::{DirtyTracker, TabKind};

fn p(s: &str) -> PatternId { PatternId(s.into()) }
fn i(s: &str) -> InstrumentId { InstrumentId(s.into()) }
fn clip(c: Container) -> DocPath { DocPath::Clip(ClipRef { container: c, key: ClipKey { instrument: i("x"), number: 1 } }) }

#[test]
fn pattern_tab_dirty_on_its_layout_or_clips() {
    let mut d = DirtyTracker::default();
    d.mark([clip(Container::Pattern(p("a")))]);
    assert!(d.tab_is_dirty(&TabKind::Pattern(p("a"))));
    assert!(!d.tab_is_dirty(&TabKind::Pattern(p("b"))));
    assert!(!d.tab_is_dirty(&TabKind::Song));
    d.mark([DocPath::PatternLayout(p("b"))]);
    assert!(d.tab_is_dirty(&TabKind::Pattern(p("b"))));
}

#[test]
fn song_tab_dirty_on_arrangement_or_song_clips() {
    let mut d = DirtyTracker::default();
    d.mark([DocPath::Arrangement]);
    assert!(d.tab_is_dirty(&TabKind::Song));
    let mut d = DirtyTracker::default();
    d.mark([clip(Container::Song)]);
    assert!(d.tab_is_dirty(&TabKind::Song));
    assert!(!d.tab_is_dirty(&TabKind::Pattern(p("a"))));
}

#[test]
fn instrument_tab_only_its_own_file() {
    let mut d = DirtyTracker::default();
    d.mark([clip(Container::Pattern(p("a")))]);
    assert!(!d.tab_is_dirty(&TabKind::Instrument(i("x"))));
    d.mark([DocPath::Instrument(i("x"))]);
    assert!(d.tab_is_dirty(&TabKind::Instrument(i("x"))));
}

#[test]
fn song_file_edit_dirties_project_not_tabs() {
    let mut d = DirtyTracker::default();
    assert!(!d.is_dirty());
    d.mark([DocPath::Song]);
    assert!(d.is_dirty());
    assert!(!d.tab_is_dirty(&TabKind::Pattern(p("a"))));
    assert!(!d.tab_is_dirty(&TabKind::Song));
    assert!(!d.tab_is_dirty(&TabKind::Instrument(i("x"))));
}

#[test]
fn clear_resets() {
    let mut d = DirtyTracker::default();
    d.mark([DocPath::Song, DocPath::Arrangement]);
    d.clear();
    assert!(!d.is_dirty());
    assert!(!d.tab_is_dirty(&TabKind::Song));
}
