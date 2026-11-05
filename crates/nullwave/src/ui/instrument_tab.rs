//! Instrument tab (`Sampler: <name>`): sample, root note, attack, release, volume and pan.
//! A slider drag shows its value live and commits one `UpdateInstrument` when released.

use gpui::{
    AnyElement, Bounds, Context, DispatchPhase, Entity, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, canvas, div, prelude::*, px,
    relative,
};
use nw_core::{Instrument, InstrumentId};

use crate::app_state::{AppState, SampleStatus};
use crate::theme;
use crate::ui::components::{Tooltip, button};
use crate::ui::pattern_tab::SampleDrag;
use crate::ui::shell::Shell;

const LABEL_WIDTH: f32 = 110.0;
const VALUE_WIDTH: f32 = 80.0;
const MAX_ATTACK_MS: f32 = 2000.0;
const MAX_RELEASE_MS: f32 = 5000.0;
/// Pan values this close to the middle snap to centre.
const PAN_SNAP: f32 = 0.03;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Attack,
    Release,
    Volume,
    Pan,
}

impl Field {
    fn get(self, inst: &Instrument) -> f32 {
        match self {
            Field::Attack => inst.sampler.volume_env.attack_ms,
            Field::Release => inst.sampler.volume_env.release_ms,
            Field::Volume => inst.volume,
            Field::Pan => inst.pan,
        }
    }

    fn set(self, inst: &mut Instrument, value: f32) {
        match self {
            Field::Attack => inst.sampler.volume_env.attack_ms = value,
            Field::Release => inst.sampler.volume_env.release_ms = value,
            Field::Volume => inst.volume = value,
            Field::Pan => inst.pan = value,
        }
    }

    /// Track position (0..=1) to value. Times use a square curve so short times get more room.
    fn value_at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Field::Attack => (MAX_ATTACK_MS * t * t).round(),
            Field::Release => (MAX_RELEASE_MS * t * t).round(),
            Field::Volume => (t * 100.0).round() / 100.0,
            Field::Pan => {
                let pan = ((t * 2.0 - 1.0) * 100.0).round() / 100.0;
                if pan.abs() < PAN_SNAP { 0.0 } else { pan }
            }
        }
    }

    fn fraction_of(self, value: f32) -> f32 {
        let t = match self {
            Field::Attack => (value / MAX_ATTACK_MS).max(0.0).sqrt(),
            Field::Release => (value / MAX_RELEASE_MS).max(0.0).sqrt(),
            Field::Volume => value,
            Field::Pan => (value + 1.0) / 2.0,
        };
        if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
    }

    fn format(self, value: f32) -> String {
        match self {
            Field::Attack | Field::Release => format!("{value:.0} ms"),
            Field::Volume => format!("{:.0}%", value * 100.0),
            Field::Pan => format_pan(value),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Field::Attack => "Attack",
            Field::Release => "Release",
            Field::Volume => "Volume",
            Field::Pan => "Pan",
        }
    }
}

/// `L100`, `C`, `R35`.
pub fn format_pan(pan: f32) -> String {
    let amount = (pan.abs() * 100.0).round();
    if amount == 0.0 {
        "C".to_string()
    } else if pan < 0.0 {
        format!("L{amount:.0}")
    } else {
        format!("R{amount:.0}")
    }
}

/// `C4` for 60 (scientific pitch notation).
pub fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = i32::from(pitch / 12) - 1;
    format!("{}{octave}", NAMES[usize::from(pitch % 12)])
}

/// A slider drag in progress: the instrument as it was, and as it will be on release.
pub struct SliderDrag {
    id: InstrumentId,
    field: Field,
    before: Instrument,
    pending: Instrument,
}

impl Shell {
    fn begin_slider(&mut self, id: &InstrumentId, field: Field, t: f32, cx: &mut Context<Shell>) {
        let Some(before) = self.state.read(cx).project.instrument(id).cloned() else { return };
        let mut pending = before.clone();
        field.set(&mut pending, field.value_at(t));
        self.slider = Some(SliderDrag { id: id.clone(), field, before, pending });
        cx.notify();
    }

    fn move_slider(&mut self, id: &InstrumentId, field: Field, t: f32, cx: &mut Context<Shell>) {
        let Some(drag) = &mut self.slider else { return };
        if &drag.id != id || drag.field != field {
            return;
        }
        let value = field.value_at(t);
        if field.get(&drag.pending) != value {
            field.set(&mut drag.pending, value);
            cx.notify();
        }
    }

