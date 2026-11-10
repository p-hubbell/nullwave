//! Pattern and song tabs: a timeline of instrument lanes (plus audio lanes in the song)
//! with numbered clip blocks, and the piano roll of the open clip underneath.
//!
//! Geometry and hit testing live in `nw_workspace::timeline`; edits come from
//! `nw_core::arrange`. A drag previews locally and commits one undo step on release.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, DragMoveEvent, Entity, FocusHandle, Hitbox, HitboxBehavior,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Rgba, ScrollWheelEvent,
    Window, canvas, div, fill, point, prelude::*, px, rgb, rgba, size,
};
use nw_core::{
    AudioLaneId, AudioPlacement, ClipRef, Container, Edit, PPQ, PatternPlacement, Placement, PlayTarget, Project,
    Tick, TimeSignature, delete_audio, delete_pattern_placements, delete_placements, duplicate_pattern_placements,
    duplicate_placements, make_unique, move_audio, move_pattern_placements, move_placements, new_clip,
    place_pattern, resize_clip, set_audio_volume, trim_clip_start,
};
use nw_workspace::piano_roll::snap_floor;
use nw_workspace::timeline::{
    Block, BlockKind, Edge, Lane, Owner, SnapSize, TimelineHit, TimelineView, blocks, clip_playhead, hit_test,
    lanes, outlines, snap_delta,
};
use nw_workspace::{TabKind, TimelineTabState, clip_header};

use crate::actions::timeline as act;
use crate::app_state::{AppState, SampleStatus};
use crate::theme;
use crate::ui::components::Tooltip;
use crate::ui::drag::{PatternDrag, SampleDrag};
use crate::ui::piano_roll;
use crate::ui::shell::{DragGhost, Shell};

const LANE_HEIGHT: f32 = 44.0;
const HEADER_WIDTH: f32 = 150.0;
const RULER_HEIGHT: f32 = 22.0;
const TOOLBAR_HEIGHT: f32 = 28.0;
const ROLL_HEADER_HEIGHT: f32 = 24.0;
const ROLL_MIN_HEIGHT: f32 = 120.0;
/// The timeline keeps at least this much height when the roll grows.
const TIMELINE_MIN_HEIGHT: f32 = 120.0;
const MIN_PX_PER_BEAT: f32 = 4.0;
const MAX_PX_PER_BEAT: f32 = 400.0;
/// Audio volume steps on the lane header.
const VOLUME_STEP: f32 = 0.05;

/// What can be selected on a timeline. Clip placements are in their own timeline's ticks
/// (pattern-local for a pattern's clip shown in the song).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    Clip { container: Container, placement: Placement },
    Pattern(PatternPlacement),
    Audio(AudioPlacement),
}

pub enum TlDrag {
    /// Moving the selection by `delta` ticks; `copy` (⌥) leaves the originals in place.
    Move { origin: f32, delta: i64, copy: bool, items: Vec<Sel> },
    /// Dragging a clip's right edge: its length changes by `delta`.
    Resize { clip: ClipRef, origin: f32, delta: i64 },
    /// Dragging a clip's left edge: its start moves by `delta`.
    Trim { clip: ClipRef, origin: f32, delta: i64 },
}

/// Per-tab UI state of a timeline tab (not saved).
pub struct TimelineUi {
    pub focus: FocusHandle,
    pub selection: Vec<Sel>,
    pub drag: Option<TlDrag>,
    /// Window position of the canvas the drag started in (a split can show the tab twice).
    pub drag_canvas: Option<Point<f32>>,
    pub snap: SnapSize,
}

/// Whether a mouse event from the canvas at `here` belongs to the drag that started in the
/// canvas at `started`. Every canvas showing a tab receives every mouse move.
pub fn same_canvas(started: Option<Point<f32>>, here: Point<f32>) -> bool {
    started.is_some_and(|s| (s.x - here.x).abs() < 0.5 && (s.y - here.y).abs() < 0.5)
}

fn origin(bounds: Bounds<Pixels>) -> Point<f32> {
    point(f32::from(bounds.origin.x), f32::from(bounds.origin.y))
}

pub type Timelines = HashMap<TabKind, TimelineUi>;

/// Drag payload for the handle between the timeline and its piano roll.
pub struct RollResize(pub TabKind);

pub fn container_of(tab: &TabKind) -> Option<Container> {
    match tab {
        TabKind::Pattern(p) => Some(Container::Pattern(p.clone())),
        TabKind::Song => Some(Container::Song),
        _ => None,
    }
}

fn target_of(tab: &TabKind) -> Option<PlayTarget> {
    match tab {
        TabKind::Pattern(p) => Some(PlayTarget::Pattern(p.clone())),
        TabKind::Song => Some(PlayTarget::Song),
        _ => None,
    }
}

fn view_of(t: &TimelineTabState) -> TimelineView {
    let px_per_beat = if t.px_per_beat.is_finite() { t.px_per_beat.clamp(MIN_PX_PER_BEAT, MAX_PX_PER_BEAT) } else { 24.0 };
    TimelineView {
        px_per_tick: px_per_beat / PPQ as f32,
        scroll: if t.scroll_ticks.is_finite() { t.scroll_ticks.max(0.0) } else { 0.0 },
        lane_height: LANE_HEIGHT,
        header_width: 0.0,
        ruler_height: 0.0,
    }
}

fn sel_of(block: &Block, container: &Container) -> Sel {
    match &block.kind {
        BlockKind::Clip { placement, owner: Owner::Direct } => {
            Sel::Clip { container: container.clone(), placement: placement.clone() }
        }
        BlockKind::Clip { placement, owner: Owner::Pattern(pp) } => {
            Sel::Clip { container: Container::Pattern(pp.pattern.clone()), placement: placement.clone() }
        }
        BlockKind::Audio(a) => Sel::Audio(a.clone()),
    }
}

fn clip_ref(sel: &Sel) -> Option<ClipRef> {
    match sel {
        Sel::Clip { container, placement } => Some(ClipRef { container: container.clone(), key: placement.clip.clone() }),
        _ => None,
    }
}

fn shifted(at: Tick, delta: i64) -> Tick {
    (i64::from(at) + delta).max(0) as Tick
}

