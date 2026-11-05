//! Root view (layout A): activity bar | sidebar | editor area over bottom panel, status bar
//! along the bottom. Owns the `Workspace` key context and the workspace-level actions.

use gpui::{
    App, Context, DragMoveEvent, Entity, FocusHandle, Focusable, ScrollHandle, Subscription,
    Window, div, prelude::*, px,
};
use nw_core::PatternId;
use nw_workspace::{SidebarView, TabKind, WorkspaceState};

use crate::actions::{edit, instrument, pattern, project, transport, workspace};
use crate::app_state::AppState;
use crate::theme;
use crate::ui::components::{TextField, TextFieldEvent};
use crate::ui::instrument_tab::SliderDrag;
use crate::ui::piano_roll::Rolls;
use crate::ui::sidebar::SidebarState;
use crate::ui::{bottom_panel, editor_area, sidebar, status_bar};

/// Drag payloads for the two resize handles.
pub struct SidebarResize;
pub struct PanelResize;

/// Drag preview for resize handles: nothing follows the cursor.
pub struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

pub struct Shell {
    pub state: Entity<AppState>,
    focus_handle: FocusHandle,
    pub sidebar: SidebarState,
    pub search_field: Entity<TextField>,
    pub tempo_field: Option<Entity<TextField>>,
    pub log_scroll: ScrollHandle,
    log_len: usize,
    tempo_subscription: Option<Subscription>,
    /// The pattern being renamed inline in the sidebar, and its field.
    pub pattern_rename: Option<(PatternId, Entity<TextField>)>,
    rename_subscription: Option<Subscription>,
    /// An instrument-tab slider drag in progress.
    pub slider: Option<SliderDrag>,
    /// State of each open piano roll tab (view, selection, drag).
    pub rolls: Rolls,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Shell {
        let search_field = cx.new(|cx| {
            TextField::new("", "Search patterns, instruments, samples", |_| true, window, cx)
        });
        let subscriptions = vec![
            cx.observe(&state, |shell, _, cx| {
                shell.sync_rolls(cx);
                cx.notify();
            }),
            cx.subscribe_in(&search_field, window, Self::on_search_event),
        ];
        let mut shell = Shell {
            state,
            focus_handle: cx.focus_handle(),
            sidebar: SidebarState::default(),
            search_field,
            tempo_field: None,
            log_scroll: ScrollHandle::new(),
            log_len: 0,
            tempo_subscription: None,
            pattern_rename: None,
            rename_subscription: None,
            slider: None,
            rolls: Rolls::new(),
            _subscriptions: subscriptions,
        };
        shell.sync_rolls(cx);
        if shell.state.read(cx).workspace.sidebar_view == SidebarView::Search {
            shell.refresh_search_index(cx);
        }
        shell
    }

