//! Piano roll editor and its velocity lane (shown in the bottom panel's Editor tab).
//!
//! Geometry and note math live in `nw_workspace::piano_roll`; this module draws them and
//! turns mouse and keyboard input into edits. A drag edits a local copy of the notes and
//! commits one `ReplaceNotes` on release, so ⌘Z undoes the whole drag.

use std::collections::HashMap;

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Entity, FocusHandle, Hitbox, HitboxBehavior,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Rgba,
    ScrollWheelEvent, Window, canvas, div, fill, point, prelude::*, px, rgb, rgba, size,
};
use nw_core::{Edit, InstrumentId, Note, PPQ, PatternId, Tick, TimeSignature};
use nw_workspace::piano_roll::{
    DEFAULT_SNAP, Hit, PianoRollView, duplicated, hit_test, moved, place_note, resized,
    with_velocity,
};
use nw_workspace::TabKind;

use crate::actions::piano_roll as act;
use crate::app_state::AppState;
use crate::theme;
use crate::ui::instrument_tab::note_name;
use crate::ui::shell::Shell;
use crate::ui::status_bar::format_position;

pub const KEYBOARD_WIDTH: f32 = 40.0;
const SNAP: Tick = DEFAULT_SNAP;
const MIN_PX_PER_TICK: f32 = 0.01;
const MAX_PX_PER_TICK: f32 = 1.0;
/// Velocity bars are hit within this many pixels of their left edge.
const VELOCITY_HIT_PX: f32 = 6.0;
const VELOCITY_BAR_WIDTH: f32 = 4.0;
/// A preview note sounds at most this long.
const MAX_PREVIEW_TICKS: Tick = PPQ * 2;

pub type RollKey = (PatternId, InstrumentId);

/// Per-tab piano roll state. Lives in `Shell::rolls` while the roll's tab is open.
pub struct RollState {
    pub view: PianoRollView,
    /// Indices into the instrument's notes in this pattern.
    pub selected: Vec<usize>,
    /// Length for newly placed notes: the last note clicked or resized.
    pub last_length: Tick,
    pub focus: FocusHandle,
    drag: Option<RollDrag>,
    /// The note under the cursor, for the hover label.
    hover: Option<usize>,
    /// Sub-row vertical scroll left over from trackpad deltas.
    scroll_rest: f32,
}

enum RollDrag {
    /// Moving the selection. `before` is the full note list when the drag began.
    Move { origin: Point<f32>, before: Vec<Note>, preview: Vec<Note> },
    Resize { origin_x: f32, before: Vec<Note>, preview: Vec<Note> },
    /// ⌥-drag on empty space. `base` is the selection kept from before (shift).
    Rubber { origin: Point<f32>, current: Point<f32>, base: Vec<usize> },
    /// Dragging velocity bars in the lane: `targets` are the notes being set.
    Velocity { targets: Vec<usize>, before: Vec<Note>, preview: Vec<Note> },
}

impl RollState {
    fn new(notes: &[Note], cx: &App) -> RollState {
        let mut view = PianoRollView::default();
        // Start with the highest note a little below the top edge.
        if let Some(high) = notes.iter().map(|n| n.pitch).max() {
            view.top_pitch = high.saturating_add(6).clamp(24, 127);
        }
        RollState {
            view,
            selected: Vec::new(),
            last_length: SNAP * 4,
            focus: cx.focus_handle(),
            drag: None,
            hover: None,
            scroll_rest: 0.0,
        }
    }

    /// The notes to draw: the live drag preview, or the stored notes.
    fn shown<'a>(&'a self, stored: &'a [Note]) -> &'a [Note] {
        match &self.drag {
            Some(RollDrag::Move { preview, .. })
            | Some(RollDrag::Resize { preview, .. })
            | Some(RollDrag::Velocity { preview, .. }) => preview,
            _ => stored,
        }
    }

    fn is_selected(&self, i: usize) -> bool {
        self.selected.contains(&i)
    }
}

fn notes_of<'a>(state: &'a AppState, key: &RollKey) -> &'a [Note] {
    state.project.pattern(&key.0).and_then(|p| p.notes.get(&key.1)).map_or(&[][..], Vec::as_slice)
}