/// The edits for moving (or, with `copy`, duplicating) `items` by `delta` ticks, and the
/// selection afterwards. Nothing moves before tick 0.
pub fn move_edits(p: &Project, items: &[Sel], delta: i64, copy: bool) -> (Vec<Edit>, Vec<Sel>) {
    let mut edits = Vec::new();
    let mut after = Vec::new();
    let mut by_container: Vec<(Container, Vec<(Placement, Tick)>)> = Vec::new();
    let mut patterns = Vec::new();
    let mut audio = Vec::new();
    for item in items {
        match item {
            Sel::Clip { container, placement } => {
                let to = shifted(placement.at, delta);
                match by_container.iter_mut().find(|(c, _)| c == container) {
                    Some((_, moves)) => moves.push((placement.clone(), to)),
                    None => by_container.push((container.clone(), vec![(placement.clone(), to)])),
                }
                after.push(Sel::Clip { container: container.clone(), placement: Placement { clip: placement.clip.clone(), at: to } });
            }
            Sel::Pattern(pp) => {
                let to = shifted(pp.at, delta);
                patterns.push((pp.clone(), to));
                after.push(Sel::Pattern(PatternPlacement { pattern: pp.pattern.clone(), at: to }));
            }
            Sel::Audio(a) => {
                let to = shifted(a.at, delta);
                audio.push((a.clone(), to));
                after.push(Sel::Audio(AudioPlacement { lane: a.lane.clone(), at: to }));
            }
        }
    }
    if copy {
        for (container, moves) in by_container {
            let added: Vec<Placement> =
                moves.iter().filter(|(pl, to)| pl.at != *to).map(|(pl, to)| Placement { clip: pl.clip.clone(), at: *to }).collect();
            if !added.is_empty() {
                edits.push(Edit::ReplacePlacements { container, before: Vec::new(), after: added });
            }
        }
        let added: Vec<PatternPlacement> =
            patterns.iter().filter(|(pp, to)| pp.at != *to).map(|(pp, to)| PatternPlacement { pattern: pp.pattern.clone(), at: *to }).collect();
        if !added.is_empty() {
            edits.push(Edit::ReplacePatternPlacements { before: Vec::new(), after: added });
        }
        let added: Vec<AudioPlacement> =
            audio.iter().filter(|(a, to)| a.at != *to).map(|(a, to)| AudioPlacement { lane: a.lane.clone(), at: *to }).collect();
        if !added.is_empty() {
            edits.push(Edit::ReplaceAudioPlacements { before: Vec::new(), after: added });
        }
    } else {
        for (container, moves) in by_container {
            edits.extend(move_placements(p, &container, &moves));
        }
        edits.extend(move_pattern_placements(p, &patterns));
        edits.extend(move_audio(p, &audio));
    }
    (edits, after)
}

/// The edits for deleting `items` (clips left with no block and audio lanes left empty go too).
pub fn delete_edits(p: &Project, items: &[Sel]) -> Vec<Edit> {
    let mut edits = Vec::new();
    let mut by_container: Vec<(Container, Vec<Placement>)> = Vec::new();
    let mut patterns = Vec::new();
    let mut audio = Vec::new();
    for item in items {
        match item {
            Sel::Clip { container, placement } => match by_container.iter_mut().find(|(c, _)| c == container) {
                Some((_, list)) => list.push(placement.clone()),
                None => by_container.push((container.clone(), vec![placement.clone()])),
            },
            Sel::Pattern(pp) => patterns.push(pp.clone()),
            Sel::Audio(a) => audio.push(a.clone()),
        }
    }
    for (container, list) in by_container {
        edits.extend(delete_placements(p, &container, &list));
    }
    edits.extend(delete_pattern_placements(p, &patterns));
    edits.extend(delete_audio(p, &audio));
    edits
}

/// The edits for ⌘D: each kind of selected item repeats right after itself.
fn duplicate_edits(p: &Project, items: &[Sel]) -> (Vec<Edit>, Vec<Sel>) {
    let mut edits = Vec::new();
    let mut after = Vec::new();
    let mut containers: Vec<Container> = Vec::new();
    for item in items {
        if let Sel::Clip { container, .. } = item
            && !containers.contains(container)
        {
            containers.push(container.clone());
        }
    }
    for container in containers {
        let list: Vec<Placement> = items
            .iter()
            .filter_map(|i| match i {
                Sel::Clip { container: c, placement } if *c == container => Some(placement.clone()),
                _ => None,
            })
            .collect();
        let (e, copies) = duplicate_placements(p, &container, &list);
        edits.extend(e);
        after.extend(copies.into_iter().map(|placement| Sel::Clip { container: container.clone(), placement }));
    }
    let patterns: Vec<PatternPlacement> =
        items.iter().filter_map(|i| if let Sel::Pattern(pp) = i { Some(pp.clone()) } else { None }).collect();
    let (e, copies) = duplicate_pattern_placements(p, &patterns);
    edits.extend(e);
    after.extend(copies.into_iter().map(Sel::Pattern));
    (edits, after)
}

impl Shell {
    /// Keeps one `TimelineUi` per open pattern or song tab.
    pub fn sync_timelines(&mut self, cx: &mut Context<Shell>) {
        let open: Vec<TabKind> = self
            .state
            .read(cx)
            .workspace
            .panes
            .iter()
            .flat_map(|p| p.tabs.iter())
            .filter(|t| container_of(t).is_some())
            .cloned()
            .collect();
        self.timelines.retain(|t, _| open.contains(t));
        for tab in open {
            self.timelines.entry(tab).or_insert_with(|| TimelineUi {
                focus: cx.focus_handle(),
                selection: Vec::new(),
                drag: None,
                drag_canvas: None,
                snap: SnapSize::default(),
            });
        }
        // Drop selections that no longer exist.
        let project = &self.state.read(cx).project;
        for ui in self.timelines.values_mut() {
            ui.selection.retain(|s| exists(project, s));
        }
    }

    fn snap_ticks(&self, tab: &TabKind, modifiers: Modifiers, cx: &App) -> Tick {
        if modifiers.platform {
            return 1;
        }
        let ts = self.state.read(cx).project.time_signature;
        self.timelines.get(tab).map_or(SnapSize::Bar, |u| u.snap).ticks(ts)
    }

