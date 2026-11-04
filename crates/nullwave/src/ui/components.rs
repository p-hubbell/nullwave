//! Small in-house components: rows, headers, buttons, panel tabs and a one-line text field.

use gpui::{
    App, Context, Div, ElementId, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
    SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};

use crate::theme;

const INDENT: f32 = 12.0;

/// An uppercase section title, like VS Code's sidebar sections.
pub fn section_header(label: impl Into<SharedString>) -> Div {
    div()
        .px(px(12.))
        .pt(px(10.))
        .pb(px(4.))
        .text_size(px(theme::SMALL_FONT_SIZE))
        .text_color(theme::text_muted())
        .child(label.into().to_uppercase())
}

/// A clickable list row indented by `depth` levels.
pub fn list_row(id: impl Into<ElementId>, depth: usize, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(22.))
        .pl(px(12. + INDENT * depth as f32))
        .pr(px(8.))
        .whitespace_nowrap()
        .overflow_hidden()
        .cursor_pointer()
        .when(selected, |d| d.bg(theme::selection_bg()).text_color(theme::text_strong()))
        .when(!selected, |d| d.hover(|s| s.bg(theme::hover_bg())))
}

/// A non-interactive, dimmed line (empty states, "not connected", ...).
pub fn dim_line(text: impl Into<SharedString>, depth: usize) -> Div {
    div()
        .h(px(22.))
        .flex()
        .items_center()
        .pl(px(12. + INDENT * depth as f32))
        .pr(px(8.))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_color(theme::text_faint())
        .child(text.into())
}

/// A bordered push button.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(14.))
        .py(px(6.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::border())
        .bg(theme::input_bg())
        .text_color(theme::text_strong())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover_bg()))
        .child(label.into())
}

/// A small uppercase tab label, as used in the bottom panel.
pub fn panel_tab(id: impl Into<ElementId>, label: &str, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h_full()
        .flex()
        .items_center()
        .px(px(4.))
        .text_size(px(theme::SMALL_FONT_SIZE))
        .cursor_pointer()
        .border_b_1()
        .when(active, |d| d.text_color(theme::text_strong()).border_color(theme::accent()))
        .when(!active, |d| {
            d.text_color(theme::text_muted())
                .border_color(theme::transparent())
                .hover(|s| s.text_color(theme::text()))
        })
        .child(label.to_uppercase())
}

pub enum TextFieldEvent {
    Changed,
    Submit,
    Cancel,
    Blur,
}

/// A minimal single-line text field. While focused its key context is `TextInput`,
/// so bindings guarded by `!TextInput` (like Space for play/stop) don't fire.
pub struct TextField {
    focus_handle: FocusHandle,
    text: String,
    placeholder: SharedString,
    accept: fn(char) -> bool,
    _blur: Subscription,
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl TextField {
    pub fn new(
        text: impl Into<String>,
        placeholder: impl Into<SharedString>,
        accept: fn(char) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TextField {
        let focus_handle = cx.focus_handle();
        let blur = cx.on_blur(&focus_handle, window, |_, _, cx| cx.emit(TextFieldEvent::Blur));
        TextField { focus_handle, text: text.into(), placeholder: placeholder.into(), accept, _blur: blur }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let event = match keystroke.key.as_str() {
            "enter" => TextFieldEvent::Submit,
            "escape" => TextFieldEvent::Cancel,
            "backspace" => {
                self.text.pop();
                TextFieldEvent::Changed
            }
            _ => {
                let m = keystroke.modifiers;
                let Some(typed) = keystroke.key_char.as_ref() else { return };
                if m.platform || m.control || m.function {
                    return;
                }
                let accepted: String = typed.chars().filter(|c| (self.accept)(*c)).collect();
                if accepted.is_empty() {
                    cx.stop_propagation();
                    return;
                }
                self.text.push_str(&accepted);
                TextFieldEvent::Changed
            }
        };
        cx.stop_propagation();
        cx.emit(event);
        cx.notify();
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let empty = self.text.is_empty();
        div()
            .key_context("TextInput")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .items_center()
            .h(px(20.))
            .px(px(5.))
            .rounded(px(3.))
            .bg(theme::input_bg())
            .border_1()
            .border_color(if focused { theme::accent() } else { theme::border() })
            .text_color(if empty { theme::text_faint() } else { theme::text_strong() })
            .whitespace_nowrap()
            .overflow_hidden()
            .child(if empty { self.placeholder.to_string() } else { self.text.clone() })
            .when(focused, |d| d.child(div().w(px(1.)).h(px(13.)).bg(theme::accent())))
    }
}