/// Indices in `notes` of each note in `wanted`, each index used at most once.
fn reselect(notes: &[Note], wanted: &[Note]) -> Vec<usize> {
    let mut used = vec![false; notes.len()];
    let mut out = Vec::new();
    for w in wanted {
        if let Some(i) = (0..notes.len()).find(|&i| !used[i] && &notes[i] == w) {
            used[i] = true;
            out.push(i);
        }
    }
    out
}

/// Notes in `indices` (skipping stale ones).
fn pick(notes: &[Note], indices: &[usize]) -> Vec<Note> {
    indices.iter().filter_map(|&i| notes.get(i).cloned()).collect()
}

/// `4/16` for whole sixteenths, otherwise ticks.
pub fn format_length(length: Tick) -> String {
    let sixteenth = PPQ / 4;
    if length.is_multiple_of(sixteenth) { format!("{}/16", length / sixteenth) } else { format!("{length} ticks") }
}

/// Snapped tick difference between two x positions.
fn snapped_delta(view: &PianoRollView, from_x: f32, to_x: f32) -> i64 {
    let ticks = (to_x - from_x) / view.px_per_tick;
    (ticks / SNAP as f32).round() as i64 * i64::from(SNAP)
}

fn row_of(view: &PianoRollView, y: f32) -> i64 {
    (y / view.row_height).floor() as i64
}

impl Shell {
    /// Keeps one `RollState` per open piano roll tab: adds new ones, drops closed ones.
    pub fn sync_rolls(&mut self, cx: &mut Context<Shell>) {
        let state = self.state.read(cx);
        let open: Vec<RollKey> = state
            .workspace
            .panes
            .iter()
            .flat_map(|p| p.tabs.iter())
            .filter_map(|t| match t {
                TabKind::PianoRoll(p, i) => Some((p.clone(), i.clone())),
                _ => None,
            })
            .collect();
        self.rolls.retain(|k, _| open.contains(k));
        for key in open {
            if let std::collections::hash_map::Entry::Vacant(slot) = self.rolls.entry(key) {
                let roll = RollState::new(notes_of(state, slot.key()), cx);
                slot.insert(roll);
            }
        }
        let state = self.state.read(cx);
        for (key, roll) in &mut self.rolls {
            let len = notes_of(state, key).len();
            roll.selected.retain(|&i| i < len);
            if roll.hover.is_some_and(|h| h >= len) {
                roll.hover = None;
            }
        }
    }

    /// The roll whose keyboard focus is active (for piano roll actions).
    fn focused_roll(&self, window: &Window, cx: &App) -> Option<RollKey> {
        self.rolls.iter().find(|(_, r)| r.focus.contains_focused(window, cx)).map(|(k, _)| k.clone())
    }
}

/// Commits `before → after` (selected notes only) and reselects `after`.
fn commit_replace(shell: &mut Shell, key: &RollKey, before: Vec<Note>, after: Vec<Note>, cx: &mut Context<Shell>) {
    if before == after {
        return;
    }
    let edit = Edit::ReplaceNotes { pattern: key.0.clone(), instrument: key.1.clone(), before, after: after.clone() };
    shell.state.update(cx, |s, cx| s.apply(vec![edit], cx));
    let notes = notes_of(shell.state.read(cx), key).to_vec();
    if let Some(roll) = shell.rolls.get_mut(key) {
        roll.selected = reselect(&notes, &after);
    }
}