    fn tl_mouse_down(
        &mut self,
        tab: &TabKind,
        (canvas, pos): (Point<f32>, Point<f32>),
        button: MouseButton,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let Some(container) = container_of(tab) else { return };
        if let Some(ui) = self.timelines.get_mut(tab) {
            window.focus(&ui.focus);
            ui.drag_canvas = Some(canvas);
        }
        let snap = self.snap_ticks(tab, modifiers, cx);
        let state = self.state.read(cx);
        let project = &state.project;
        let view = view_of(&state.workspace.timeline(tab));
        let lane_list = lanes(project, &container);
        let block_list = blocks(project, &container, &|id| state.audio_length(id));
        let outline_list = if container == Container::Song { outlines(project) } else { Vec::new() };
        let hit = hit_test(&view, &lane_list, &block_list, &outline_list, pos.x, pos.y);
        let tick = view.tick_at(pos.x);
        let bar = project.time_signature.ticks_per_bar();

        match hit {
            TimelineHit::Block { index, edge } => {
                let sel = sel_of(&block_list[index], &container);
                if button == MouseButton::Right {
                    let edits = delete_edits(project, std::slice::from_ref(&sel));
                    self.state.update(cx, |s, cx| s.apply(edits, cx));
                    return;
                }
                if button != MouseButton::Left {
                    return;
                }
                let clip = clip_ref(&sel);
                if let Some(r) = &clip {
                    self.state.update(cx, |s, cx| s.open_clip(tab, Some(r.clone()), cx));
                }
                let Some(ui) = self.timelines.get_mut(tab) else { return };
                match (edge, clip) {
                    (Edge::Left, Some(clip)) => {
                        ui.selection = vec![sel];
                        ui.drag = Some(TlDrag::Trim { clip, origin: tick, delta: 0 });
                    }
                    (Edge::Right, Some(clip)) => {
                        ui.selection = vec![sel];
                        ui.drag = Some(TlDrag::Resize { clip, origin: tick, delta: 0 });
                    }
                    _ => {
                        if modifiers.shift {
                            match ui.selection.iter().position(|s| *s == sel) {
                                Some(i) => {
                                    ui.selection.remove(i);
                                }
                                None => ui.selection.push(sel),
                            }
                        } else {
                            if !ui.selection.contains(&sel) {
                                ui.selection = vec![sel];
                            }
                            let items = ui.selection.clone();
                            ui.drag = Some(TlDrag::Move { origin: tick, delta: 0, copy: modifiers.alt, items });
                        }
                    }
                }
            }
            TimelineHit::OutlineLabel(pp) => {
                if button == MouseButton::Right {
                    let edits = delete_pattern_placements(project, std::slice::from_ref(&pp));
                    self.state.update(cx, |s, cx| s.apply(edits, cx));
                    return;
                }
                let Some(ui) = self.timelines.get_mut(tab) else { return };
                let sel = Sel::Pattern(pp);
                if modifiers.shift {
                    match ui.selection.iter().position(|s| *s == sel) {
                        Some(i) => {
                            ui.selection.remove(i);
                        }
                        None => ui.selection.push(sel),
                    }
                } else {
                    if !ui.selection.contains(&sel) {
                        ui.selection = vec![sel];
                    }
                    let items = ui.selection.clone();
                    ui.drag = Some(TlDrag::Move { origin: tick, delta: 0, copy: modifiers.alt, items });
                }
            }
            TimelineHit::Empty { lane, tick: at, inside } if button == MouseButton::Left => {
                let Some(Lane::Instrument(instrument)) = lane_list.get(lane).cloned() else {
                    if let Some(ui) = self.timelines.get_mut(tab) {
                        ui.selection.clear();
                    }
                    cx.notify();
                    return;
                };
                // Inside a placed pattern's outline the new clip goes into that pattern.
                let (target, local) = match &inside {
                    Some(pp) => (Container::Pattern(pp.pattern.clone()), at - pp.at),
                    None => (container.clone(), at),
                };
                let start = snap_floor(local as f32, snap);
                let (edits, key) = new_clip(project, &target, &instrument, start, bar);
                let r = ClipRef { container: target.clone(), key: key.clone() };
                self.state.update(cx, |s, cx| {
                    s.apply(edits, cx);
                    s.open_clip(tab, Some(r), cx);
                });
                if let Some(ui) = self.timelines.get_mut(tab) {
                    ui.selection = vec![Sel::Clip { container: target, placement: Placement { clip: key, at: start } }];
                }
            }
            _ => {
                if let Some(ui) = self.timelines.get_mut(tab) {
                    ui.selection.clear();
                }
            }
        }
        cx.notify();
    }

    fn tl_mouse_move(&mut self, tab: &TabKind, canvas: Point<f32>, pos: Point<f32>, modifiers: Modifiers, cx: &mut Context<Shell>) {
        let snap = self.snap_ticks(tab, modifiers, cx);
        let view = view_of(&self.state.read(cx).workspace.timeline(tab));
        let Some(ui) = self.timelines.get_mut(tab) else { return };
        if !same_canvas(ui.drag_canvas, canvas) {
            return;
        }
        let tick = view.tick_at(pos.x);
        let changed = match &mut ui.drag {
            Some(TlDrag::Move { origin, delta, .. })
            | Some(TlDrag::Resize { origin, delta, .. })
            | Some(TlDrag::Trim { origin, delta, .. }) => {
                let next = snap_delta(tick - *origin, snap);
                let changed = next != *delta;
                *delta = next;
                changed
            }
            None => false,
        };
        if changed {
            cx.notify();
        }
    }

    fn tl_mouse_up(&mut self, tab: &TabKind, cx: &mut Context<Shell>) {
        let Some(ui) = self.timelines.get_mut(tab) else { return };
        let Some(drag) = ui.drag.take() else { return };
        let project = self.state.read(cx).project.clone();
        match drag {
            TlDrag::Move { delta, copy, items, .. } => {
                if delta == 0 {
                    cx.notify();
                    return;
                }
                let (edits, after) = move_edits(&project, &items, delta, copy);
                self.state.update(cx, |s, cx| s.apply(edits, cx));
                if let Some(ui) = self.timelines.get_mut(tab) {
                    ui.selection = after;
                }
            }
            TlDrag::Resize { clip, delta, .. } => {
                let length = project.timeline(&clip.container).and_then(|t| t.clip(&clip.key)).map_or(1, |c| c.length);
                let edits = resize_clip(&project, &clip, (i64::from(length) + delta).max(1) as Tick);
                self.state.update(cx, |s, cx| s.apply(edits, cx));
            }
            TlDrag::Trim { clip, delta, .. } => {
                let edits = trim_clip_start(&project, &clip, delta);
                self.state.update(cx, |s, cx| s.apply(edits, cx));
                // The blocks moved; select the clip's blocks afresh.
                if let Some(ui) = self.timelines.get_mut(tab) {
                    ui.selection.clear();
                }
            }
        }
        cx.notify();
    }

    /// ⌘+wheel zooms around the cursor; sideways (or ⇧) scrolls time; plain vertical
    /// wheel is left to the lane list. Returns whether the event was used.
    fn tl_scroll(&mut self, tab: &TabKind, pos: Point<f32>, delta: Point<f32>, modifiers: Modifiers, cx: &mut Context<Shell>) -> bool {
        let mut t = self.state.read(cx).workspace.timeline(tab);
        let view = view_of(&t);
        if modifiers.platform {
            let anchor = view.tick_at(pos.x);
            let factor = (delta.y * 0.01).exp();
            t.px_per_beat = (t.px_per_beat * factor).clamp(MIN_PX_PER_BEAT, MAX_PX_PER_BEAT);
            let ppt = t.px_per_beat / PPQ as f32;
            t.scroll_ticks = (anchor - pos.x / ppt).max(0.0);
        } else if modifiers.shift || delta.x.abs() > delta.y.abs() {
            let d = if modifiers.shift && delta.x == 0.0 { delta.y } else { delta.x };
            t.scroll_ticks = (t.scroll_ticks - d / view.px_per_tick).max(0.0);
        } else {
            return false;
        }
        self.state.update(cx, |s, cx| s.update_workspace(cx, |ws| ws.set_timeline(t)));
        true
    }

    fn tl_ruler_click(&mut self, tab: &TabKind, x: f32, modifiers: Modifiers, cx: &mut Context<Shell>) {
        if *tab != TabKind::Song {
            return;
        }
        let snap = self.snap_ticks(tab, modifiers, cx);
        let view = view_of(&self.state.read(cx).workspace.timeline(tab));
        let tick = snap_floor(view.tick_at(x).max(0.0), snap);
        self.state.update(cx, |s, cx| s.set_song_playhead(tick, cx));
    }

