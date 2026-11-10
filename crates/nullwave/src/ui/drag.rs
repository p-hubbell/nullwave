//! Drag payloads from the sidebar: sample files and patterns.

use std::path::PathBuf;

use gpui::{Context, Pixels, Point, SharedString, Window, div, prelude::*, px};
use nw_core::PatternId;

use crate::theme;

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

/// A pattern dragged from the sidebar onto the song.
#[derive(Clone)]
pub struct PatternDrag {
    pub id: PatternId,
    pub name: SharedString,
}

/// What follows the cursor while a pattern is dragged.
pub struct PatternDragView {
    name: SharedString,
    grab: Point<Pixels>,
}

impl PatternDragView {
    pub fn new(drag: &PatternDrag, grab: Point<Pixels>) -> PatternDragView {
        PatternDragView { name: drag.name.clone(), grab }
    }
}

impl Render for PatternDragView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
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
                .child(format!("▦ {}", self.name)),
        )
    }
}