// Mouse handling.
impl Shell {
    fn roll_mouse_down(
        &mut self,
        key: &RollKey,
        pos: Point<f32>,
        button: MouseButton,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let notes = notes_of(self.state.read(cx), key).to_vec();
        let Some(roll) = self.rolls.get_mut(key) else { return };
        window.focus(&roll.focus);
        let hit = hit_test(&notes, &roll.view, pos.x, pos.y, SNAP);
        match (button, hit) {
            (MouseButton::Right, Hit::Body(i) | Hit::RightEdge(i)) => {
                roll.selected.clear();
                roll.hover = None;
                let edit = Edit::RemoveNotes {
                    pattern: key.0.clone(),
                    instrument: key.1.clone(),
                    notes: vec![notes[i].clone()],
                };
                self.state.update(cx, |s, cx| s.apply(vec![edit], cx));
            }
            (MouseButton::Left, Hit::Body(i)) => {
                roll.last_length = notes[i].length;
                if modifiers.shift {
                    if let Some(at) = roll.selected.iter().position(|&s| s == i) {
                        roll.selected.remove(at);
                    } else {
                        roll.selected.push(i);
                    }
                } else {
                    if !roll.is_selected(i) {
                        roll.selected = vec![i];
                    }
                    roll.drag = Some(RollDrag::Move { origin: pos, before: notes.clone(), preview: notes.clone() });
                }
                let n = &notes[i];
                let (pitch, velocity, length) = (n.pitch, n.velocity, n.length);
                self.state.update(cx, |s, _| s.preview_note(&key.1, pitch, velocity, length.min(MAX_PREVIEW_TICKS)));
            }
            (MouseButton::Left, Hit::RightEdge(i)) => {
                if !roll.is_selected(i) {
                    roll.selected = vec![i];
                }
                roll.drag = Some(RollDrag::Resize { origin_x: pos.x, before: notes.clone(), preview: notes });
            }
            (MouseButton::Left, Hit::Empty { tick, pitch }) => {
                if modifiers.alt {
                    let base = if modifiers.shift { roll.selected.clone() } else { Vec::new() };
                    roll.selected = base.clone();
                    roll.drag = Some(RollDrag::Rubber { origin: pos, current: pos, base });
                } else if pos.y >= 0.0 {
                    let note = place_note(tick, pitch, roll.last_length);
                    let edit = Edit::AddNotes {
                        pattern: key.0.clone(),
                        instrument: key.1.clone(),
                        notes: vec![note.clone()],
                    };
                    self.state.update(cx, |s, cx| {
                        s.apply(vec![edit], cx);
                        s.preview_note(&key.1, note.pitch, note.velocity, note.length.min(MAX_PREVIEW_TICKS));
                    });
                    let notes = notes_of(self.state.read(cx), key).to_vec();
                    if let Some(roll) = self.rolls.get_mut(key) {
                        roll.selected = reselect(&notes, &[note]);
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn roll_mouse_move(&mut self, key: &RollKey, pos: Point<f32>, hovered: bool, cx: &mut Context<Shell>) {
        let notes = notes_of(self.state.read(cx), key).to_vec();
        let Some(roll) = self.rolls.get_mut(key) else { return };
        let view = roll.view;
        match &mut roll.drag {
            Some(RollDrag::Move { origin, before, preview }) => {
                let dt = snapped_delta(&view, origin.x, pos.x);
                let dp = (row_of(&view, origin.y) - row_of(&view, pos.y)) as i32;
                let next = moved(before, &roll.selected, dt, dp);
                if &next != preview {
                    *preview = next;
                    cx.notify();
                }
            }
            Some(RollDrag::Resize { origin_x, before, preview }) => {
                let next = resized(before, &roll.selected, snapped_delta(&view, *origin_x, pos.x), SNAP);
                if &next != preview {
                    *preview = next;
                    cx.notify();
                }
            }
            Some(RollDrag::Rubber { origin, current, base }) => {
                *current = pos;
                let (x0, x1) = (origin.x.min(pos.x), origin.x.max(pos.x));
                let (y0, y1) = (origin.y.min(pos.y), origin.y.max(pos.y));
                let mut selected = base.clone();
                for (i, n) in notes.iter().enumerate() {
                    let (x, y, w, h) = view.note_rect(n);
                    if x < x1 && x + w > x0 && y < y1 && y + h > y0 && !selected.contains(&i) {
                        selected.push(i);
                    }
                }
                roll.selected = selected;
                cx.notify();
            }
            Some(RollDrag::Velocity { .. }) => {}
            None => {
                let hover = if hovered {
                    match hit_test(&notes, &view, pos.x, pos.y, SNAP) {
                        Hit::Body(i) | Hit::RightEdge(i) => Some(i),
                        Hit::Empty { .. } => None,
                    }
                } else {
                    None
                };
                if hover != roll.hover {
                    roll.hover = hover;
                    cx.notify();
                }
            }
        }
    }

    fn roll_mouse_up(&mut self, key: &RollKey, cx: &mut Context<Shell>) {
        let Some(roll) = self.rolls.get_mut(key) else { return };
        let Some(drag) = roll.drag.take() else { return };
        let selected = roll.selected.clone();
        match drag {
            RollDrag::Move { before, preview, .. } => {
                commit_replace(self, key, pick(&before, &selected), pick(&preview, &selected), cx);
            }
            RollDrag::Resize { before, preview, .. } => {
                if let Some(n) = selected.first().and_then(|&first| preview.get(first)) {
                    roll.last_length = n.length;
                }
                commit_replace(self, key, pick(&before, &selected), pick(&preview, &selected), cx);
            }
            RollDrag::Velocity { targets, before, preview } => {
                commit_replace(self, key, pick(&before, &targets), pick(&preview, &targets), cx);
                // Keep the selection as it was (reselect picked the targets).
                if let Some(roll) = self.rolls.get_mut(key) {
                    let notes = notes_of(self.state.read(cx), key).to_vec();
                    let kept = pick(&preview, &selected);
                    roll.selected = reselect(&notes, &kept);
                }
            }
            RollDrag::Rubber { .. } => {}
        }
        cx.notify();
    }

    /// Trackpad: scroll pitch (vertical) and time (horizontal). ⌘+scroll zooms time.
    fn roll_scroll(&mut self, key: &RollKey, pos: Point<f32>, delta: Point<f32>, modifiers: Modifiers, height: f32, cx: &mut Context<Shell>) {
        let Some(roll) = self.rolls.get_mut(key) else { return };
        let view = &mut roll.view;
        if modifiers.platform {
            let anchor = view.tick_at(pos.x);
            let factor = (delta.y * 0.01).exp();
            view.px_per_tick = (view.px_per_tick * factor).clamp(MIN_PX_PER_TICK, MAX_PX_PER_TICK);
            view.scroll_ticks = (anchor - pos.x / view.px_per_tick).max(0.0);
        } else {
            view.scroll_ticks = (view.scroll_ticks - delta.x / view.px_per_tick).max(0.0);
            roll.scroll_rest += delta.y;
            let rows = (roll.scroll_rest / view.row_height).trunc();
            roll.scroll_rest -= rows * view.row_height;
            let visible = (height / view.row_height).floor() as i32;
            let lowest_top = (visible - 1).clamp(0, 127);
            let top = (i32::from(view.top_pitch) + rows as i32).clamp(lowest_top, 127);
            view.top_pitch = top as u8;
        }
        cx.notify();
    }

    fn lane_mouse_down(&mut self, key: &RollKey, pos: Point<f32>, height: f32, cx: &mut Context<Shell>) {
        let notes = notes_of(self.state.read(cx), key).to_vec();
        let Some(roll) = self.rolls.get_mut(key) else { return };
        let hit = notes.iter().enumerate().rev().find(|(_, n)| {
            let (x, ..) = roll.view.note_rect(n);
            pos.x >= x - VELOCITY_HIT_PX / 2.0 && pos.x <= x + VELOCITY_HIT_PX
        });
        let Some((i, _)) = hit else { return };
        let targets = if roll.is_selected(i) { roll.selected.clone() } else { vec![i] };
        let velocity = lane_velocity(pos.y, height);
        let preview = with_velocity(&notes, &targets, velocity);
        roll.drag = Some(RollDrag::Velocity { targets, before: notes, preview });
        cx.notify();
    }

    fn lane_mouse_move(&mut self, key: &RollKey, pos: Point<f32>, height: f32, cx: &mut Context<Shell>) {
        let Some(roll) = self.rolls.get_mut(key) else { return };
        if let Some(RollDrag::Velocity { targets, before, preview }) = &mut roll.drag {
            let next = with_velocity(before, targets, lane_velocity(pos.y, height));
            if &next != preview {
                *preview = next;
                cx.notify();
            }
        }
    }
}

fn lane_velocity(y: f32, height: f32) -> u8 {
    let t = 1.0 - (y / height.max(1.0)).clamp(0.0, 1.0);
    (t * 127.0).round().clamp(1.0, 127.0) as u8
}

// Keyboard actions (context `PianoRoll`). Each one is a single undo step.
impl Shell {
    fn with_focused_roll(
        &mut self,
        window: &Window,
        cx: &mut Context<Shell>,
        f: impl FnOnce(&mut Shell, &RollKey, Vec<Note>, Vec<usize>, &mut Context<Shell>),
    ) {
        let Some(key) = self.focused_roll(window, cx) else { return };
        let notes = notes_of(self.state.read(cx), &key).to_vec();
        let selected = self.rolls.get(&key).map(|r| r.selected.clone()).unwrap_or_default();
        f(self, &key, notes, selected, cx);
        cx.notify();
    }

    fn roll_delete(&mut self, _: &act::Delete, window: &mut Window, cx: &mut Context<Shell>) {
        self.with_focused_roll(window, cx, |shell, key, notes, selected, cx| {
            let doomed = pick(&notes, &selected);
            if doomed.is_empty() {
                return;
            }
            if let Some(roll) = shell.rolls.get_mut(key) {
                roll.selected.clear();
                roll.hover = None;
            }
            let edit = Edit::RemoveNotes { pattern: key.0.clone(), instrument: key.1.clone(), notes: doomed };
            shell.state.update(cx, |s, cx| s.apply(vec![edit], cx));
        });
    }

    fn roll_select_all(&mut self, _: &act::SelectAll, window: &mut Window, cx: &mut Context<Shell>) {
        self.with_focused_roll(window, cx, |shell, key, notes, _, _| {
            if let Some(roll) = shell.rolls.get_mut(key) {
                roll.selected = (0..notes.len()).collect();
            }
        });
    }

    fn roll_duplicate(&mut self, _: &act::Duplicate, window: &mut Window, cx: &mut Context<Shell>) {
        self.with_focused_roll(window, cx, |shell, key, notes, selected, cx| {
            let (all, copies) = duplicated(&notes, &selected);
            let added = pick(&all, &copies);
            if added.is_empty() {
                return;
            }
            let edit = Edit::AddNotes { pattern: key.0.clone(), instrument: key.1.clone(), notes: added.clone() };
            shell.state.update(cx, |s, cx| s.apply(vec![edit], cx));
            let now = notes_of(shell.state.read(cx), key).to_vec();
            if let Some(roll) = shell.rolls.get_mut(key) {
                roll.selected = reselect(&now, &added);
            }
        });
    }

    fn roll_nudge(&mut self, d_ticks: i64, d_pitch: i32, window: &Window, cx: &mut Context<Shell>) {
        self.with_focused_roll(window, cx, |shell, key, notes, selected, cx| {
            let after = moved(&notes, &selected, d_ticks, d_pitch);
            commit_replace(shell, key, pick(&notes, &selected), pick(&after, &selected), cx);
        });
    }

    fn roll_nudge_left(&mut self, _: &act::NudgeLeft, window: &mut Window, cx: &mut Context<Shell>) {
        self.roll_nudge(-i64::from(SNAP), 0, window, cx);
    }
    fn roll_nudge_right(&mut self, _: &act::NudgeRight, window: &mut Window, cx: &mut Context<Shell>) {
        self.roll_nudge(i64::from(SNAP), 0, window, cx);
    }
    fn roll_nudge_up(&mut self, _: &act::NudgeUp, window: &mut Window, cx: &mut Context<Shell>) {
        self.roll_nudge(0, 1, window, cx);
    }
    fn roll_nudge_down(&mut self, _: &act::NudgeDown, window: &mut Window, cx: &mut Context<Shell>) {
        self.roll_nudge(0, -1, window, cx);
    }
    fn roll_octave_up(&mut self, _: &act::OctaveUp, window: &mut Window, cx: &mut Context<Shell>) {
        self.roll_nudge(0, 12, window, cx);
    }
    fn roll_octave_down(&mut self, _: &act::OctaveDown, window: &mut Window, cx: &mut Context<Shell>) {
        self.roll_nudge(0, -12, window, cx);
    }
}

// Colors local to the roll.
fn white_row() -> Rgba {
    rgb(0x20232a)
}
fn black_row() -> Rgba {
    rgb(0x1a1c21)
}
fn snap_line() -> Rgba {
    rgba(0xffffff0a)
}
fn beat_line() -> Rgba {
    rgba(0xffffff18)
}
fn bar_line() -> Rgba {
    rgba(0xffffff36)
}
fn past_end() -> Rgba {
    rgba(0x00000066)
}
fn note_fill() -> Rgba {
    rgb(0x4f7fd0)
}
fn note_selected() -> Rgba {
    rgb(0x9cc2ff)
}
fn note_edge() -> Rgba {
    rgba(0x00000080)
}
fn playhead_color() -> Rgba {
    rgb(0xffd166)
}
fn rubber_fill() -> Rgba {
    rgba(0x6aa0ff22)
}

fn is_black(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// Everything the grid paints, copied out of the state so the paint closure owns it.
struct GridFrame {
    view: PianoRollView,
    notes: Vec<Note>,
    selected: Vec<usize>,
    length: Tick,
    ts: TimeSignature,
    playhead: Option<Tick>,
    rubber: Option<(Point<f32>, Point<f32>)>,
}

pub fn render(shell: &Shell, state: &AppState, key: &RollKey, cx: &Context<Shell>) -> AnyElement {
    let Some(roll) = shell.rolls.get(key) else {
        return div().into_any_element();
    };
    let stored = notes_of(state, key);
    let notes = roll.shown(stored).to_vec();
    let playing_here = state.playing && state.last_played_pattern.as_ref() == Some(&key.0);
    let frame = GridFrame {
        view: roll.view,
        notes: notes.clone(),
        selected: roll.selected.clone(),
        length: state.project.pattern_length(&key.0),
        ts: state.project.time_signature,
        playhead: playing_here.then_some(state.playhead),
        rubber: match &roll.drag {
            Some(RollDrag::Rubber { origin, current, .. }) => Some((*origin, *current)),
            _ => None,
        },
    };
    let hover_label = roll.hover.filter(|_| roll.drag.is_none()).and_then(|i| notes.get(i)).map(|n| {
        let (x, y, _, _) = roll.view.note_rect(n);
        let text = format!(
            "{} ({}) · {} · {} · vel {}",
            note_name(n.pitch),
            n.pitch,
            format_position(n.start, frame.ts),
            format_length(n.length),
            n.velocity
        );
        let top = if y >= 22.0 { y - 22.0 } else { y + roll.view.row_height + 4.0 };
        div()
            .absolute()
            .left(px(x.max(0.0)))
            .top(px(top))
            .px(px(6.))
            .py(px(2.))
            .rounded(px(3.))
            .bg(theme::chrome_bg())
            .border_1()
            .border_color(theme::border())
            .text_size(px(theme::SMALL_FONT_SIZE))
            .text_color(theme::text_strong())
            .whitespace_nowrap()
            .child(text)
    });

    let shell_entity = cx.entity();
    div()
        .key_context("PianoRoll")
        .track_focus(&roll.focus)
        .on_action(cx.listener(Shell::roll_delete))
        .on_action(cx.listener(Shell::roll_select_all))
        .on_action(cx.listener(Shell::roll_duplicate))
        .on_action(cx.listener(Shell::roll_nudge_left))
        .on_action(cx.listener(Shell::roll_nudge_right))
        .on_action(cx.listener(Shell::roll_nudge_up))
        .on_action(cx.listener(Shell::roll_nudge_down))
        .on_action(cx.listener(Shell::roll_octave_up))
        .on_action(cx.listener(Shell::roll_octave_down))
        .size_full()
        .flex()
        .flex_row()
        .child(keyboard(&roll.view))
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_hidden()
                .child(grid(shell_entity, key.clone(), frame).absolute().inset_0())
                .children(hover_label),
        )
        .into_any_element()
}

/// The key strip: black keys darker, C notes labelled.
fn keyboard(view: &PianoRollView) -> impl IntoElement {
    let rows = (0..=127u8).rev().filter_map(|pitch| {
        let top = (f32::from(view.top_pitch) - f32::from(pitch)) * view.row_height;
        if top < -view.row_height {
            return None;
        }
        let black = is_black(pitch);
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(top))
                .h(px(view.row_height))
                .flex()
                .items_center()
                .justify_end()
                .pr(px(4.))
                .border_b_1()
                .border_color(rgba(0x0000002a))
                .bg(if black { rgb(0x2a2d33) } else { rgb(0xb9bec8) })
                .text_size(px(9.))
                .text_color(rgb(0x30333a))
                .when(pitch % 12 == 0, |d| d.child(note_name(pitch))),
        )
    });
    div()
        .relative()
        .w(px(KEYBOARD_WIDTH))
        .flex_none()
        .h_full()
        .overflow_hidden()
        .border_r_1()
        .border_color(theme::border())
        .children(rows)
}

fn local(bounds: Bounds<Pixels>, p: Point<Pixels>) -> Point<f32> {
    point(f32::from(p.x - bounds.origin.x), f32::from(p.y - bounds.origin.y))
}

fn rect(bounds: Bounds<Pixels>, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w.max(0.0)), px(h.max(0.0))))
}