    fn end_slider(&mut self, id: &InstrumentId, field: Field, cx: &mut Context<Shell>) {
        if !self.slider.as_ref().is_some_and(|d| &d.id == id && d.field == field) {
            return;
        }
        let Some(drag) = self.slider.take() else { return };
        self.state.update(cx, |s, cx| s.update_instrument(drag.before, drag.pending, cx));
        cx.notify();
    }
}

pub fn render(shell: &Shell, state: &AppState, id: &InstrumentId, cx: &Context<Shell>) -> AnyElement {
    let Some(stored) = state.project.instrument(id) else {
        return div().into_any_element();
    };
    // While a slider is dragged, show the value it will commit.
    let inst = match &shell.slider {
        Some(drag) if &drag.id == id => &drag.pending,
        _ => stored,
    };
    let shell_entity = cx.entity();

    let delete_state = shell.state.clone();
    let delete_id = id.clone();
    div()
        .id("instrument-tab")
        .size_full()
        .overflow_y_scroll()
        .p(px(20.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .pb(px(6.))
                .text_size(px(15.))
                .text_color(theme::text_strong())
                .child(format!("Sampler: {}", inst.name)),
        )
        .child(sample_row(shell, state, inst))
        .child(root_row(shell, stored))
        .children(
            [Field::Attack, Field::Release, Field::Volume, Field::Pan]
                .into_iter()
                .map(|field| slider_row(&shell_entity, id, field, inst)),
        )
        .child(
            div().pt(px(16.)).flex().child(
                button("delete-instrument", "Delete instrument")
                    .text_color(theme::warning())
                    .on_click(move |_, _, cx| {
                        delete_state.update(cx, |s, cx| s.delete_instrument(&delete_id, cx));
                    }),
            ),
        )
        .into_any_element()
}

fn row(label: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(28.))
        .child(div().w(px(LABEL_WIDTH)).flex_none().text_color(theme::text_muted()).child(label))
}

fn sample_row(shell: &Shell, state: &AppState, inst: &Instrument) -> AnyElement {
    let (text, tooltip, missing): (String, Option<String>, bool) = match (&inst.sampler.sample_path, state.sample_status(&inst.id)) {
        (None, _) | (_, SampleStatus::NoSample) => ("No sample".into(), None, false),
        (Some(path), SampleStatus::Ok) => {
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into());
            (name, Some(path.display().to_string()), false)
        }
        (Some(path), SampleStatus::Missing(e)) => {
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into());
            (format!("{name} — missing"), Some(e), true)
        }
    };
    let change_state = shell.state.clone();
    let change_id = inst.id.clone();
    let drop_state = shell.state.clone();
    let drop_id = inst.id.clone();
    let label = div()
        .id("sample-path")
        .flex_1()
        .min_w_0()
        .px(px(8.))
        .py(px(4.))
        .rounded(px(4.))
        .bg(theme::input_bg())
        .border_1()
        .border_color(if missing { theme::warning() } else { theme::border() })
        .text_color(if missing { theme::warning() } else { theme::text_strong() })
        .whitespace_nowrap()
        .overflow_hidden()
        .drag_over::<SampleDrag>(|s, _, _, _| s.border_color(theme::accent()).bg(theme::selection_bg()))
        .on_drop(move |drag: &SampleDrag, _, cx| {
            drop_state.update(cx, |s, cx| s.set_instrument_sample(&drop_id, drag.path.clone(), cx));
        })
        .child(text)
        .when_some(tooltip, |d, t| d.tooltip(Tooltip::text(t)));
    row("Sample")
        .child(label)
        .child(
            button("change-sample", "Change…")
                .py(px(3.))
                .on_click(move |_, _, cx| change_state.update(cx, |s, cx| s.prompt_change_sample(&change_id, cx))),
        )
        .into_any_element()
}

