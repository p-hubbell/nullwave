use nw_core::*;
use nw_workspace::*;

fn pat(s: &str) -> PatternId { PatternId(s.into()) }
fn ins(s: &str) -> InstrumentId { InstrumentId(s.into()) }
fn tp(s: &str) -> TabKind { TabKind::Pattern(pat(s)) }

fn project() -> Project {
    let mut p = Project::default();
    p.instruments.push(Instrument::new(ins("drums"), "Drums"));
    p.patterns.push(Pattern { id: pat("p1"), name: "pattern 1".into(), timeline: Timeline::default() });
    p.patterns.push(Pattern { id: pat("p2"), name: "pattern 2".into(), timeline: Timeline::default() });
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
    assert_eq!(tab_title(&TabKind::Song, &p), "Song");
    assert_eq!(tab_title(&TabKind::Instrument(ins("drums")), &p), "Sampler: Drums");
    assert_eq!(tab_title(&tp("gone"), &p), "gone");
    assert_eq!(tab_title(&TabKind::Instrument(ins("gone")), &p), "gone");
}

#[test]
fn play_target_follows_tab_kind() {
    let mut ws = WorkspaceState::default();
    assert_eq!(ws.play_target(), None);
    ws.open(TabKind::Instrument(ins("drums")));
    assert_eq!(ws.play_target(), None);
    ws.open(tp("p2"));
    assert_eq!(ws.play_target(), Some(PlayTarget::Pattern(pat("p2"))));
    ws.open(TabKind::Song);
    assert_eq!(ws.play_target(), Some(PlayTarget::Song));
}

#[test]
fn retain_valid_drops_deleted_instrument_and_pattern_tabs() {
    let p = project();
    let mut ws = WorkspaceState::default();
    ws.open(tp("p2"));
    ws.open(TabKind::Instrument(ins("gone")));
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
    ws.open(TabKind::Song);
    ws.open(TabKind::Instrument(ins("drums")));
    ws.split_right();
    ws.set_timeline(TimelineTabState {
        tab: tp("p1"),
        open_clip: Some(ClipRef { container: Container::Pattern(pat("p1")), key: ClipKey { instrument: ins("drums"), number: 2 } }),
        roll_height: 300.0,
        px_per_beat: 40.0,
        scroll_ticks: 960.0,
    });
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

#[test]
fn legacy_piano_roll_tabs_become_pattern_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let p = project();
    let mut ws = WorkspaceState::default();
    ws.open(TabKind::PianoRoll(pat("p1"), ins("drums")));
    ws.open(tp("p2"));
    ws.open(TabKind::PianoRoll(pat("p2"), ins("drums")));
    ws.open(tp("p1"));
    ws.open(TabKind::PianoRoll(pat("p2"), ins("gone")));
    ws.sidebar_width = 333.0;
    ws.panel_height = 222.0;
    save_workspace(dir.path(), &ws).unwrap();
    let text = std::fs::read_to_string(dir.path().join(".nullwave/workspace.ron")).unwrap();
    assert!(text.contains("PianoRoll"), "the file holds the old tab kind");
    let loaded = load_workspace(dir.path(), &p);
    assert_eq!(loaded.panes[0].tabs, vec![tp("p1"), tp("p2")]);
    assert_eq!(loaded.active_tab(), Some(&tp("p2")), "the active legacy tab's pattern stays active");
    assert_eq!((loaded.sidebar_width, loaded.panel_height), (333.0, 222.0));
    check_invariants(&loaded);
}

#[test]
fn song_tab_is_always_valid() {
    let mut ws = WorkspaceState::default();
    ws.open(TabKind::Song);
    ws.retain_valid(&Project::default());
    assert_eq!(ws.active_tab(), Some(&TabKind::Song));
}

#[test]
fn timeline_state_defaults_and_updates() {
    let mut ws = WorkspaceState::default();
    let d = ws.timeline(&TabKind::Song);
    assert_eq!(d, TimelineTabState { tab: TabKind::Song, open_clip: None, roll_height: 240.0, px_per_beat: 24.0, scroll_ticks: 0.0 });
    ws.set_timeline(TimelineTabState { roll_height: 100.0, ..d.clone() });
    ws.set_timeline(TimelineTabState { roll_height: 120.0, ..d });
    assert_eq!(ws.timelines.len(), 1, "one state per tab");
    assert_eq!(ws.timeline(&TabKind::Song).roll_height, 120.0);
    assert_eq!(ws.timeline(&tp("p1")).roll_height, 240.0);
}

fn header_project() -> Project {
    let mut p = project();
    p.instruments.push(Instrument::new(ins("bell"), "bell"));
    let key = ClipKey { instrument: ins("bell"), number: 2 };
    let t = &mut p.patterns[0].timeline;
    t.clips.push(Clip::new(key.clone(), 960));
    t.placements = (0..3).map(|n| Placement { clip: key.clone(), at: n * 960 }).collect();
    p.patterns[0].name = "Pattern 1".into();
    p.song.timeline.clips.push(Clip::new(ClipKey { instrument: ins("bell"), number: 3 }, 960));
    p.song.timeline.placements = (0..2)
        .map(|n| Placement { clip: ClipKey { instrument: ins("bell"), number: 3 }, at: n * 960 })
        .collect();
    p
}

#[test]
fn clip_headers_say_what_is_linked() {
    let mut p = header_project();
    let in_pattern = ClipRef { container: Container::Pattern(pat("p1")), key: ClipKey { instrument: ins("bell"), number: 2 } };
    assert_eq!(clip_header(&p, &in_pattern), "Pattern 1 › bell 2 · used 3×");
    p.song.patterns = vec![
        PatternPlacement { pattern: pat("p1"), at: 0 },
        PatternPlacement { pattern: pat("p1"), at: 7680 },
    ];
    assert_eq!(clip_header(&p, &in_pattern), "Pattern 1 › bell 2 · 3× in pattern, pattern placed 2× in song");
    let loose = ClipRef { container: Container::Song, key: ClipKey { instrument: ins("bell"), number: 3 } };
    assert_eq!(clip_header(&p, &loose), "Song › bell 3 · used 2×");
}