fn grid(shell: Entity<Shell>, key: RollKey, frame: GridFrame) -> gpui::Canvas<Hitbox> {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            paint_grid(bounds, &frame, window);

            let (down_shell, down_key, down_hitbox) = (shell.clone(), key.clone(), hitbox.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !down_hitbox.is_hovered(window) {
                    return;
                }
                let pos = local(bounds, e.position);
                down_shell.update(cx, |sh, cx| sh.roll_mouse_down(&down_key, pos, e.button, e.modifiers, window, cx));
                cx.stop_propagation();
            });
            let (move_shell, move_key, move_hitbox) = (shell.clone(), key.clone(), hitbox.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let hovered = move_hitbox.is_hovered(window);
                let pos = local(bounds, e.position);
                move_shell.update(cx, |sh, cx| sh.roll_mouse_move(&move_key, pos, hovered, cx));
            });
            let (up_shell, up_key) = (shell.clone(), key.clone());
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    up_shell.update(cx, |sh, cx| sh.roll_mouse_up(&up_key, cx));
                }
            });
            let (scroll_shell, scroll_key) = (shell.clone(), key.clone());
            window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                    return;
                }
                let d = e.delta.pixel_delta(window.line_height());
                let delta = point(f32::from(d.x), f32::from(d.y));
                let pos = local(bounds, e.position);
                let height = f32::from(bounds.size.height);
                scroll_shell.update(cx, |sh, cx| sh.roll_scroll(&scroll_key, pos, delta, e.modifiers, height, cx));
                cx.stop_propagation();
            });
        },
    )
}

