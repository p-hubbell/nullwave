//! Editor area: one tab bar per pane and the active editor below it. Without a project it
//! shows the New / Open buttons instead.

use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use nw_workspace::{TabKind, tab_title};

use crate::actions::project;
use crate::app_state::AppState;
use crate::theme;
use crate::ui::components::button;
use crate::ui::{instrument_tab, pattern_tab};
use crate::ui::shell::Shell;

pub fn render(shell: &Shell, cx: &Context<Shell>) -> AnyElement {
    let state = shell.state.read(cx);
    let area = div().flex_1().min_h_0().flex().flex_row().bg(theme::editor_bg());
    if state.project_dir.is_none() {
        return area.child(no_project()).into_any_element();
    }
    area.children((0..state.workspace.panes.len()).map(|pane| render_pane(shell, state, pane, cx)))
        .into_any_element()
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
        Some(tab) => placeholder_editor(state, tab),
        None => empty_pane().into_any_element(),
    };
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .when(pane > 0, |d| d.border_l_1().border_color(theme::border()))
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

/// Stand-in for the piano roll until it exists.
fn placeholder_editor(state: &AppState, tab: &TabKind) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .child(div().text_color(theme::text_strong()).child(tab_title(tab, &state.project)))
        .child(div().text_color(theme::text_faint()).child("Piano roll editor — coming soon"))
        .into_any_element()
}