fn root_row(shell: &Shell, inst: &Instrument) -> AnyElement {
    let step = |delta: i16, label: &'static str, id: &'static str| {
        let state = shell.state.clone();
        let inst_id = inst.id.clone();
        let disabled = (delta < 0 && inst.sampler.root_note == 0) || (delta > 0 && inst.sampler.root_note == 127);
        button(id, label)
            .px(px(8.))
            .py(px(1.))
            .when(disabled, |d| d.text_color(theme::text_faint()))
            .on_click(move |_, _, cx| {
                // Read the instrument now, not at render time, so quick clicks all count.
                state.update(cx, |s, cx| {
                    let Some(before) = s.project.instrument(&inst_id).cloned() else { return };
                    let mut after = before.clone();
                    after.sampler.root_note = (i16::from(before.sampler.root_note) + delta).clamp(0, 127) as u8;
                    s.update_instrument(before, after, cx);
                });
            })
    };
    let root = inst.sampler.root_note;
    row("Root note")
        .child(step(-1, "−", "root-down"))
        .child(
            div()
                .w(px(VALUE_WIDTH))
                .flex()
                .justify_center()
                .text_color(theme::text_strong())
                .child(format!("{} ({root})", note_name(root))),
        )
        .child(step(1, "+", "root-up"))
        .into_any_element()
}

fn slider_row(shell: &Entity<Shell>, id: &InstrumentId, field: Field, inst: &Instrument) -> AnyElement {
    let value = field.get(inst);
    let t = field.fraction_of(value);
    let track = div()
        .relative()
        .flex_1()
        .max_w(px(360.))
        .h(px(18.))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .w_full()
                .h(px(4.))
                .rounded(px(2.))
                .bg(theme::border())
                .child(div().h_full().w(relative(t)).rounded(px(2.)).bg(theme::accent())),
        )
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(relative(t))
                .ml(px(-6.))
                .size(px(12.))
                .rounded_full()
                .bg(theme::text_strong()),
        )
        .child(slider_listeners(shell.clone(), id.clone(), field).absolute().inset_0());
    row(field.label())
        .child(track)
        .child(div().w(px(VALUE_WIDTH)).text_color(theme::text_strong()).child(field.format(value)))
        .into_any_element()
}

fn fraction(bounds: Bounds<Pixels>, position: Point<Pixels>) -> f32 {
    let width = f32::from(bounds.size.width).max(1.0);
    (f32::from(position.x - bounds.origin.x) / width).clamp(0.0, 1.0)
}

/// An invisible layer over the track that drives the drag. Move and up listen window-wide,
/// so the drag keeps tracking (and still commits) when the cursor leaves the track.
fn slider_listeners(shell: Entity<Shell>, id: InstrumentId, field: Field) -> gpui::Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let (down_shell, down_id) = (shell.clone(), id.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !bounds.contains(&e.position) {
                    return;
                }
                let t = fraction(bounds, e.position);
                down_shell.update(cx, |sh, cx| sh.begin_slider(&down_id, field, t, cx));
                cx.stop_propagation();
            });
            let (move_shell, move_id) = (shell.clone(), id.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble || e.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let t = fraction(bounds, e.position);
                move_shell.update(cx, |sh, cx| sh.move_slider(&move_id, field, t, cx));
            });
            let (up_shell, up_id) = (shell.clone(), id.clone());
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    up_shell.update(cx, |sh, cx| sh.end_slider(&up_id, field, cx));
                }
            });
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(0), "C-1");
        assert_eq!(note_name(61), "C#4");
        assert_eq!(note_name(127), "G9");
    }

    #[test]
    fn pan_formatting() {
        assert_eq!(format_pan(0.0), "C");
        assert_eq!(format_pan(-1.0), "L100");
        assert_eq!(format_pan(0.35), "R35");
    }

    #[test]
    fn fractions_round_trip_and_clamp() {
        for field in [Field::Attack, Field::Release, Field::Volume, Field::Pan] {
            assert_eq!(field.value_at(0.0), field.value_at(-1.0));
            assert_eq!(field.value_at(1.0), field.value_at(2.0));
            let v = field.value_at(1.0);
            assert!((field.fraction_of(v) - 1.0).abs() < 1e-4, "{field:?}");
        }
        assert_eq!(Field::Attack.value_at(1.0), MAX_ATTACK_MS);
        assert_eq!(Field::Release.value_at(1.0), MAX_RELEASE_MS);
        assert_eq!(Field::Pan.value_at(0.5), 0.0);
        assert_eq!(Field::Pan.value_at(0.51), 0.0, "snaps to centre");
        assert_eq!(Field::Volume.value_at(0.5), 0.5);
    }
}