fn paint_grid(bounds: Bounds<Pixels>, f: &GridFrame, window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let view = &f.view;

    // Pitch rows.
    let rows = (height / view.row_height).ceil() as i32 + 1;
    for r in 0..rows {
        let pitch = i32::from(view.top_pitch) - r;
        if !(0..=127).contains(&pitch) {
            continue;
        }
        let color = if is_black(pitch as u8) { black_row() } else { white_row() };
        window.paint_quad(fill(rect(bounds, 0.0, r as f32 * view.row_height, width, view.row_height), color));
    }

    // Time grid: snap lines (when not too dense), stronger beats, strongest bars.
    let beat = (PPQ * 4 / u32::from(f.ts.denominator.max(1))).max(1);
    let bar = f.ts.ticks_per_bar().max(1);
    let first = view.scroll_ticks.max(0.0) as Tick / SNAP * SNAP;
    let last = view.tick_at(width).max(0.0) as Tick;
    let show_snap = SNAP as f32 * view.px_per_tick >= 5.0;
    let show_beats = beat as f32 * view.px_per_tick >= 4.0;
    let mut tick = first;
    while tick <= last {
        let color = if tick.is_multiple_of(bar) {
            Some(bar_line())
        } else if tick.is_multiple_of(beat) && show_beats {
            Some(beat_line())
        } else if show_snap {
            Some(snap_line())
        } else {
            None
        };
        if let Some(color) = color {
            let x = (tick as f32 - view.scroll_ticks) * view.px_per_tick;
            window.paint_quad(fill(rect(bounds, x, 0.0, 1.0, height), color));
        }
        tick += SNAP;
    }

    // Past the end of the pattern loop.
    let end_x = (f.length as f32 - view.scroll_ticks) * view.px_per_tick;
    if end_x < width {
        let x = end_x.max(0.0);
        window.paint_quad(fill(rect(bounds, x, 0.0, width - x, height), past_end()));
    }

    // Notes: a dark edge, then the body.
    for (i, n) in f.notes.iter().enumerate() {
        let (x, y, w, h) = view.note_rect(n);
        if x > width || x + w < 0.0 || y > height || y + h < 0.0 {
            continue;
        }
        let color = if f.selected.contains(&i) { note_selected() } else { note_fill() };
        window.paint_quad(fill(rect(bounds, x, y, w, h), note_edge()));
        window.paint_quad(fill(rect(bounds, x + 1.0, y + 1.0, w - 2.0, h - 2.0), color));
    }

    if let Some((a, b)) = f.rubber {
        let (x, y) = (a.x.min(b.x), a.y.min(b.y));
        window.paint_quad(fill(rect(bounds, x, y, (a.x - b.x).abs(), (a.y - b.y).abs()), rubber_fill()));
    }

    if let Some(tick) = f.playhead {
        let x = (tick as f32 - view.scroll_ticks) * view.px_per_tick;
        if (0.0..=width).contains(&x) {
            window.paint_quad(fill(rect(bounds, x, 0.0, 1.5, height), playhead_color()));
        }
    }
}