    /// The timeline tab whose keyboard focus is active.
    fn focused_timeline(&self, window: &Window, cx: &App) -> Option<TabKind> {
        self.timelines.iter().find(|(_, u)| u.focus.contains_focused(window, cx)).map(|(t, _)| t.clone())
    }

    fn active_timeline(&self, window: &Window, cx: &App) -> Option<TabKind> {
        self.focused_timeline(window, cx)
            .or_else(|| self.state.read(cx).workspace.active_tab().filter(|t| container_of(t).is_some()).cloned())
    }

    fn tl_duplicate(&mut self, _: &act::Duplicate, window: &mut Window, cx: &mut Context<Shell>) {
        let Some(tab) = self.active_timeline(window, cx) else { return };
        let Some(items) = self.timelines.get(&tab).map(|u| u.selection.clone()) else { return };
        let (edits, after) = duplicate_edits(&self.state.read(cx).project, &items);
        self.state.update(cx, |s, cx| s.apply(edits, cx));
        if let Some(ui) = self.timelines.get_mut(&tab) {
            ui.selection = after;
        }
        cx.notify();
    }

    fn tl_delete(&mut self, _: &act::Delete, window: &mut Window, cx: &mut Context<Shell>) {
        let Some(tab) = self.active_timeline(window, cx) else { return };
        let Some(items) = self.timelines.get(&tab).map(|u| u.selection.clone()) else { return };
        let edits = delete_edits(&self.state.read(cx).project, &items);
        self.state.update(cx, |s, cx| s.apply(edits, cx));
        if let Some(ui) = self.timelines.get_mut(&tab) {
            ui.selection.clear();
        }
        cx.notify();
    }

    fn tl_make_unique(&mut self, _: &act::MakeUnique, window: &mut Window, cx: &mut Context<Shell>) {
        let Some(tab) = self.active_timeline(window, cx) else { return };
        let selected = self.timelines.get(&tab).map(|u| u.selection.clone()).unwrap_or_default();
        let [Sel::Clip { container, placement }] = selected.as_slice() else {
            self.state.update(cx, |s, cx| {
                s.log("Make Unique: select one clip block first");
                cx.notify();
            });
            return;
        };
        let (edits, key) = make_unique(&self.state.read(cx).project, container, placement);
        let r = ClipRef { container: container.clone(), key: key.clone() };
        self.state.update(cx, |s, cx| {
            s.apply(edits, cx);
            s.open_clip(&tab, Some(r), cx);
        });
        if let Some(ui) = self.timelines.get_mut(&tab) {
            ui.selection = vec![Sel::Clip { container: container.clone(), placement: Placement { clip: key, at: placement.at } }];
        }
        cx.notify();
    }

    fn tl_close_roll(&mut self, _: &act::CloseRoll, window: &mut Window, cx: &mut Context<Shell>) {
        let Some(tab) = self.active_timeline(window, cx) else { return };
        self.state.update(cx, |s, cx| s.open_clip(&tab, None, cx));
        if let Some(ui) = self.timelines.get(&tab) {
            window.focus(&ui.focus);
        }
    }

    fn resize_roll(&mut self, e: &DragMoveEvent<RollResize>, _: &mut Window, cx: &mut Context<Shell>) {
        let tab = e.drag(cx).0.clone();
        let total = f32::from(e.bounds.size.height);
        let bottom = f32::from(e.bounds.origin.y + e.bounds.size.height);
        let wanted = bottom - f32::from(e.event.position.y);
        let max = (total - TIMELINE_MIN_HEIGHT).max(ROLL_MIN_HEIGHT);
        let height = wanted.clamp(ROLL_MIN_HEIGHT, max);
        let mut t = self.state.read(cx).workspace.timeline(&tab);
        if (t.roll_height - height).abs() >= 1.0 {
            t.roll_height = height;
            self.state.update(cx, |s, cx| s.update_workspace(cx, |ws| ws.set_timeline(t)));
        }
    }
}

fn exists(p: &Project, sel: &Sel) -> bool {
    match sel {
        Sel::Clip { container, placement } => p.timeline(container).is_some_and(|t| t.placements.contains(placement)),
        Sel::Pattern(pp) => p.song.patterns.contains(pp),
        Sel::Audio(a) => p.song.audio.contains(a),
    }
}

// Colors local to the timeline.
fn lane_bg(i: usize) -> Rgba {
    if i.is_multiple_of(2) { rgb(0x1f2126) } else { rgb(0x1c1e22) }
}
fn bar_line() -> Rgba {
    rgba(0xffffff26)
}
fn beat_line() -> Rgba {
    rgba(0xffffff0c)
}
fn clip_fill() -> Rgba {
    rgb(0x3b4a63)
}
fn clip_border() -> Rgba {
    rgb(0x5b7299)
}
fn pattern_clip_fill() -> Rgba {
    rgb(0x323c50)
}
fn audio_fill() -> Rgba {
    rgb(0x2f5a48)
}
fn audio_border() -> Rgba {
    rgb(0x57a585)
}
fn selected_fill() -> Rgba {
    rgb(0x56739f)
}
fn linked_border() -> Rgba {
    rgb(0xffd166)
}
fn note_mark() -> Rgba {
    rgba(0xdfe6f3cc)
}
fn outline_border() -> Rgba {
    rgba(0x5b86d6c0)
}
fn outline_fill() -> Rgba {
    rgba(0x5b86d612)
}
fn outline_tab() -> Rgba {
    rgb(0x2f4f8f)
}
fn outline_tab_selected() -> Rgba {
    rgb(0x4a72c0)
}
fn playhead_color() -> Rgba {
    rgb(0xffd166)
}
fn song_cursor_color() -> Rgba {
    rgba(0xffd16680)
}

struct PaintBlock {
    lane: usize,
    start: f32,
    end: f32,
    fill: Rgba,
    border: Rgba,
    /// Notes as (start, length, height) fractions of the block.
    notes: Vec<(f32, f32, f32)>,
}

struct LanesFrame {
    view: TimelineView,
    lane_count: usize,
    instrument_lanes: usize,
    blocks: Vec<PaintBlock>,
    /// Outline spans (ticks) and whether selected.
    outlines: Vec<(f32, f32, bool)>,
    ts: TimeSignature,
    playhead: Option<Tick>,
    cursor: Option<Tick>,
}

fn rect(bounds: Bounds<Pixels>, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w.max(0.0)), px(h.max(0.0))))
}

fn local(bounds: Bounds<Pixels>, p: Point<Pixels>) -> Point<f32> {
    point(f32::from(p.x - bounds.origin.x), f32::from(p.y - bounds.origin.y))
}