    fn update_workspace(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut WorkspaceState)) {
        self.state.update(cx, |s, cx| s.update_workspace(cx, f));
    }

    fn log(&self, cx: &mut Context<Self>, line: &str) {
        self.state.update(cx, |s, cx| {
            s.log(line);
            cx.notify();
        });
    }

    /// Shows `view` in the sidebar (making the sidebar visible).
    pub fn show_view(&mut self, view: SidebarView, window: &mut Window, cx: &mut Context<Self>) {
        self.update_workspace(cx, |ws| {
            ws.sidebar_view = view;
            ws.sidebar_visible = true;
        });
        if view == SidebarView::Search {
            self.refresh_search_index(cx);
            window.focus(&self.search_field.focus_handle(cx));
        }
    }

    /// Activity bar click: a different view shows it; the current view toggles the sidebar.
    pub fn activity_clicked(&mut self, view: SidebarView, window: &mut Window, cx: &mut Context<Self>) {
        let ws = &self.state.read(cx).workspace;
        if ws.sidebar_visible && ws.sidebar_view == view {
            self.update_workspace(cx, |ws| ws.sidebar_visible = false);
        } else {
            self.show_view(view, window, cx);
        }
    }

    fn on_search_event(
        &mut self,
        _: &Entity<TextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Changed => cx.notify(),
            TextFieldEvent::Submit => sidebar::open_first_search_result(self, cx),
            TextFieldEvent::Cancel => window.focus(&self.focus_handle),
            TextFieldEvent::Blur => {}
        }
    }

    /// Replaces the tempo readout with a text field holding the current tempo.
    pub fn begin_tempo_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tempo_field.is_some() || self.state.read(cx).project_dir.is_none() {
            return;
        }
        let tempo = format_tempo(self.state.read(cx).project.tempo);
        let field = cx.new(|cx| {
            TextField::new(tempo, "BPM", |c| c.is_ascii_digit() || c == '.', window, cx)
        });
        self.tempo_subscription = Some(cx.subscribe_in(&field, window, Self::on_tempo_event));
        window.focus(&field.focus_handle(cx));
        self.tempo_field = Some(field);
        cx.notify();
    }

    fn on_tempo_event(
        &mut self,
        field: &Entity<TextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Changed => {}
            TextFieldEvent::Submit => {
                if let Ok(tempo) = field.read(cx).text().parse::<f64>() {
                    // `set_tempo` clamps to 20..=999 before committing `Edit::SetTempo`.
                    self.state.update(cx, |s, cx| s.set_tempo(tempo, cx));
                }
                self.end_tempo_edit(cx);
                window.focus(&self.focus_handle);
            }
            TextFieldEvent::Cancel => {
                self.end_tempo_edit(cx);
                window.focus(&self.focus_handle);
            }
            // Focus already moved elsewhere; just drop the field.
            TextFieldEvent::Blur => self.end_tempo_edit(cx),
        }
    }

    fn end_tempo_edit(&mut self, cx: &mut Context<Self>) {
        self.tempo_field = None;
        self.tempo_subscription = None;
        cx.notify();
    }

    /// Replaces the pattern's sidebar row with a text field holding its name.
    pub fn begin_pattern_rename(&mut self, id: PatternId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.state.read(cx).project.pattern(&id).map(|p| p.name.clone()) else { return };
        self.show_view(SidebarView::Project, window, cx);
        let field = cx.new(|cx| TextField::new(name, "pattern name", |c| !c.is_control(), window, cx));
        self.rename_subscription = Some(cx.subscribe_in(&field, window, Self::on_rename_event));
        window.focus(&field.focus_handle(cx));
        self.pattern_rename = Some((id, field));
        cx.notify();
    }

    fn on_rename_event(
        &mut self,
        field: &Entity<TextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commit = match event {
            TextFieldEvent::Changed => return,
            TextFieldEvent::Submit | TextFieldEvent::Blur => true,
            TextFieldEvent::Cancel => false,
        };
        if commit {
            let name = field.read(cx).text().to_string();
            if let Some((id, _)) = &self.pattern_rename {
                let id = id.clone();
                // `rename_pattern` ignores empty and unchanged names.
                self.state.update(cx, |s, cx| s.rename_pattern(&id, &name, cx));
            }
        }
        self.pattern_rename = None;
        self.rename_subscription = None;
        if !matches!(event, TextFieldEvent::Blur) {
            window.focus(&self.focus_handle);
        }
        cx.notify();
    }

    /// The instrument of the active tab (a sampler tab or a piano roll).
    fn active_instrument(&self, cx: &App) -> Option<nw_core::InstrumentId> {
        match self.state.read(cx).workspace.active_tab()? {
            TabKind::Instrument(i) | TabKind::PianoRoll(_, i) => Some(i.clone()),
            TabKind::Pattern(_) => None,
        }
    }

    pub fn refresh_search_index(&mut self, cx: &mut Context<Self>) {
        let folders = self.state.read(cx).settings.sample_folders.clone();
        self.sidebar.refresh_index(folders, cx);
    }

    fn resize_sidebar(&mut self, e: &DragMoveEvent<SidebarResize>, _: &mut Window, cx: &mut Context<Self>) {
        let x = f32::from(e.event.position.x) - theme::ACTIVITY_BAR_WIDTH - theme::RESIZE_HANDLE / 2.0;
        let width = x.clamp(theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH);
        if width != self.state.read(cx).workspace.sidebar_width {
            self.update_workspace(cx, |ws| ws.sidebar_width = width);
        }
    }

    fn resize_panel(&mut self, e: &DragMoveEvent<PanelResize>, window: &mut Window, cx: &mut Context<Self>) {
        let window_height = f32::from(window.viewport_size().height);
        let y = f32::from(e.event.position.y);
        let height = (window_height - theme::STATUS_BAR_HEIGHT - y)
            .clamp(theme::PANEL_MIN_HEIGHT, max_panel_height(window_height));
        if height != self.state.read(cx).workspace.panel_height {
            self.update_workspace(cx, |ws| ws.panel_height = height);
        }
    }

    // Workspace-level action handlers.

    fn toggle_sidebar(&mut self, _: &workspace::ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.update_workspace(cx, |ws| ws.sidebar_visible = !ws.sidebar_visible);
    }
    fn toggle_panel(&mut self, _: &workspace::TogglePanel, _: &mut Window, cx: &mut Context<Self>) {
        self.update_workspace(cx, |ws| ws.panel_visible = !ws.panel_visible);
    }
    fn close_tab(&mut self, _: &workspace::CloseTab, _: &mut Window, cx: &mut Context<Self>) {
        self.update_workspace(cx, WorkspaceState::close_active);
    }
    fn next_tab(&mut self, _: &workspace::NextTab, _: &mut Window, cx: &mut Context<Self>) {
        self.update_workspace(cx, WorkspaceState::activate_next);
    }
    fn prev_tab(&mut self, _: &workspace::PrevTab, _: &mut Window, cx: &mut Context<Self>) {
        self.update_workspace(cx, WorkspaceState::activate_prev);
    }
    fn show_project(&mut self, _: &workspace::ShowProject, window: &mut Window, cx: &mut Context<Self>) {
        self.show_view(SidebarView::Project, window, cx);
    }
    fn show_sounds(&mut self, _: &workspace::ShowSounds, window: &mut Window, cx: &mut Context<Self>) {
        self.show_view(SidebarView::Sounds, window, cx);
    }
    fn show_search(&mut self, _: &workspace::ShowSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.show_view(SidebarView::Search, window, cx);
    }
    fn new_project(&mut self, _: &project::New, _: &mut Window, cx: &mut Context<Self>) {
        self.log(cx, "New Project… is not available yet");
    }
    fn open_project(&mut self, _: &project::Open, _: &mut Window, cx: &mut Context<Self>) {
        self.log(cx, "Open Project… is not available yet");
    }
    fn save_project(&mut self, _: &project::Save, _: &mut Window, cx: &mut Context<Self>) {
        self.log(cx, "Saving is not available yet");
    }
    fn new_pattern(&mut self, _: &project::NewPattern, window: &mut Window, cx: &mut Context<Self>) {
        self.create_pattern(window, cx);
    }
    /// Adds a pattern, opens its rack and starts renaming it in the sidebar.
    pub fn create_pattern(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.state.update(cx, |s, cx| s.new_pattern(cx)) {
            self.begin_pattern_rename(id, window, cx);
        }
    }
    fn rename_pattern(&mut self, _: &pattern::Rename, window: &mut Window, cx: &mut Context<Self>) {
        match self.state.read(cx).workspace.active_pattern() {
            Some(id) => self.begin_pattern_rename(id, window, cx),
            None => self.log(cx, "Rename Pattern: open a pattern first"),
        }
    }
    fn delete_pattern(&mut self, _: &pattern::Delete, _: &mut Window, cx: &mut Context<Self>) {
        match self.state.read(cx).workspace.active_pattern() {
            Some(id) => self.state.update(cx, |s, cx| s.delete_pattern(&id, cx)),
            None => self.log(cx, "Delete Pattern: open a pattern first"),
        }
    }
    fn delete_instrument(&mut self, _: &instrument::Delete, _: &mut Window, cx: &mut Context<Self>) {
        match self.active_instrument(cx) {
            Some(id) => self.state.update(cx, |s, cx| s.delete_instrument(&id, cx)),
            None => self.log(cx, "Delete Instrument: open an instrument or piano roll first"),
        }
    }
    fn add_sample_folder(&mut self, _: &project::AddSampleFolder, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.add_sample_folder(cx));
    }
    fn undo(&mut self, _: &edit::Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.undo(cx));
    }
    fn redo(&mut self, _: &edit::Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.redo(cx));
    }
    fn play_stop(&mut self, _: &transport::PlayStop, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.play_stop(cx));
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let log_len = self.state.read(cx).log.len();
        if log_len != self.log_len {
            self.log_len = log_len;
            if log_len > 0 {
                self.log_scroll.scroll_to_item(log_len - 1);
            }
        }

        let window_height = f32::from(window.viewport_size().height);
        let this = &*self;
        let cx = &*cx;
        let ws = &this.state.read(cx).workspace;
        let sidebar_width =
            sanitize(ws.sidebar_width, 240.0).clamp(theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH);
        let panel_height = sanitize(ws.panel_height, 180.0)
            .clamp(theme::PANEL_MIN_HEIGHT, max_panel_height(window_height));

        let main = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(editor_area::render(this, cx))
            .when(ws.panel_visible, |d| {
                d.child(
                    div()
                        .id("panel-resize")
                        .h(px(theme::RESIZE_HANDLE))
                        .w_full()
                        .bg(theme::sidebar_bg())
                        .border_t_1()
                        .border_color(theme::border())
                        .cursor_row_resize()
                        .on_drag(PanelResize, |_, _, _, cx| cx.new(|_| DragGhost)),
                )
                .child(bottom_panel::render(this, panel_height, cx))
            });

        div()
            .key_context("Workspace")
            .track_focus(&this.focus_handle)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_panel))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::prev_tab))
            .on_action(cx.listener(Self::show_project))
            .on_action(cx.listener(Self::show_sounds))
            .on_action(cx.listener(Self::show_search))
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::open_project))
            .on_action(cx.listener(Self::save_project))
            .on_action(cx.listener(Self::new_pattern))
            .on_action(cx.listener(Self::rename_pattern))
            .on_action(cx.listener(Self::delete_pattern))
            .on_action(cx.listener(Self::delete_instrument))
            .on_action(cx.listener(Self::add_sample_folder))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::play_stop))
            .on_drag_move(cx.listener(Self::resize_sidebar))
            .on_drag_move(cx.listener(Self::resize_panel))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::editor_bg())
            .text_color(theme::text())
            .text_size(px(theme::FONT_SIZE))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(sidebar::render_activity_bar(this, cx))
                    .when(ws.sidebar_visible, |d| {
                        d.child(sidebar::render(this, sidebar_width, cx)).child(
                            div()
                                .id("sidebar-resize")
                                .w(px(theme::RESIZE_HANDLE))
                                .h_full()
                                .bg(theme::sidebar_bg())
                                .border_r_1()
                                .border_color(theme::border())
                                .cursor_col_resize()
                                .on_drag(SidebarResize, |_, _, _, cx| cx.new(|_| DragGhost)),
                        )
                    })
                    .child(main),
            )
            .child(status_bar::render(this, cx))
    }
}

fn max_panel_height(window_height: f32) -> f32 {
    (window_height - theme::PANEL_RESERVED_HEIGHT).max(theme::PANEL_MIN_HEIGHT)
}

/// Sizes come from a hand-editable file; replace non-finite values with a default.
fn sanitize(value: f32, default: f32) -> f32 {
    if value.is_finite() { value } else { default }
}

/// `140` for whole tempos, `140.5` otherwise.
pub fn format_tempo(tempo: f64) -> String {
    if tempo.fract() == 0.0 { format!("{tempo:.0}") } else { format!("{tempo}") }
}