/// The velocity lane for the active piano roll, aligned with its time axis.
pub fn render_velocity_lane(shell: &Shell, state: &AppState, key: &RollKey, cx: &Context<Shell>) -> AnyElement {
    let Some(roll) = shell.rolls.get(key) else {
        return div().into_any_element();
    };
    let notes = roll.shown(notes_of(state, key)).to_vec();
    let selected = roll.selected.clone();
    let view = roll.view;
    let shell_entity = cx.entity();
    let key = key.clone();
    let lane = canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            for (i, n) in notes.iter().enumerate() {
                let (x, ..) = view.note_rect(n);
                if x < -VELOCITY_BAR_WIDTH || x > width {
                    continue;
                }
                let h = height * f32::from(n.velocity) / 127.0;
                let color = if selected.contains(&i) { note_selected() } else { note_fill() };
                window.paint_quad(fill(rect(bounds, x, height - h, VELOCITY_BAR_WIDTH, h), color));
                window.paint_quad(fill(rect(bounds, x, height - h, 8.0, 1.5), color));
            }

            let (down_shell, down_key, down_hitbox) = (shell_entity.clone(), key.clone(), hitbox);
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !down_hitbox.is_hovered(window) {
                    return;
                }
                let pos = local(bounds, e.position);
                down_shell.update(cx, |sh, cx| sh.lane_mouse_down(&down_key, pos, height, cx));
                cx.stop_propagation();
            });
            let (move_shell, move_key) = (shell_entity.clone(), key.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && e.pressed_button == Some(MouseButton::Left) {
                    let pos = local(bounds, e.position);
                    move_shell.update(cx, |sh, cx| sh.lane_mouse_move(&move_key, pos, height, cx));
                }
            });
            // `roll_mouse_up` commits a velocity drag like any other roll drag.
            let (up_shell, up_key) = (shell_entity.clone(), key.clone());
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    up_shell.update(cx, |sh, cx| sh.roll_mouse_up(&up_key, cx));
                }
            });
        },
    );
    div()
        .size_full()
        .flex()
        .flex_row()
        .child(
            div()
                .w(px(KEYBOARD_WIDTH))
                .flex_none()
                .h_full()
                .flex()
                .items_start()
                .justify_center()
                .pt(px(4.))
                .text_size(px(9.))
                .text_color(theme::text_faint())
                .child("VEL"),
        )
        .child(div().relative().flex_1().min_w_0().h_full().py(px(6.)).overflow_hidden().child(lane.size_full()))
        .into_any_element()
}