/// Where each block is drawn, with the live drag applied.
fn preview_span(block: &Block, sel: &Sel, drag: Option<&TlDrag>, project: &Project) -> (f32, f32) {
    let (mut start, mut end) = (block.start as f32, block.end as f32);
    match drag {
        Some(TlDrag::Move { delta, items, copy: false, .. }) => {
            let moved = items.iter().any(|i| i == sel || matches!((i, &block.kind), (Sel::Pattern(pp), BlockKind::Clip { owner: Owner::Pattern(o), .. }) if pp == o));
            if moved {
                let own_at = match sel {
                    Sel::Clip { placement, .. } => placement.at,
                    Sel::Audio(a) => a.at,
                    Sel::Pattern(pp) => pp.at,
                };
                // Pattern-owned clips move with their pattern; otherwise clamp at 0.
                let by = match (&block.kind, items.iter().find(|i| *i == sel)) {
                    (_, Some(_)) => (shifted(own_at, *delta) as i64 - i64::from(own_at)) as f32,
                    (BlockKind::Clip { owner: Owner::Pattern(o), .. }, None) => {
                        (shifted(o.at, *delta) as i64 - i64::from(o.at)) as f32
                    }
                    _ => 0.0,
                };
                start += by;
                end += by;
            }
        }
        Some(TlDrag::Resize { clip, delta, .. }) if clip_ref(sel).as_ref() == Some(clip) => {
            end = (end + *delta as f32).max(start + 1.0);
        }
        Some(TlDrag::Trim { clip, delta, .. }) if clip_ref(sel).as_ref() == Some(clip) => {
            let length = project.timeline(&clip.container).and_then(|t| t.clip(&clip.key)).map_or(1, |c| c.length);
            let d = (*delta).min(i64::from(length) - 1) as f32;
            start = (start + d).max(0.0).min(end - 1.0);
        }
        _ => {}
    }
    (start, end)
}

/// Renders a pattern or song tab.
pub fn render(shell: &Shell, state: &AppState, tab: &TabKind, cx: &Context<Shell>) -> AnyElement {
    let (Some(container), Some(ui)) = (container_of(tab), shell.timelines.get(tab)) else {
        return div().into_any_element();
    };
    let project = &state.project;
    let tstate = state.workspace.timeline(tab);
    let view = view_of(&tstate);
    let lane_list = lanes(project, &container);
    let block_list = blocks(project, &container, &|id| state.audio_length(id));
    let outline_list = if container == Container::Song { outlines(project) } else { Vec::new() };
    let open_clip = tstate.open_clip.clone().filter(|r| shell.rolls.contains_key(r));
    let playing_here = state.playing && state.last_played.as_ref() == target_of(tab).as_ref();

    // Blocks to paint, with the drag preview applied.
    let paint_blocks: Vec<PaintBlock> = block_list
        .iter()
        .map(|b| {
            let sel = sel_of(b, &container);
            let (start, end) = preview_span(b, &sel, ui.drag.as_ref(), project);
            let selected = ui.selection.contains(&sel)
                || matches!(&b.kind, BlockKind::Clip { owner: Owner::Pattern(o), .. } if ui.selection.contains(&Sel::Pattern(o.clone())));
            let linked = open_clip.is_some() && clip_ref(&sel) == open_clip;
            let (base, border) = match &b.kind {
                BlockKind::Audio(_) => (audio_fill(), audio_border()),
                BlockKind::Clip { owner: Owner::Pattern(_), .. } => (pattern_clip_fill(), clip_border()),
                BlockKind::Clip { .. } => (clip_fill(), clip_border()),
            };
            let notes = match clip_ref(&sel) {
                Some(r) => project.timeline(&r.container).and_then(|t| t.clip(&r.key)).map_or(Vec::new(), |c| {
                    let heard: Vec<_> = c.audible().collect();
                    let lo = heard.iter().map(|n| n.pitch).min().unwrap_or(0);
                    let hi = heard.iter().map(|n| n.pitch).max().unwrap_or(0);
                    let span = f32::from(hi - lo).max(1.0);
                    let len = c.length.max(1) as f32;
                    heard
                        .iter()
                        .map(|n| (n.start as f32 / len, n.length as f32 / len, f32::from(n.pitch - lo) / span))
                        .collect()
                }),
                None => Vec::new(),
            };
            PaintBlock {
                lane: b.lane,
                start,
                end,
                fill: if selected { selected_fill() } else { base },
                border: if linked { linked_border() } else { border },
                notes,
            }
        })
        .collect();
    let paint_outlines: Vec<(f32, f32, bool)> = outline_list
        .iter()
        .map(|o| {
            let sel = Sel::Pattern(o.placement.clone());
            let by = match &ui.drag {
                Some(TlDrag::Move { delta, items, copy: false, .. }) if items.contains(&sel) => {
                    (shifted(o.start, *delta) as i64 - i64::from(o.start)) as f32
                }
                _ => 0.0,
            };
            (o.start as f32 + by, o.end as f32 + by, ui.selection.contains(&sel))
        })
        .collect();
    let frame = LanesFrame {
        view,
        lane_count: lane_list.len(),
        instrument_lanes: project.instruments.len(),
        blocks: paint_blocks,
        outlines: paint_outlines.clone(),
        ts: project.time_signature,
        playhead: playing_here.then_some(state.playhead),
        cursor: (container == Container::Song && !playing_here).then_some(state.song_playhead),
    };

    // Text over the canvas: block labels and outline name tabs.
    let mut labels: Vec<AnyElement> = Vec::new();
    for (b, pb) in block_list.iter().zip(&frame.blocks) {
        let x = view.x_of(0) + pb.start * view.px_per_tick;
        let w = (pb.end - pb.start) * view.px_per_tick;
        if w < 10.0 {
            continue;
        }
        labels.push(
            div()
                .absolute()
                .left(px(x + 4.0))
                .top(px(pb.lane as f32 * LANE_HEIGHT + LANE_HEIGHT - 17.0))
                .max_w(px((w - 6.0).max(0.0)))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(theme::SMALL_FONT_SIZE))
                .text_color(theme::text_strong())
                .child(b.label.clone())
                .into_any_element(),
        );
    }
    for (o, (start, end, selected)) in outline_list.iter().zip(&paint_outlines) {
        let x = view.x_of(0) + start * view.px_per_tick;
        let w = ((end - start) * view.px_per_tick).min(nw_workspace::timeline::LABEL_MAX_WIDTH);
        labels.push(
            div()
                .absolute()
                .left(px(x))
                .top_0()
                .w(px(w.max(0.0)))
                .h(px(nw_workspace::timeline::LABEL_HEIGHT))
                .px(px(5.))
                .overflow_hidden()
                .whitespace_nowrap()
                .rounded_br(px(3.))
                .bg(if *selected { outline_tab_selected() } else { outline_tab() })
                .text_size(px(theme::SMALL_FONT_SIZE))
                .text_color(theme::text_strong())
                .child(o.name.clone())
                .into_any_element(),
        );
    }

    let shell_entity = cx.entity();
    let lanes_height = lane_list.len() as f32 * LANE_HEIGHT;
    let drop_bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));
    let lanes_canvas = lanes_canvas(shell_entity.clone(), tab.clone(), frame, drop_bounds.clone());

    let is_song = container == Container::Song;
    let (sample_bounds, pattern_bounds) = (drop_bounds.clone(), drop_bounds);
    let (sample_state, pattern_state) = (shell.state.clone(), shell.state.clone());
    let lane_area = div()
        .relative()
        .flex_1()
        .min_w_0()
        .h(px(lanes_height))
        .overflow_hidden()
        .child(lanes_canvas.absolute().inset_0())
        .children(labels)
        .when(is_song, |d| {
            d.drag_over::<SampleDrag>(|s, _, _, _| s.bg(rgba(0x6aa0ff14)))
                .drag_over::<PatternDrag>(|s, _, _, _| s.bg(rgba(0x6aa0ff14)))
                .on_drop(move |drag: &SampleDrag, window, cx| {
                    let Some(bounds) = sample_bounds.get() else { return };
                    let x = local(bounds, window.mouse_position()).x;
                    let path = drag.path.clone();
                    sample_state.update(cx, |s, cx| {
                        let v = view_of(&s.workspace.timeline(&TabKind::Song));
                        let snap = SnapSize::Bar.ticks(s.project.time_signature);
                        let at = snap_floor(v.tick_at(x).max(0.0), snap);
                        s.add_audio_clip(path, at, cx);
                    });
                })
                .on_drop(move |drag: &PatternDrag, window, cx| {
                    let Some(bounds) = pattern_bounds.get() else { return };
                    let x = local(bounds, window.mouse_position()).x;
                    let id = drag.id.clone();
                    pattern_state.update(cx, |s, cx| {
                        let v = view_of(&s.workspace.timeline(&TabKind::Song));
                        let snap = SnapSize::Bar.ticks(s.project.time_signature);
                        let at = snap_floor(v.tick_at(x).max(0.0), snap);
                        let edits = place_pattern(&s.project, &id, at);
                        s.apply(edits, cx);
                    });
                })
        });

    let headers = lane_list.iter().enumerate().map(|(i, lane)| lane_header(shell, state, i, lane));
    let lanes_list = div()
        .id(("timeline-lanes", tab_index(tab)))
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_row()
                .child(div().w(px(HEADER_WIDTH)).flex_none().flex().flex_col().children(headers))
                .child(lane_area),
        );

    let timeline = div()
        .flex_1()
        .min_h(px(TIMELINE_MIN_HEIGHT))
        .flex()
        .flex_col()
        .child(toolbar(shell, tab, ui.snap, cx))
        .child(ruler(shell_entity, tab.clone(), view, project.time_signature, state, is_song))
        .child(lanes_list);

    let roll = open_clip.map(|r| {
        let header = clip_header(project, &r);
        let playhead = state
            .last_played
            .as_ref()
            .filter(|_| state.playing)
            .and_then(|t| clip_playhead(project, t, &r, state.playhead));
        let height = if tstate.roll_height.is_finite() { tstate.roll_height.max(ROLL_MIN_HEIGHT) } else { 240.0 };
        let close_tab = tab.clone();
        let close_state = shell.state.clone();
        div()
            .h(px(height))
            .flex_none()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(("roll-resize", tab_index(tab)))
                    .h(px(theme::RESIZE_HANDLE))
                    .w_full()
                    .flex_none()
                    .bg(theme::sidebar_bg())
                    .border_t_1()
                    .border_color(theme::border())
                    .cursor_row_resize()
                    .on_drag(RollResize(tab.clone()), |_, _, _, cx| cx.new(|_| DragGhost)),
            )
            .child(
                div()
                    .h(px(ROLL_HEADER_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .gap(px(8.))
                    .bg(theme::chrome_bg())
                    .border_b_1()
                    .border_color(theme::border())
                    .text_size(px(theme::SMALL_FONT_SIZE))
                    .text_color(theme::text())
                    .child(div().flex_1().whitespace_nowrap().overflow_hidden().child(header))
                    .child(
                        div()
                            .id(("roll-close", tab_index(tab)))
                            .px(px(5.))
                            .rounded(px(3.))
                            .cursor_pointer()
                            .text_color(theme::text_faint())
                            .hover(|s| s.bg(theme::hover_bg()).text_color(theme::text_strong()))
                            .on_click(move |_, _, cx| {
                                close_state.update(cx, |s, cx| s.open_clip(&close_tab, None, cx));
                            })
                            .child("×"),
                    ),
            )
            .child(div().flex_1().min_h_0().child(piano_roll::render(shell, state, &r, playhead, cx)))
    });

    div()
        .key_context("Timeline")
        .track_focus(&ui.focus)
        .on_action(cx.listener(Shell::tl_duplicate))
        .on_action(cx.listener(Shell::tl_delete))
        .on_action(cx.listener(Shell::tl_make_unique))
        .on_action(cx.listener(Shell::tl_close_roll))
        .on_drag_move(cx.listener(Shell::resize_roll))
        .size_full()
        .flex()
        .flex_col()
        .child(timeline)
        .children(roll)
        .into_any_element()
}

