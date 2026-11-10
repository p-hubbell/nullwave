//! Editor area: one tab bar per pane and the active editor below it. Without a project it
//! shows the New / Open buttons instead.

use gpui::{AnyElement, Context, DragMoveEvent, MouseButton, SharedString, Window, div, prelude::*, px, relative};
use nw_workspace::{TabKind, tab_title};

use crate::actions::project;
use crate::app_state::AppState;
use crate::theme;
use crate::ui::components::button;
use crate::ui::{instrument_tab, pattern_tab, piano_roll};
use crate::ui::shell::{DragGhost, Shell};

/// Drag payload for the divider between split panes.
pub struct SplitResize;

const MIN_SPLIT: f32 = 0.15;
const MAX_SPLIT: f32 = 0.85;

pub fn render(shell: &Shell, cx: &Context<Shell>) -> AnyElement {
    let state = shell.state.read(cx);
    let area = div().flex_1().min_h_0().flex().flex_row().bg(theme::editor_bg());
    if state.project_dir.is_none() {
        return area.child(no_project()).into_any_element();
    }
    let panes = state.workspace.panes.len();
    if panes < 2 {
        return area.children((0..panes).map(|pane| render_pane(shell, state, pane, cx))).into_any_element();
    }
    // Two panes side by side with a draggable divider; nothing splits unless asked.
    area.on_drag_move(cx.listener(resize_split))
        .child(div().w(relative(shell.split_ratio)).flex_none().h_full().flex().child(render_pane(shell, state, 0, cx)))
        .child(
            div()
                .id("split-divider")
                .w(px(theme::RESIZE_HANDLE))
                .flex_none()
                .h_full()
                .bg(theme::chrome_bg())
                .border_l_1()
                .border_color(theme::border())
                .cursor_col_resize()
                .on_drag(SplitResize, |_, _, _, cx| cx.new(|_| DragGhost)),
        )
        .child(div().flex_1().min_w_0().h_full().flex().child(render_pane(shell, state, 1, cx)))
        .into_any_element()
}

fn resize_split(shell: &mut Shell, e: &DragMoveEvent<SplitResize>, _: &mut Window, cx: &mut Context<Shell>) {
    let width = f32::from(e.bounds.size.width).max(1.0);
    let x = f32::from(e.event.position.x - e.bounds.origin.x);
    let ratio = (x / width).clamp(MIN_SPLIT, MAX_SPLIT);
    if (ratio - shell.split_ratio).abs() > f32::EPSILON {
        shell.split_ratio = ratio;
        cx.notify();
    }
}

fn no_project() -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .child(div().text_color(theme::text_muted()).child("No project open"))
        .child(
            div()
                .flex()
                .gap(px(10.))
                .child(button("new-project", "New Project…").on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(project::New), cx)
                }))
                .child(button("open-project", "Open Project…").on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(project::Open), cx)
                })),
        )
}

fn render_pane(shell: &Shell, state: &AppState, pane: usize, cx: &Context<Shell>) -> AnyElement {
    let p = &state.workspace.panes[pane];
    let focused = pane == state.workspace.focused_pane;
    let tabs = p.tabs.iter().enumerate().map(|(i, tab)| {
        let active = p.active == Some(i);
        render_tab(shell, state, pane, i, tab, active, focused && active)
    });
    let body = match p.active.and_then(|a| p.tabs.get(a)) {
        Some(TabKind::Pattern(id)) => pattern_tab::render(shell, state, id),
        Some(TabKind::Instrument(id)) => instrument_tab::render(shell, state, id, cx),
        Some(TabKind::PianoRoll(p, i)) => piano_roll::render(shell, state, &(p.clone(), i.clone()), cx),
        None => empty_pane().into_any_element(),
    };
    let focus_state = shell.state.clone();
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        // Capture phase: editors below stop propagation of their own clicks.
        .capture_any_mouse_down(move |e, _, cx| {
            if e.button == MouseButton::Left && !focused {
                focus_state.update(cx, |s, cx| s.update_workspace(cx, |ws| ws.focused_pane = pane));
            }
        })
        .child(
            div()
                .id(("tab-bar", pane))
                .h(px(theme::TAB_BAR_HEIGHT))
                .flex_none()
                .flex()
                .bg(theme::chrome_bg())
                .border_b_1()
                .border_color(theme::border())
                .overflow_x_scroll()
                .children(tabs),
        )
        .child(div().flex_1().min_h_0().child(body))
        .into_any_element()
}

fn render_tab(
    shell: &Shell,
    state: &AppState,
    pane: usize,
    index: usize,
    tab: &TabKind,
    active: bool,
    highlighted: bool,
) -> AnyElement {
    let dirty = state.dirty.tab_is_dirty(tab);
    let activate = shell.state.clone();
    let close = shell.state.clone();
    let id = SharedString::from(format!("tab-{pane}-{index}"));
    div()
        .id(id.clone())
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .h_full()
        .pl(px(14.))
        .pr(px(8.))
        .border_r_1()
        .border_color(theme::border())
        .relative()
        .whitespace_nowrap()
        .cursor_pointer()
        .when(active, |d| d.bg(theme::editor_bg()).text_color(theme::text_strong()))
        .when(!active, |d| d.text_color(theme::text_muted()))
        .when(highlighted, |d| {
            d.child(div().absolute().top_0().left_0().right_0().h(px(2.)).bg(theme::accent()))
        })
        .on_click(move |_, _, cx| {
            activate.update(cx, |s, cx| {
                s.update_workspace(cx, |ws| {
                    ws.focused_pane = pane;
                    ws.panes[pane].active = Some(index);
                })
            })
        })
        .child(tab_title(tab, &state.project))
        .child(if dirty {
            // An unsaved tab shows a dot where the close button would be.
            div().w(px(14.)).flex().justify_center().text_color(theme::text()).child("●").into_any_element()
        } else {
            div()
                .id(SharedString::from(format!("{id}-close")))
                .w(px(14.))
                .flex()
                .justify_center()
                .rounded(px(3.))
                .text_color(theme::text_faint())
                .hover(|s| s.bg(theme::hover_bg()).text_color(theme::text_strong()))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    close.update(cx, |s, cx| s.update_workspace(cx, |ws| ws.close(pane, index)));
                })
                .child("×")
                .into_any_element()
        })
        .into_any_element()
}

fn empty_pane() -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme::text_faint())
        .child("Open a pattern from the sidebar")
}
