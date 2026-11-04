//! Bottom panel with the tabs *Editor* (follows the active editor; empty for now) and *Log*.

use gpui::{Context, div, prelude::*, px};
use nw_workspace::PanelTab;

use crate::theme;
use crate::ui::components::panel_tab;
use crate::ui::shell::Shell;

pub fn render(shell: &Shell, height: f32, cx: &Context<Shell>) -> impl IntoElement {
    let state = shell.state.read(cx);
    let current = state.workspace.panel_tab;
    let tab = |tab: PanelTab, label: &str, id: &'static str| {
        let state = shell.state.clone();
        panel_tab(id, label, tab == current).on_click(move |_, _, cx| {
            state.update(cx, |s, cx| s.update_workspace(cx, |ws| ws.panel_tab = tab));
        })
    };

    let body = match current {
        PanelTab::Editor => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme::text_faint())
            .child("Nothing to show for this editor")
            .into_any_element(),
        PanelTab::Log => div()
            .id("log")
            .size_full()
            .px(px(12.))
            .py(px(6.))
            .overflow_y_scroll()
            .track_scroll(&shell.log_scroll)
            .font_family("Menlo")
            .text_size(px(11.))
            .children(state.log.iter().map(|line| div().child(line.clone())))
            .into_any_element(),
    };

    div()
        .h(px(height))
        .flex_none()
        .flex()
        .flex_col()
        .bg(theme::sidebar_bg())
        .child(
            div()
                .h(px(theme::PANEL_TABS_HEIGHT))
                .flex_none()
                .flex()
                .gap(px(14.))
                .px(px(10.))
                .border_b_1()
                .border_color(theme::border())
                .child(tab(PanelTab::Editor, "Editor", "panel-editor"))
                .child(tab(PanelTab::Log, "Log", "panel-log")),
        )
        .child(div().flex_1().min_h_0().child(body))
}