/// A stable small number per tab for element ids.
fn tab_index(tab: &TabKind) -> usize {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tab.hash(&mut h);
    h.finish() as usize
}

fn toolbar(shell: &Shell, tab: &TabKind, snap: SnapSize, cx: &Context<Shell>) -> impl IntoElement {
    let snap_tab = tab.clone();
    let hint = match tab {
        TabKind::Song => "Drag patterns or samples here · click a lane to add a clip · ⌘-drag ignores snap",
        _ => "Click a lane to add a clip · ⌥-drag repeats · edges trim and resize · ⌘-drag ignores snap",
    };
    let _ = shell;
    div()
        .h(px(TOOLBAR_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(10.))
        .bg(theme::chrome_bg())
        .border_b_1()
        .border_color(theme::border())
        .text_size(px(theme::SMALL_FONT_SIZE))
        .child(
            div()
                .id(("snap", tab_index(tab)))
                .px(px(6.))
                .py(px(2.))
                .rounded(px(3.))
                .border_1()
                .border_color(theme::border())
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover_bg()))
                .on_click(cx.listener(move |shell, _, _, cx| {
                    if let Some(ui) = shell.timelines.get_mut(&snap_tab) {
                        ui.snap = ui.snap.next();
                        cx.notify();
                    }
                }))
                .child(format!("Snap: {}", snap.label())),
        )
        .child(div().text_color(theme::text_faint()).whitespace_nowrap().overflow_hidden().child(hint))
}

fn ruler(
    shell: Entity<Shell>,
    tab: TabKind,
    view: TimelineView,
    ts: TimeSignature,
    state: &AppState,
    is_song: bool,
) -> impl IntoElement {
    let bar = ts.ticks_per_bar().max(1);
    let px_per_bar = bar as f32 * view.px_per_tick;
    let every = (40.0 / px_per_bar).ceil().max(1.0) as u32;
    let first = (view.scroll / bar as f32).floor() as u32;
    let numbers = (first..first + 400).filter(|b| b % every == 0).map(|b| {
        let x = view.x_of(b * bar);
        div()
            .absolute()
            .left(px(x + 3.0))
            .top(px(3.))
            .text_size(px(theme::SMALL_FONT_SIZE))
            .text_color(theme::text_faint())
            .child(format!("{}", b + 1))
    });
    let cursor = is_song.then_some(state.song_playhead);
    let marks = canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let last = view.tick_at(width).max(0.0) as Tick;
            let mut b = (view.scroll.max(0.0) as Tick / bar) * bar;
            while b <= last {
                let x = view.x_of(b);
                window.paint_quad(fill(rect(bounds, x, height - 8.0, 1.0, 8.0), bar_line()));
                b += bar;
            }
            if let Some(c) = cursor {
                let x = view.x_of(c);
                window.paint_quad(fill(rect(bounds, x - 4.0, height - 6.0, 9.0, 6.0), playhead_color()));
            }
            let tab = tab.clone();
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !hitbox.is_hovered(window) {
                    return;
                }
                let x = local(bounds, e.position).x;
                shell.update(cx, |sh, cx| sh.tl_ruler_click(&tab, x, e.modifiers, cx));
                cx.stop_propagation();
            });
        },
    );
    div()
        .h(px(RULER_HEIGHT))
        .flex_none()
        .flex()
        .flex_row()
        .bg(theme::chrome_bg())
        .border_b_1()
        .border_color(theme::border())
        .child(div().w(px(HEADER_WIDTH)).flex_none())
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_hidden()
                .when(is_song, |d| d.cursor_pointer())
                .child(marks.absolute().inset_0())
                .children(numbers),
        )
}

