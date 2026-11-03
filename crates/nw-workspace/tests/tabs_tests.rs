use nw_core::*;
use nw_workspace::*;

fn pat(s: &str) -> PatternId { PatternId(s.into()) }
fn ins(s: &str) -> InstrumentId { InstrumentId(s.into()) }
fn tp(s: &str) -> TabKind { TabKind::Pattern(pat(s)) }

fn project() -> Project {
    let mut p = Project::default();
    p.instruments.push(Instrument::new(ins("drums"), "Drums"));
    p.patterns.push(Pattern { id: pat("p1"), name: "pattern 1".into(), notes: Default::default() });
    p.patterns.push(Pattern { id: pat("p2"), name: "pattern 2".into(), notes: Default::default() });
    p
}

fn check_invariants(ws: &WorkspaceState) {
    assert!(!ws.panes.is_empty());
    assert!(ws.focused_pane < ws.panes.len());
    for p in &ws.panes {
        assert_eq!(p.active.is_none(), p.tabs.is_empty());
        if let Some(a) = p.active { assert!(a < p.tabs.len()); }
    }
}

#[test]
fn opening_open_tab_focuses_it() {
    let mut ws = WorkspaceState::default();
    ws.open(tp("p1")); ws.open(tp("p2")); ws.open(tp("p1"));
    assert_eq!(ws.panes[0].tabs.len(), 2);
    assert_eq!(ws.active_tab(), Some(&tp("p1")));
}

#[test]
fn opening_tab_open_in_other_pane_focuses_that_pane() {
    let mut ws = WorkspaceState::default();
    ws.open(tp("p1")); ws.split_right(); ws.open(tp("p2"));
    assert_eq!(ws.focused_pane, 1);
    ws.open(tp("p1"));
    assert_eq!(ws.focused_pane, 1, "open in both panes: focused pane wins");
    ws.close(1, 0); // pane 1 now only p2
    ws.open(tp("p1"));
    assert_eq!(ws.focused_pane, 0);
    assert_eq!(ws.panes[1].tabs.len(), 1);
    check_invariants(&ws);
}

#[test]
fn open_in_both_panes_prefers_focused_then_other() {
    let mut ws = WorkspaceState::default();
    ws.open(tp("p1")); ws.open(tp("p2")); ws.split_right(); // both panes hold p2
    ws.focused_pane = 0;
    ws.open(tp("p2"));
    assert_eq!(ws.focused_pane, 0);
    assert_eq!(ws.panes[0].active, Some(1));
}

#[test]
fn new_tab_inserted_after_active() {
    let mut ws = WorkspaceState::default();
    ws.open(tp("a")); ws.open(tp("b")); ws.open(tp("c"));
    ws.panes[0].active = Some(0);
    ws.open(tp("d"));
    assert_eq!(ws.panes[0].tabs, vec![tp("a"), tp("d"), tp("b"), tp("c")]);
    assert_eq!(ws.panes[0].active, Some(1));
}

#[test]
fn close_activates_right_then_left() {
    let mut ws = WorkspaceState::default();
    for n in ["a", "b", "c"] { ws.open(tp(n)); }
    ws.panes[0].active = Some(1);
    ws.close(0, 1);
    assert_eq!(ws.active_tab(), Some(&tp("c")));
    ws.close(0, 1);
    assert_eq!(ws.active_tab(), Some(&tp("a")));
    ws.close_active();
    assert_eq!(ws.panes.len(), 1);
    assert_eq!(ws.panes[0].active, None);
    check_invariants(&ws);
}

#[test]
fn close_non_active_tab_keeps_active_tab() {
    let mut ws = WorkspaceState::default();
    for n in ["a", "b", "c"] { ws.open(tp(n)); }
    ws.close(0, 0);
    assert_eq!(ws.active_tab(), Some(&tp("c")));
    ws.close(5, 0); ws.close(0, 9); // out of range: no-op
    assert_eq!(ws.panes[0].tabs.len(), 2);
}

#[test]
fn next_prev_wrap() {
    let mut ws = WorkspaceState::default();
    ws.activate_next(); ws.activate_prev(); // empty: no panic
    for n in ["a", "b", "c"] { ws.open(tp(n)); }
    ws.activate_next();
    assert_eq!(ws.active_tab(), Some(&tp("a")));
    ws.activate_prev();
    assert_eq!(ws.active_tab(), Some(&tp("c")));
}

#[test]
fn split_only_on_request_and_only_once() {
    let mut ws = WorkspaceState::default();
    assert_eq!(ws.panes.len(), 1);
    ws.split_right(); // no active tab
    assert_eq!(ws.panes.len(), 1);
    ws.open(tp("a")); ws.open(tp("b"));
    assert_eq!(ws.panes.len(), 1);
    ws.split_right();
    assert_eq!(ws.panes.len(), 2);
    assert_eq!(ws.panes[1].tabs, vec![tp("b")]);
    assert_eq!(ws.focused_pane, 1);
    ws.split_right();
    assert_eq!(ws.panes.len(), 2);
}

#[test]
fn closing_last_tab_of_second_pane_collapses() {
    let mut ws = WorkspaceState::default();
    ws.open(tp("a")); ws.split_right();
    ws.close_active();
    assert_eq!(ws.panes.len(), 1);
    assert_eq!(ws.focused_pane, 0);
    assert_eq!(ws.active_tab(), Some(&tp("a")));
    check_invariants(&ws);
}

