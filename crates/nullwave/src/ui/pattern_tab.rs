//! Pattern tab (channel rack): one row per instrument with a preview of its notes in this
//! pattern, plus "+ add instrument". Samples dragged from the sidebar drop onto rows.

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, Pixels, Point, SharedString, Window, canvas, div, fill, point, prelude::*,
    px, size,
};
use nw_core::{Instrument, Note, PatternId, Tick};
use nw_workspace::TabKind;

use crate::app_state::{AppState, SampleStatus};
use crate::theme;
use crate::ui::components::Tooltip;
use crate::ui::shell::Shell;

const ROW_HEIGHT: f32 = 36.0;
const PREVIEW_HEIGHT: f32 = 28.0;
const NOTE_BAR_HEIGHT: f32 = 3.0;
const NAME_WIDTH: f32 = 150.0;

/// A sample file dragged from the sidebar.
#[derive(Clone)]
pub struct SampleDrag {
    pub path: PathBuf,
    pub name: SharedString,
}

/// What follows the cursor while a sample is dragged.
pub struct SampleDragView {
    name: SharedString,
    grab: Point<Pixels>,
}

impl SampleDragView {
    pub fn new(drag: &SampleDrag, grab: Point<Pixels>) -> SampleDragView {
        SampleDragView { name: drag.name.clone(), grab }
    }
}

impl Render for SampleDragView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The view is drawn where the drag started; offset it so it sits at the cursor.
        div().pl(self.grab.x + px(10.)).pt(self.grab.y - px(10.)).child(
            div()
                .px(px(8.))
                .py(px(3.))
                .rounded(px(4.))
                .bg(theme::selection_bg())
                .border_1()
                .border_color(theme::accent())
                .text_color(theme::text_strong())
                .text_size(px(theme::FONT_SIZE))
                .whitespace_nowrap()
                .child(format!("♪ {}", self.name)),
        )
    }
}

pub fn render(shell: &Shell, state: &AppState, pattern_id: &PatternId) -> AnyElement {
    let Some(pattern) = state.project.pattern(pattern_id) else {
        return div().into_any_element();
    };
    let length = state.project.pattern_length(pattern_id).max(1);
    let rows = state.project.instruments.iter().enumerate().map(|(i, inst)| {
        let notes = pattern.notes.get(&inst.id).map_or(&[][..], Vec::as_slice);
        rack_row(shell, state, pattern_id, i, inst, notes, length)
    });
    div()
        .id("rack")
        .size_full()
        .overflow_y_scroll()
        .p(px(14.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .children(rows)
        .child(add_row(shell))
        .into_any_element()
}

fn rack_row(
    shell: &Shell,
    state: &AppState,
    pattern_id: &PatternId,
    index: usize,
    inst: &Instrument,
    notes: &[Note],
    length: Tick,
) -> AnyElement {
    let roll = TabKind::PianoRoll(pattern_id.clone(), inst.id.clone());
    let settings = TabKind::Instrument(inst.id.clone());
    let open_roll = shell.state.clone();
    let open_settings = shell.state.clone();
    let drop_state = shell.state.clone();
    let drop_id = inst.id.clone();
    let missing = match state.sample_status(&inst.id) {
        SampleStatus::Missing(e) => Some(e),
        _ => None,
    };

    let name = div()
        .id(("rack-name", index))
        .w(px(NAME_WIDTH))
        .flex_none()
        .h(px(26.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(theme::input_bg())
        .border_1()
        .border_color(theme::border())
        .text_color(theme::text_strong())
        .whitespace_nowrap()
        .overflow_hidden()
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover_bg()))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            open_settings.update(cx, |s, cx| s.open_tab(settings.clone(), cx));
        })
        .child(inst.name.clone());

    div()
        .id(("rack-row", index))
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(6.))
        .rounded(px(4.))
        .hover(|s| s.bg(theme::hover_bg()))
        .drag_over::<SampleDrag>(|s, _, _, _| s.bg(theme::selection_bg()))
        .on_drop(move |drag: &SampleDrag, _, cx| {
            drop_state.update(cx, |s, cx| s.set_instrument_sample(&drop_id, drag.path.clone(), cx));
        })
        .on_click(move |event, _, cx| {
            if event.click_count() == 2 {
                open_roll.update(cx, |s, cx| s.open_tab(roll.clone(), cx));
            }
        })
        .child(name)
        .child(preview(notes, length))
        .when_some(missing, |d, error| {
            d.child(
                div()
                    .id(("rack-missing", index))
                    .flex_none()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(theme::warning())
                    .text_color(theme::warning())
                    .text_size(px(theme::SMALL_FONT_SIZE))
                    .child("missing sample")
                    .tooltip(Tooltip::text(error)),
            )
        })
        .into_any_element()
}

/// A strip with each note as a thin bar: time across `length`, pitch across the range used.
fn preview(notes: &[Note], length: Tick) -> AnyElement {
    let strip = div().flex_1().min_w_0().h(px(PREVIEW_HEIGHT)).rounded(px(3.)).border_1();
    if notes.is_empty() {
        return strip
            .border_dashed()
            .border_color(theme::border())
            .flex()
            .items_center()
            .px(px(10.))
            .text_color(theme::text_faint())
            .text_size(px(theme::SMALL_FONT_SIZE))
            .child("empty — double-click to add notes")
            .into_any_element();
    }
    let bars: Vec<(Tick, Tick, u8)> = notes.iter().map(|n| (n.start, n.length, n.pitch)).collect();
    let low = bars.iter().map(|b| b.2).min().unwrap_or(0);
    let high = bars.iter().map(|b| b.2).max().unwrap_or(0);
    strip
        .border_color(theme::border())
        .bg(theme::chrome_bg())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let width = f32::from(bounds.size.width);
                    let pad = 4.0;
                    let usable = (f32::from(bounds.size.height) - 2.0 * pad - NOTE_BAR_HEIGHT).max(0.0);
                    let span = f32::from(high - low);
                    for &(start, len, pitch) in &bars {
                        let x = width * start as f32 / length as f32;
                        let w = (width * len as f32 / length as f32).max(2.0).min(width - x);
                        let y = if span == 0.0 {
                            usable / 2.0
                        } else {
                            usable * f32::from(high - pitch) / span
                        };
                        let origin = bounds.origin + point(px(x), px(pad + y));
                        let rect = gpui::Bounds::new(origin, size(px(w.max(0.0)), px(NOTE_BAR_HEIGHT)));
                        window.paint_quad(fill(rect, theme::accent()));
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

fn add_row(shell: &Shell) -> AnyElement {
    let pick = shell.state.clone();
    let drop_state = shell.state.clone();
    div()
        .id("add-instrument")
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .px(px(16.))
        .rounded(px(4.))
        .text_color(theme::text_muted())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover_bg()).text_color(theme::text()))
        .drag_over::<SampleDrag>(|s, _, _, _| s.bg(theme::selection_bg()).text_color(theme::text_strong()))
        .on_click(move |_, _, cx| pick.update(cx, |s, cx| s.prompt_add_instruments(cx)))
        .on_drop(move |drag: &SampleDrag, _, cx| {
            drop_state.update(cx, |s, cx| s.add_instrument_from_sample(drag.path.clone(), cx));
        })
        .child("+ add instrument")
        .into_any_element()
}