/// Piano roll state per open roll tab.
pub type Rolls = HashMap<RollKey, RollState>;

#[cfg(test)]
mod tests {
    use super::*;

    fn n(pitch: u8, start: Tick) -> Note {
        Note { pitch, start, length: 240, velocity: 100 }
    }

    #[test]
    fn reselect_matches_each_note_once() {
        let notes = vec![n(60, 0), n(60, 0), n(62, 240)];
        assert_eq!(reselect(&notes, &[n(60, 0), n(60, 0)]), vec![0, 1]);
        assert_eq!(reselect(&notes, &[n(62, 240), n(64, 0)]), vec![2]);
    }

    #[test]
    fn lengths_format_in_sixteenths() {
        assert_eq!(format_length(240), "1/16");
        assert_eq!(format_length(960), "4/16");
        assert_eq!(format_length(100), "100 ticks");
    }

    #[test]
    fn lane_velocity_maps_top_to_127_and_bottom_to_1() {
        assert_eq!(lane_velocity(0.0, 100.0), 127);
        assert_eq!(lane_velocity(100.0, 100.0), 1);
        assert_eq!(lane_velocity(500.0, 100.0), 1);
        assert_eq!(lane_velocity(-5.0, 100.0), 127);
    }

    #[test]
    fn snapped_delta_rounds_to_snap() {
        let view = PianoRollView::default(); // 0.1 px per tick → 24 px per snap
        assert_eq!(snapped_delta(&view, 0.0, 11.0), 0);
        assert_eq!(snapped_delta(&view, 0.0, 13.0), i64::from(SNAP));
        assert_eq!(snapped_delta(&view, 50.0, 0.0), -2 * i64::from(SNAP));
    }
}