#[test]
fn titles() {
    let p = project();
    assert_eq!(tab_title(&tp("p1"), &p), "pattern 1");
    assert_eq!(tab_title(&TabKind::PianoRoll(pat("p1"), ins("drums")), &p), "pattern 1 / Drums");
    assert_eq!(tab_title(&TabKind::Instrument(ins("drums")), &p), "Sampler: Drums");
    assert_eq!(tab_title(&tp("gone"), &p), "gone");
    assert_eq!(tab_title(&TabKind::Instrument(ins("gone")), &p), "gone");
    assert_eq!(tab_title(&TabKind::PianoRoll(pat("p1"), ins("gone")), &p), "pattern 1 / gone");
}

#[test]
fn active_pattern_follows_tab_kind() {
    let mut ws = WorkspaceState::default();
    assert_eq!(ws.active_pattern(), None);
    ws.open(TabKind::Instrument(ins("drums")));
    assert_eq!(ws.active_pattern(), None);
    ws.open(TabKind::PianoRoll(pat("p1"), ins("drums")));
    assert_eq!(ws.active_pattern(), Some(pat("p1")));
    ws.open(tp("p2"));
    assert_eq!(ws.active_pattern(), Some(pat("p2")));
}

#[test]
fn retain_valid_drops_deleted_instrument_and_pattern_tabs() {
    let p = project();
    let mut ws = WorkspaceState::default();
    ws.open(tp("p2"));
    ws.open(TabKind::Instrument(ins("gone")));
    ws.open(TabKind::PianoRoll(pat("p1"), ins("gone")));
    ws.open(TabKind::PianoRoll(pat("deleted"), ins("drums")));
    ws.retain_valid(&p);
    assert_eq!(ws.panes[0].tabs, vec![tp("p2")]);
    assert_eq!(ws.panes[0].active, Some(0));
    check_invariants(&ws);
}

#[test]
fn retain_valid_keeps_active_tab_when_earlier_ones_dropped() {
    let p = project();
    let mut ws = WorkspaceState::default();
    ws.open(TabKind::Instrument(ins("gone"))); ws.open(tp("p1")); ws.open(tp("p2"));
    ws.panes[0].active = Some(1);
    ws.retain_valid(&p);
    assert_eq!(ws.active_tab(), Some(&tp("p1")));
}

#[test]
fn retain_valid_collapses_emptied_second_pane() {
    let p = project();
    let mut ws = WorkspaceState::default();
    ws.open(tp("p1")); ws.split_right();
    ws.panes[1].tabs = vec![tp("zzz")];
    ws.retain_valid(&p);
    assert_eq!(ws.panes.len(), 1);
    assert_eq!(ws.focused_pane, 0);
    check_invariants(&ws);
}

#[test]
fn retain_valid_empties_only_pane_but_keeps_it() {
    let p = project();
    let mut ws = WorkspaceState::default();
    ws.open(tp("zzz"));
    ws.retain_valid(&p);
    assert_eq!(ws.panes.len(), 1);
    assert_eq!(ws.panes[0].active, None);
}

#[test]
fn load_workspace_skips_stale_tabs_and_survives_corrupt_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = project();
    assert_eq!(load_workspace(dir.path(), &p), WorkspaceState::default());
    let mut ws = WorkspaceState::default();
    ws.open(tp("p1")); ws.open(tp("stale"));
    save_workspace(dir.path(), &ws).unwrap();
    let loaded = load_workspace(dir.path(), &p);
    assert_eq!(loaded.panes[0].tabs, vec![tp("p1")]);
    check_invariants(&loaded);
    std::fs::write(dir.path().join(".nullwave/workspace.ron"), "((( not ron").unwrap();
    assert_eq!(load_workspace(dir.path(), &p), WorkspaceState::default());
}

#[test]
fn load_workspace_normalizes_bad_but_valid_files() {
    let dir = tempfile::tempdir().unwrap();
    let p = project();
    let mut ws = WorkspaceState::default();
    ws.panes.clear();
    save_workspace(dir.path(), &ws).unwrap();
    let l = load_workspace(dir.path(), &p);
    assert_eq!(l.panes.len(), 1);
    assert_eq!(l.focused_pane, 0);
    check_invariants(&l);

    let mut ws = WorkspaceState::default();
    ws.open(tp("p1"));
    ws.focused_pane = 7;
    save_workspace(dir.path(), &ws).unwrap();
    let l = load_workspace(dir.path(), &p);
    assert_eq!(l.focused_pane, 0);
    check_invariants(&l);
}

#[test]
fn workspace_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let p = project();
    let mut ws = WorkspaceState::default();
    ws.open(tp("p1"));
    ws.open(TabKind::PianoRoll(pat("p2"), ins("drums")));
    ws.open(TabKind::Instrument(ins("drums")));
    ws.split_right();
    ws.sidebar_visible = false;
    ws.sidebar_view = SidebarView::Search;
    ws.sidebar_width = 311.5;
    ws.panel_visible = false;
    ws.panel_tab = PanelTab::Log;
    ws.panel_height = 99.0;
    save_workspace(dir.path(), &ws).unwrap();
    assert_eq!(load_workspace(dir.path(), &p), ws);
    let d = WorkspaceState::default();
    assert!(d.sidebar_visible && d.panel_visible);
    assert_eq!((d.sidebar_width, d.panel_height, d.panel_tab), (240.0, 180.0, PanelTab::Editor));
}