fn lane_header(shell: &Shell, state: &AppState, index: usize, lane: &Lane) -> AnyElement {
    let base = div()
        .id(("lane-header", index))
        .h(px(LANE_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .border_b_1()
        .border_r_1()
        .border_color(theme::border())
        .bg(theme::sidebar_bg())
        .whitespace_nowrap()
        .overflow_hidden();
    match lane {
        Lane::Instrument(id) => {
            let Some(inst) = state.project.instrument(id) else { return base.into_any_element() };
            let open_state = shell.state.clone();
            let drop_state = shell.state.clone();
            let open_tab = TabKind::Instrument(id.clone());
            let drop_id = id.clone();
            let missing = match state.sample_status(id) {
                SampleStatus::Missing(e) => Some(e),
                _ => None,
            };
            base.cursor_pointer()
                .hover(|s| s.bg(theme::hover_bg()))
                .drag_over::<SampleDrag>(|s, _, _, _| s.bg(theme::selection_bg()))
                .on_drop(move |drag: &SampleDrag, _, cx| {
                    drop_state.update(cx, |s, cx| s.set_instrument_sample(&drop_id, drag.path.clone(), cx));
                })
                .on_click(move |_, _, cx| open_state.update(cx, |s, cx| s.open_tab(open_tab.clone(), cx)))
                .child(div().flex_1().overflow_hidden().text_color(theme::text_strong()).child(inst.name.clone()))
                .when_some(missing, |d, error| {
                    d.child(
                        div()
                            .id(("lane-missing", index))
                            .flex_none()
                            .text_color(theme::warning())
                            .text_size(px(theme::SMALL_FONT_SIZE))
                            .child("missing")
                            .tooltip(Tooltip::text(error)),
                    )
                })
                .into_any_element()
        }
        Lane::Audio(id) => {
            let Some(lane) = state.project.song.audio_lanes.iter().find(|l| &l.id == id) else {
                return base.into_any_element();
            };
            let name = lane.path.file_stem().map_or_else(|| id.0.clone(), |s| s.to_string_lossy().into_owned());
            let missing = state.audio_missing(id);
            let step = |delta: f32, label: &'static str, suffix: &'static str| {
                let st = shell.state.clone();
                let lane_id: AudioLaneId = id.clone();
                let volume = lane.volume;
                div()
                    .id(SharedId::from(format!("vol-{suffix}-{}", id.0)))
                    .px(px(4.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .text_color(theme::text_muted())
                    .hover(|s| s.bg(theme::hover_bg()).text_color(theme::text_strong()))
                    .on_click(move |_, _, cx| {
                        st.update(cx, |s, cx| {
                            let target = ((volume + delta) / VOLUME_STEP).round() * VOLUME_STEP;
                            let edits = set_audio_volume(&s.project, &lane_id, target);
                            s.apply(edits, cx);
                        })
                    })
                    .child(label)
            };
            base.child(div().text_color(audio_border()).child("♪"))
                .child(div().flex_1().overflow_hidden().text_color(theme::text_strong()).child(name))
                .when_some(missing, |d, error| {
                    d.child(
                        div()
                            .id(("audio-missing", index))
                            .text_color(theme::warning())
                            .text_size(px(theme::SMALL_FONT_SIZE))
                            .child("missing")
                            .tooltip(Tooltip::text(error)),
                    )
                })
                .child(step(-VOLUME_STEP, "−", "down"))
                .child(
                    div()
                        .w(px(30.))
                        .flex()
                        .justify_center()
                        .text_size(px(theme::SMALL_FONT_SIZE))
                        .child(format!("{:.0}%", lane.volume * 100.0)),
                )
                .child(step(VOLUME_STEP, "+", "up"))
                .into_any_element()
        }
        Lane::AddInstrument => {
            let pick = shell.state.clone();
            let drop_state = shell.state.clone();
            base.cursor_pointer()
                .text_color(theme::text_muted())
                .hover(|s| s.bg(theme::hover_bg()).text_color(theme::text()))
                .drag_over::<SampleDrag>(|s, _, _, _| s.bg(theme::selection_bg()).text_color(theme::text_strong()))
                .on_click(move |_, _, cx| pick.update(cx, |s, cx| s.prompt_add_instruments(cx)))
                .on_drop(move |drag: &SampleDrag, _, cx| {
                    drop_state.update(cx, |s, cx| s.add_instrument_from_sample(drag.path.clone(), cx));
                })
                .child("+ add instrument")
                .into_any_element()
        }
    }
}

type SharedId = gpui::SharedString;

fn lanes_canvas(
    shell: Entity<Shell>,
    tab: TabKind,
    frame: LanesFrame,
    drop_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
) -> gpui::Canvas<Hitbox> {
    canvas(
        move |bounds, window, _| {
            drop_bounds.set(Some(bounds));
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        },
        move |bounds, hitbox, window, _| {
            paint_lanes(bounds, &frame, window);

            let (down_shell, down_tab, down_hitbox) = (shell.clone(), tab.clone(), hitbox.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !down_hitbox.is_hovered(window) {
                    return;
                }
                let pos = local(bounds, e.position);
                down_shell.update(cx, |sh, cx| sh.tl_mouse_down(&down_tab, (origin(bounds), pos), e.button, e.modifiers, window, cx));
                cx.stop_propagation();
            });
            let (move_shell, move_tab) = (shell.clone(), tab.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble || e.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let pos = local(bounds, e.position);
                move_shell.update(cx, |sh, cx| sh.tl_mouse_move(&move_tab, origin(bounds), pos, e.modifiers, cx));
            });
            let (up_shell, up_tab) = (shell.clone(), tab.clone());
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    up_shell.update(cx, |sh, cx| sh.tl_mouse_up(&up_tab, cx));
                }
            });
            let (scroll_shell, scroll_tab) = (shell.clone(), tab.clone());
            window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                    return;
                }
                let d = e.delta.pixel_delta(window.line_height());
                let delta = point(f32::from(d.x), f32::from(d.y));
                let pos = local(bounds, e.position);
                let used = scroll_shell.update(cx, |sh, cx| sh.tl_scroll(&scroll_tab, pos, delta, e.modifiers, cx));
                if used {
                    cx.stop_propagation();
                }
            });
        },
    )
}

fn paint_lanes(bounds: Bounds<Pixels>, f: &LanesFrame, window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let view = &f.view;
    for i in 0..f.lane_count {
        window.paint_quad(fill(rect(bounds, 0.0, i as f32 * LANE_HEIGHT, width, LANE_HEIGHT), lane_bg(i)));
        window.paint_quad(fill(rect(bounds, 0.0, (i + 1) as f32 * LANE_HEIGHT - 1.0, width, 1.0), rgba(0x00000040)));
    }

    // Beat and bar lines.
    let bar = f.ts.ticks_per_bar().max(1);
    let beat = (PPQ * 4 / u32::from(f.ts.denominator.max(1))).max(1);
    let show_beats = beat as f32 * view.px_per_tick >= 8.0;
    let last = view.tick_at(width).max(0.0) as Tick;
    let step = if show_beats { beat } else { bar };
    let mut t = (view.scroll.max(0.0) as Tick / step) * step;
    while t <= last {
        let color = if t.is_multiple_of(bar) { bar_line() } else { beat_line() };
        window.paint_quad(fill(rect(bounds, view.x_of(t), 0.0, 1.0, height), color));
        t += step;
    }

    let x_of = |tick: f32| view.x_of(0) + tick * view.px_per_tick;

    // Pattern outlines span the instrument lanes.
    let outline_h = f.instrument_lanes as f32 * LANE_HEIGHT;
    for &(start, end, _) in &f.outlines {
        let (x0, x1) = (x_of(start), x_of(end));
        if x1 < 0.0 || x0 > width {
            continue;
        }
        window.paint_quad(fill(rect(bounds, x0, 0.0, x1 - x0, outline_h), outline_fill()));
        for (x, y, w, h) in [(x0, 0.0, x1 - x0, 1.0), (x0, outline_h - 1.0, x1 - x0, 1.0), (x0, 0.0, 1.0, outline_h), (x1 - 1.0, 0.0, 1.0, outline_h)] {
            window.paint_quad(fill(rect(bounds, x, y, w, h), outline_border()));
        }
    }

    for b in &f.blocks {
        let (x0, x1) = (x_of(b.start), x_of(b.end));
        if x1 < 0.0 || x0 > width {
            continue;
        }
        let y = b.lane as f32 * LANE_HEIGHT + 3.0;
        let h = LANE_HEIGHT - 6.0;
        window.paint_quad(fill(rect(bounds, x0, y, x1 - x0, h), b.border));
        window.paint_quad(fill(rect(bounds, x0 + 1.0, y + 1.0, x1 - x0 - 2.0, h - 2.0), b.fill));
        let w = x1 - x0;
        for &(s, l, p) in &b.notes {
            let nx = x0 + s * w;
            let nw = (l * w).max(1.5).min(x1 - nx - 1.0);
            let ny = y + 3.0 + (1.0 - p) * (h - 22.0);
            window.paint_quad(fill(rect(bounds, nx, ny, nw, 2.0), note_mark()));
        }
    }

    if let Some(c) = f.cursor {
        window.paint_quad(fill(rect(bounds, view.x_of(c), 0.0, 1.0, height), song_cursor_color()));
    }
    if let Some(p) = f.playhead {
        window.paint_quad(fill(rect(bounds, view.x_of(p), 0.0, 1.5, height), playhead_color()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_core::{Clip, ClipKey, Instrument, InstrumentId, Pattern, PatternId, Timeline};

    fn key(n: u32) -> ClipKey {
        ClipKey { instrument: InstrumentId("bell".into()), number: n }
    }

    fn project() -> Project {
        let mut p = Project::default();
        p.instruments.push(Instrument::new(InstrumentId("bell".into()), "Bell"));
        let mut t = Timeline::default();
        t.clips.push(Clip::new(key(1), 3840));
        t.placements = vec![Placement { clip: key(1), at: 0 }, Placement { clip: key(1), at: 3840 }];
        p.patterns.push(Pattern { id: PatternId("p1".into()), name: "Pattern 1".into(), timeline: t });
        p.song.patterns.push(PatternPlacement { pattern: PatternId("p1".into()), at: 3840 });
        p
    }

    fn clip_sel(at: Tick) -> Sel {
        Sel::Clip { container: Container::Pattern(PatternId("p1".into())), placement: Placement { clip: key(1), at } }
    }

    #[test]
    fn drags_follow_only_the_canvas_they_started_in() {
        // The same tab split into two panes: both canvases see every mouse move.
        let left = point(200.0, 80.0);
        let right = point(900.0, 80.0);
        assert!(same_canvas(Some(right), right));
        assert!(!same_canvas(Some(right), left), "the other pane's moves are ignored");
        assert!(!same_canvas(None, left), "no drag, nothing to follow");
    }

    #[test]
    fn move_clamps_at_zero_and_reselects() {
        let p = project();
        let (edits, after) = move_edits(&p, &[clip_sel(3840)], -9999, false);
        assert_eq!(after, vec![clip_sel(0)]);
        let mut q = p.clone();
        for e in &edits {
            e.apply(&mut q);
        }
        assert_eq!(q.patterns[0].timeline.placements.iter().filter(|pl| pl.at == 0).count(), 2);
    }

    #[test]
    fn copy_move_adds_and_keeps_originals() {
        let p = project();
        let pp = Sel::Pattern(PatternPlacement { pattern: PatternId("p1".into()), at: 3840 });
        let (edits, after) = move_edits(&p, &[clip_sel(0), pp], 7680, true);
        let mut q = p.clone();
        for e in &edits {
            e.apply(&mut q);
        }
        assert_eq!(q.patterns[0].timeline.uses(&key(1)), 3);
        assert_eq!(q.song.patterns.len(), 2);
        assert_eq!(after.len(), 2);
    }

    #[test]
    fn delete_edits_cover_every_kind() {
        let p = project();
        let pp = Sel::Pattern(PatternPlacement { pattern: PatternId("p1".into()), at: 3840 });
        let edits = delete_edits(&p, &[clip_sel(0), clip_sel(3840), pp]);
        let mut q = p.clone();
        for e in &edits {
            e.apply(&mut q);
        }
        assert!(q.patterns[0].timeline.clips.is_empty(), "the last block took its clip with it");
        assert!(q.song.patterns.is_empty());
    }

    #[test]
    fn duplicate_repeats_each_kind() {
        let p = project();
        let pp = Sel::Pattern(PatternPlacement { pattern: PatternId("p1".into()), at: 3840 });
        let (edits, after) = duplicate_edits(&p, &[clip_sel(3840), pp]);
        let mut q = p.clone();
        for e in &edits {
            e.apply(&mut q);
        }
        assert!(q.patterns[0].timeline.placements.contains(&Placement { clip: key(1), at: 7680 }));
        assert!(q.song.patterns.contains(&PatternPlacement { pattern: PatternId("p1".into()), at: 3840 + 7680 }));
        assert_eq!(after.len(), 2);
    }
}
