//! Instrument tab (`Sampler: <name>`): volume and pan, then the sampler's sections —
//! Sample (waveform with trim and loop markers), Pitch, Envelopes (graph + sliders),
//! Filter, LFO, Velocity and Voice.
//!
//! Every drag (slider, waveform marker, envelope point) edits a pending copy of the
//! instrument that the tab shows live, and commits one `UpdateInstrument` on release.
//! Buttons, toggles and steppers commit at once. Each change is one undo step.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Entity, Hitbox, HitboxBehavior,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString,
    Window, canvas, div, point, prelude::*, px, relative,
};
use nw_core::{
    Envelope, FilterKind, Instrument, InstrumentId, Lfo, LfoRate, LfoShape, LoopMode, PPQ,
    Sampler, limits,
};

use crate::app_state::{AppState, SampleStatus};
use crate::theme;
use crate::ui::components::{Tooltip, button, section_header};
use crate::ui::envelope_graph::{self, EnvPoint};
use crate::ui::param::{self, ParamSpec};
use crate::ui::pattern_tab::SampleDrag;
use crate::ui::shell::Shell;
use crate::ui::waveform::{self, Marker, keep_loop_in_region, moved_marker};

const LABEL_WIDTH: f32 = 110.0;
const VALUE_WIDTH: f32 = 84.0;
const CONTROL_WIDTH: f32 = 360.0;
const WAVEFORM_HEIGHT: f32 = 96.0;
const GRAPH_HEIGHT: f32 = 110.0;
/// How close (px) a click must be to grab a marker or envelope point.
const GRAB_PX: f32 = 8.0;

/// `C4` for 60 (scientific pitch notation).
pub fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = i32::from(pitch / 12) - 1;
    format!("{}{octave}", NAMES[usize::from(pitch % 12)])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnvTarget {
    #[default]
    Volume,
    Cutoff,
    Pitch,
    Pan,
}

impl EnvTarget {
    const ALL: [EnvTarget; 4] = [EnvTarget::Volume, EnvTarget::Cutoff, EnvTarget::Pitch, EnvTarget::Pan];

    fn label(self) -> &'static str {
        match self {
            EnvTarget::Volume => "Volume",
            EnvTarget::Cutoff => "Cutoff",
            EnvTarget::Pitch => "Pitch",
            EnvTarget::Pan => "Pan",
        }
    }

    fn specs(self) -> &'static [ParamSpec; 8] {
        match self {
            EnvTarget::Volume => &param::VOLUME_ENV,
            EnvTarget::Cutoff => &param::CUTOFF_ENV,
            EnvTarget::Pitch => &param::PITCH_ENV,
            EnvTarget::Pan => &param::PAN_ENV,
        }
    }

    fn amount(self) -> Option<&'static ParamSpec> {
        match self {
            EnvTarget::Volume => None,
            EnvTarget::Cutoff => Some(&param::CUTOFF_ENV_AMOUNT),
            EnvTarget::Pitch => Some(&param::PITCH_ENV_AMOUNT),
            EnvTarget::Pan => Some(&param::PAN_ENV_AMOUNT),
        }
    }

    fn env(self, s: &Sampler) -> Envelope {
        match self {
            EnvTarget::Volume => s.volume_env,
            EnvTarget::Cutoff => s.cutoff_env.env,
            EnvTarget::Pitch => s.pitch_env.env,
            EnvTarget::Pan => s.pan_env.env,
        }
    }

    fn env_mut(self, s: &mut Sampler) -> &mut Envelope {
        match self {
            EnvTarget::Volume => &mut s.volume_env,
            EnvTarget::Cutoff => &mut s.cutoff_env.env,
            EnvTarget::Pitch => &mut s.pitch_env.env,
            EnvTarget::Pan => &mut s.pan_env.env,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LfoTarget {
    #[default]
    Volume,
    Pan,
    Cutoff,
    Pitch,
}

impl LfoTarget {
    const ALL: [LfoTarget; 4] = [LfoTarget::Volume, LfoTarget::Pan, LfoTarget::Cutoff, LfoTarget::Pitch];

    fn label(self) -> &'static str {
        match self {
            LfoTarget::Volume => "Volume",
            LfoTarget::Pan => "Pan",
            LfoTarget::Cutoff => "Cutoff",
            LfoTarget::Pitch => "Pitch",
        }
    }

    fn specs(self) -> &'static [ParamSpec; 2] {
        match self {
            LfoTarget::Volume => &param::VOLUME_LFO,
            LfoTarget::Pan => &param::PAN_LFO,
            LfoTarget::Cutoff => &param::CUTOFF_LFO,
            LfoTarget::Pitch => &param::PITCH_LFO,
        }
    }

    fn hz_spec(self) -> &'static ParamSpec {
        match self {
            LfoTarget::Volume => &param::VOLUME_LFO_HZ,
            LfoTarget::Pan => &param::PAN_LFO_HZ,
            LfoTarget::Cutoff => &param::CUTOFF_LFO_HZ,
            LfoTarget::Pitch => &param::PITCH_LFO_HZ,
        }
    }

    fn lfo(self, s: &Sampler) -> Lfo {
        match self {
            LfoTarget::Volume => s.volume_lfo,
            LfoTarget::Pan => s.pan_lfo,
            LfoTarget::Cutoff => s.cutoff_lfo,
            LfoTarget::Pitch => s.pitch_lfo,
        }
    }

    fn lfo_mut(self, s: &mut Sampler) -> &mut Lfo {
        match self {
            LfoTarget::Volume => &mut s.volume_lfo,
            LfoTarget::Pan => &mut s.pan_lfo,
            LfoTarget::Cutoff => &mut s.cutoff_lfo,
            LfoTarget::Pitch => &mut s.pitch_lfo,
        }
    }
}

/// Which envelope and LFO each instrument tab is showing (not saved).
#[derive(Clone, Copy, Debug, Default)]
pub struct InstView {
    pub env: EnvTarget,
    pub lfo: LfoTarget,
}

#[derive(Clone, Copy)]
enum DragSource {
    Param(&'static ParamSpec),
    Marker(Marker),
    EnvPoint(EnvTarget, EnvPoint),
}

/// A drag in progress on an instrument tab.
pub struct InstDrag {
    id: InstrumentId,
    source: DragSource,
    before: Instrument,
    pending: Instrument,
    origin: Point<f32>,
    width: f32,
    /// Envelope drags: milliseconds per pixel and the graph's inner height, fixed at the
    /// start so the scale does not shift under the cursor.
    ms_per_px: f32,
    level_height: f32,
}

/// What a drag layer does on mouse-down.
#[derive(Clone, Copy)]
enum Layer {
    Slider(&'static ParamSpec),
    Waveform,
    EnvGraph(EnvTarget),
}

impl Shell {
    fn inst_drag_down(&mut self, id: &InstrumentId, layer: Layer, pos: Point<f32>, size: (f32, f32), cx: &mut Context<Shell>) {
        let Some(before) = self.state.read(cx).project.instrument(id).cloned() else { return };
        let (width, height) = size;
        let mut drag = InstDrag {
            id: id.clone(),
            source: DragSource::Param(&param::VOLUME),
            pending: before.clone(),
            before,
            origin: pos,
            width,
            ms_per_px: 0.0,
            level_height: 1.0,
        };
        match layer {
            Layer::Slider(spec) => {
                drag.source = DragSource::Param(spec);
                // A click jumps the value to the click position.
                (spec.set)(&mut drag.pending, spec.value_at(pos.x / width.max(1.0)));
            }
            Layer::Waveform => {
                let tolerance = GRAB_PX / width.max(1.0);
                let Some(marker) = waveform::marker_at(&drag.before.sampler, pos.x / width.max(1.0), tolerance) else {
                    return;
                };
                drag.source = DragSource::Marker(marker);
            }
            Layer::EnvGraph(target) => {
                let env = target.env(&drag.before.sampler);
                let Some(p) = envelope_graph::point_at(&env, width, height, pos.x, pos.y, GRAB_PX + 2.0) else {
                    return;
                };
                drag.source = DragSource::EnvPoint(target, p);
                drag.ms_per_px = 1.0 / envelope_graph::layout(&env, width).px_per_ms;
                drag.level_height = (height - 2.0 * envelope_graph::PAD).max(1.0);
            }
        }
        self.inst_drag = Some(drag);
        cx.notify();
    }

    fn inst_drag_move(&mut self, id: &InstrumentId, pos: Point<f32>, cx: &mut Context<Shell>) {
        let Some(d) = self.inst_drag.as_mut().filter(|d| &d.id == id) else { return };
        let mut next = d.before.clone();
        match d.source {
            DragSource::Param(spec) => (spec.set)(&mut next, spec.value_at(pos.x / d.width.max(1.0))),
            DragSource::Marker(m) => next.sampler = moved_marker(&d.before.sampler, m, pos.x / d.width.max(1.0)),
            DragSource::EnvPoint(target, p) => {
                let d_ms = (pos.x - d.origin.x) * d.ms_per_px;
                let d_level = -(pos.y - d.origin.y) / d.level_height;
                *target.env_mut(&mut next.sampler) =
                    envelope_graph::drag_env_point(&target.env(&d.before.sampler), p, d_ms, d_level);
            }
        }
        if next != d.pending {
            d.pending = next;
            cx.notify();
        }
    }

    fn inst_drag_up(&mut self, id: &InstrumentId, cx: &mut Context<Shell>) {
        if !self.inst_drag.as_ref().is_some_and(|d| &d.id == id) {
            return;
        }
        let Some(d) = self.inst_drag.take() else { return };
        self.state.update(cx, |s, cx| s.update_instrument(d.before, d.pending, cx));
        cx.notify();
    }

    fn inst_view(&self, id: &InstrumentId) -> InstView {
        self.inst_views.get(id).copied().unwrap_or_default()
    }
}

/// Applies `f` to the current instrument and commits it as one undo step.
fn edit(state: &Entity<AppState>, id: &InstrumentId, f: impl FnOnce(&mut Instrument), cx: &mut App) {
    state.update(cx, |s, cx| {
        let Some(before) = s.project.instrument(id).cloned() else { return };
        let mut after = before.clone();
        f(&mut after);
        s.update_instrument(before, after, cx);
    });
}

type Change = Rc<dyn Fn(&mut Instrument)>;

fn change(f: impl Fn(&mut Instrument) + 'static) -> Change {
    Rc::new(f)
}

pub fn render(shell: &Shell, state: &AppState, id: &InstrumentId, cx: &Context<Shell>) -> AnyElement {
    let Some(stored) = state.project.instrument(id) else {
        return div().into_any_element();
    };
    // While something is dragged, show the value it will commit.
    let inst = match &shell.inst_drag {
        Some(d) if &d.id == id => &d.pending,
        _ => stored,
    };
    let ctx = Ctx { shell: cx.entity(), state: shell.state.clone(), id: id.clone() };
    let view = shell.inst_view(id);
    let s = &inst.sampler;

    let audition_state = shell.state.clone();
    let audition_id = id.clone();
    let root = s.root_note;
    let delete_state = shell.state.clone();
    let delete_id = id.clone();

    div()
        .id("instrument-tab")
        .size_full()
        .overflow_y_scroll()
        .p(px(20.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .pb(px(6.))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().text_size(px(15.)).text_color(theme::text_strong()).child(format!("Sampler: {}", inst.name)))
                .child(
                    button("audition", "▶")
                        .px(px(8.))
                        .py(px(1.))
                        .tooltip(Tooltip::text("Play the root note"))
                        .on_click(move |_, _, cx| {
                            audition_state.update(cx, |s, _| s.preview_note(&audition_id, root, 100, PPQ));
                        }),
                ),
        )
        .child(slider_row(&ctx, &param::VOLUME, inst, false))
        .child(slider_row(&ctx, &param::PAN, inst, false))
        .child(section_header("sample"))
        .child(sample_row(shell, state, inst))
        .child(waveform_view(&ctx, state, inst))
        .child(processing_row(&ctx, s))
        .child(loop_row(&ctx, inst))
        .child(section_header("pitch"))
        .child(stepper_row(
            &ctx,
            "Root note",
            "root",
            format!("{} ({})", note_name(s.root_note), s.root_note),
            change(|i| i.sampler.root_note = i.sampler.root_note.saturating_sub(1)),
            change(|i| i.sampler.root_note = (i.sampler.root_note + 1).min(127)),
        ))
        .child(stepper_row(
            &ctx,
            "Coarse tune",
            "coarse",
            format!("{:+} st", s.tune.coarse),
            change(|i| i.sampler.tune.coarse = (i.sampler.tune.coarse - 1).max(*limits::COARSE.start())),
            change(|i| i.sampler.tune.coarse = (i.sampler.tune.coarse + 1).min(*limits::COARSE.end())),
        ))
        .child(slider_row(&ctx, &param::FINE_TUNE, inst, false))
        .child(section_header("envelopes"))
        .child(envelope_section(&ctx, inst, view.env))
        .child(section_header("filter"))
        .child(filter_section(&ctx, inst))
        .child(section_header("lfo"))
        .child(lfo_section(&ctx, inst, view.lfo))
        .child(section_header("velocity"))
        .child(slider_row(&ctx, &param::VEL_TO_VOLUME, inst, false))
        .child(slider_row(&ctx, &param::VEL_TO_CUTOFF, inst, false))
        .child(section_header("voice"))
        .child(voice_section(&ctx, inst))
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

/// What every control needs to commit changes.
#[derive(Clone)]
struct Ctx {
    shell: Entity<Shell>,
    state: Entity<AppState>,
    id: InstrumentId,
}

fn row(label: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(26.))
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
        .max_w(px(CONTROL_WIDTH))
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
    row("File")
        .child(label)
        .child(
            button("change-sample", "Change…")
                .py(px(3.))
                .on_click(move |_, _, cx| change_state.update(cx, |s, cx| s.prompt_change_sample(&change_id, cx))),
        )
        .into_any_element()
}

fn local(bounds: Bounds<Pixels>, p: Point<Pixels>) -> Point<f32> {
    point(f32::from(p.x - bounds.origin.x), f32::from(p.y - bounds.origin.y))
}

/// Registers the mouse listeners of a drag layer covering `bounds`. Move and up listen
/// window-wide, so a drag keeps tracking (and still commits) outside the control.
fn register_drag(bounds: Bounds<Pixels>, hitbox: Hitbox, ctx: &Ctx, layer: Layer, window: &mut Window) {
    let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let (shell, id) = (ctx.shell.clone(), ctx.id.clone());
    window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !hitbox.is_hovered(window) {
            return;
        }
        let pos = local(bounds, e.position);
        shell.update(cx, |sh, cx| sh.inst_drag_down(&id, layer, pos, size, cx));
        cx.stop_propagation();
    });
    let (shell, id) = (ctx.shell.clone(), ctx.id.clone());
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && e.pressed_button == Some(MouseButton::Left) {
            let pos = local(bounds, e.position);
            shell.update(cx, |sh, cx| sh.inst_drag_move(&id, pos, cx));
        }
    });
    let (shell, id) = (ctx.shell.clone(), ctx.id.clone());
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            shell.update(cx, |sh, cx| sh.inst_drag_up(&id, cx));
        }
    });
}

fn slider_row(ctx: &Ctx, spec: &'static ParamSpec, inst: &Instrument, dimmed: bool) -> AnyElement {
    let value = (spec.get)(inst);
    let t = spec.fraction_of(value);
    let layer_ctx = ctx.clone();
    let track = div()
        .id(spec.key)
        .relative()
        .flex_1()
        .max_w(px(CONTROL_WIDTH))
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
                .child(div().h_full().w(relative(t)).rounded(px(2.)).bg(if dimmed { theme::text_faint() } else { theme::accent() })),
        )
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(relative(t))
                .ml(px(-6.))
                .size(px(12.))
                .rounded_full()
                .bg(if dimmed { theme::text_muted() } else { theme::text_strong() }),
        )
        .child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, _| register_drag(bounds, hitbox, &layer_ctx, Layer::Slider(spec), window),
            )
            .absolute()
            .inset_0(),
        );
    row(spec.label)
        .when(dimmed, |d| d.opacity(0.5))
        .child(track)
        .child(div().w(px(VALUE_WIDTH)).text_color(theme::text_strong()).child((spec.format)(value)))
        .into_any_element()
}

fn small_button(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    button(id, label).px(px(8.)).py(px(1.))
}

fn stepper_row(ctx: &Ctx, label: &'static str, key: &'static str, value: String, dec: Change, inc: Change) -> AnyElement {
    let (state, id) = (ctx.state.clone(), ctx.id.clone());
    let (state2, id2) = (ctx.state.clone(), ctx.id.clone());
    row(label)
        .child(small_button(SharedString::from(format!("{key}-down")), "−").on_click(move |_, _, cx| {
            let dec = dec.clone();
            edit(&state, &id, move |i| dec(i), cx);
        }))
        .child(div().w(px(VALUE_WIDTH)).flex().justify_center().text_color(theme::text_strong()).child(value))
        .child(small_button(SharedString::from(format!("{key}-up")), "+").on_click(move |_, _, cx| {
            let inc = inc.clone();
            edit(&state2, &id2, move |i| inc(i), cx);
        }))
        .into_any_element()
}

/// A row of mutually exclusive options; `apply` runs for the clicked one.
fn segmented(
    ctx: &Ctx,
    key: &'static str,
    options: Vec<(&'static str, bool, Change)>,
) -> gpui::Div {
    div().flex().rounded(px(4.)).border_1().border_color(theme::border()).overflow_hidden().children(
        options.into_iter().enumerate().map(|(n, (label, selected, apply))| {
            let (state, id) = (ctx.state.clone(), ctx.id.clone());
            div()
                .id(SharedString::from(format!("{key}-{n}")))
                .px(px(9.))
                .py(px(2.))
                .cursor_pointer()
                .when(n > 0, |d| d.border_l_1().border_color(theme::border()))
                .when(selected, |d| d.bg(theme::selection_bg()).text_color(theme::text_strong()))
                .when(!selected, |d| d.text_color(theme::text_muted()).hover(|s| s.bg(theme::hover_bg())))
                .on_click(move |_, _, cx| {
                    let apply = apply.clone();
                    edit(&state, &id, move |i| apply(i), cx);
                })
                .child(label)
        }),
    )
}

/// Like `segmented`, but switching what the tab shows (not part of the instrument).
fn view_tabs<T: Copy + PartialEq + 'static>(
    shell: &Entity<Shell>,
    key: &'static str,
    id: &InstrumentId,
    options: &[(T, &'static str)],
    selected: T,
    set: fn(&mut InstView, T),
) -> gpui::Div {
    div().flex().rounded(px(4.)).border_1().border_color(theme::border()).overflow_hidden().children(
        options.iter().enumerate().map(|(n, &(value, label))| {
            let (shell, id) = (shell.clone(), id.clone());
            div()
                .id(SharedString::from(format!("{key}-{n}")))
                .px(px(9.))
                .py(px(2.))
                .cursor_pointer()
                .when(n > 0, |d| d.border_l_1().border_color(theme::border()))
                .when(value == selected, |d| d.bg(theme::selection_bg()).text_color(theme::text_strong()))
                .when(value != selected, |d| d.text_color(theme::text_muted()).hover(|s| s.bg(theme::hover_bg())))
                .on_click(move |_, _, cx| {
                    shell.update(cx, |sh, cx| {
                        set(sh.inst_views.entry(id.clone()).or_default(), value);
                        cx.notify();
                    });
                })
                .child(label)
        }),
    )
}

fn toggle(ctx: &Ctx, key: &'static str, label: &'static str, on: bool, flip: Change) -> AnyElement {
    let (state, id) = (ctx.state.clone(), ctx.id.clone());
    div()
        .id(key)
        .flex()
        .items_center()
        .gap(px(5.))
        .px(px(7.))
        .py(px(2.))
        .rounded(px(4.))
        .border_1()
        .cursor_pointer()
        .when(on, |d| d.border_color(theme::accent()).bg(theme::selection_bg()).text_color(theme::text_strong()))
        .when(!on, |d| d.border_color(theme::border()).text_color(theme::text_muted()).hover(|s| s.bg(theme::hover_bg())))
        .on_click(move |_, _, cx| {
            let flip = flip.clone();
            edit(&state, &id, move |i| flip(i), cx);
        })
        .child(if on { "●" } else { "○" })
        .child(label)
        .into_any_element()
}

fn waveform_view(ctx: &Ctx, state: &AppState, inst: &Instrument) -> AnyElement {
    let frame = || div()
        .h(px(WAVEFORM_HEIGHT))
        .max_w(px(CONTROL_WIDTH + LABEL_WIDTH + 12.0 + VALUE_WIDTH))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::border())
        .overflow_hidden();
    let message = |text: String, warn: bool| {
        frame()
            .flex()
            .items_center()
            .justify_center()
            .text_color(if warn { theme::warning() } else { theme::text_faint() })
            .child(text)
            .into_any_element()
    };
    match state.sample_status(&inst.id) {
        SampleStatus::NoSample => return message("Drop a sample here or choose one above".into(), false),
        SampleStatus::Missing(e) => return message(format!("Missing sample: {e}"), true),
        SampleStatus::Ok => {}
    }
    let Some(peaks) = state.waveforms.get(&inst.id).map(|w| w.peaks.clone()) else {
        return message("Reading waveform…".into(), false);
    };
    let sampler = inst.sampler.clone();
    let layer_ctx = ctx.clone();
    frame().child(
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                waveform::paint(bounds, &peaks, &sampler, window);
                register_drag(bounds, hitbox, &layer_ctx, Layer::Waveform, window);
            },
        )
        .size_full(),
    )
    .into_any_element()
}

fn processing_row(ctx: &Ctx, s: &Sampler) -> AnyElement {
    let p = s.processing;
    div()
        .flex()
        .flex_wrap()
        .gap(px(6.))
        .child(toggle(ctx, "reverse", "Reverse", p.reverse, change(|i| i.sampler.processing.reverse ^= true)))
        .child(toggle(ctx, "normalize", "Normalize", p.normalize, change(|i| i.sampler.processing.normalize ^= true)))
        .child(toggle(ctx, "invert", "Invert", p.invert_polarity, change(|i| i.sampler.processing.invert_polarity ^= true)))
        .child(toggle(ctx, "swap", "Swap L/R", p.swap_stereo, change(|i| i.sampler.processing.swap_stereo ^= true)))
        .child(toggle(ctx, "declick", "Declick", p.declick, change(|i| i.sampler.processing.declick ^= true)))
        .child(toggle(ctx, "one-shot", "One-shot", s.one_shot, change(|i| i.sampler.one_shot ^= true)))
        .into_any_element()
}

fn loop_row(ctx: &Ctx, inst: &Instrument) -> AnyElement {
    let mode = inst.sampler.looping.mode;
    let set = |m: LoopMode| {
        change(move |i: &mut Instrument| {
            i.sampler.looping.mode = m;
            // Loop points may be stale from before; make sure they fit the region.
            keep_loop_in_region(&mut i.sampler);
        })
    };
    let modes = segmented(
        ctx,
        "loop-mode",
        vec![
            ("Off", mode == LoopMode::Off, set(LoopMode::Off)),
            ("Forward", mode == LoopMode::Forward, set(LoopMode::Forward)),
            ("Ping-pong", mode == LoopMode::PingPong, set(LoopMode::PingPong)),
        ],
    );
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(row("Loop").child(modes))
        .when(mode == LoopMode::Forward, |d| d.child(slider_row(ctx, &param::CROSSFADE, inst, false)))
        .into_any_element()
}

fn envelope_section(ctx: &Ctx, inst: &Instrument, target: EnvTarget) -> AnyElement {
    let options: Vec<(EnvTarget, &'static str)> = EnvTarget::ALL.iter().map(|t| (*t, t.label())).collect();
    let tabs = view_tabs(&ctx.shell, "env-target", &ctx.id, &options, target, |v, t| v.env = t);
    let env = target.env(&inst.sampler);
    let layer_ctx = ctx.clone();
    let graph = div()
        .h(px(GRAPH_HEIGHT))
        .max_w(px(CONTROL_WIDTH + LABEL_WIDTH + 12.0 + VALUE_WIDTH))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::border())
        .overflow_hidden()
        .child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, _| {
                    envelope_graph::paint(bounds, &env, window);
                    register_drag(bounds, hitbox, &layer_ctx, Layer::EnvGraph(target), window);
                },
            )
            .size_full(),
        );
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(tabs)
        .child(graph)
        .when_some(target.amount(), |d, amount| d.child(slider_row(ctx, amount, inst, false)))
        .children(target.specs().iter().map(|spec| slider_row(ctx, spec, inst, false)))
        .into_any_element()
}

fn filter_section(ctx: &Ctx, inst: &Instrument) -> AnyElement {
    let kind = inst.sampler.filter.kind;
    let set = |k: FilterKind| change(move |i: &mut Instrument| i.sampler.filter.kind = k);
    let kinds = segmented(
        ctx,
        "filter-kind",
        vec![
            ("Off", kind == FilterKind::Off, set(FilterKind::Off)),
            ("Low-pass", kind == FilterKind::LowPass, set(FilterKind::LowPass)),
            ("High-pass", kind == FilterKind::HighPass, set(FilterKind::HighPass)),
            ("Band-pass", kind == FilterKind::BandPass, set(FilterKind::BandPass)),
            ("Notch", kind == FilterKind::Notch, set(FilterKind::Notch)),
        ],
    );
    let off = kind == FilterKind::Off;
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(row("Type").child(kinds))
        .child(slider_row(ctx, &param::CUTOFF, inst, off))
        .child(slider_row(ctx, &param::RESONANCE, inst, off))
        .into_any_element()
}

/// Index into `SYNC_VALUES` of a synced rate (the nearest, if it is not in the list).
fn sync_index(numerator: u16, denominator: u16) -> usize {
    let whole = f32::from(numerator) / f32::from(denominator.max(1));
    param::SYNC_VALUES
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let da = (f32::from(a.1.0) / f32::from(a.1.1) / whole).ln().abs();
            let db = (f32::from(b.1.0) / f32::from(b.1.1) / whole).ln().abs();
            da.total_cmp(&db)
        })
        .map_or(4, |(i, _)| i)
}

fn lfo_section(ctx: &Ctx, inst: &Instrument, target: LfoTarget) -> AnyElement {
    let options: Vec<(LfoTarget, &'static str)> = LfoTarget::ALL.iter().map(|t| (*t, t.label())).collect();
    let tabs = view_tabs(&ctx.shell, "lfo-target", &ctx.id, &options, target, |v, t| v.lfo = t);
    let lfo = target.lfo(&inst.sampler);
    let shape = |sh: LfoShape| change(move |i: &mut Instrument| target.lfo_mut(&mut i.sampler).shape = sh);
    let shapes = segmented(
        ctx,
        "lfo-shape",
        vec![
            ("Sine", lfo.shape == LfoShape::Sine, shape(LfoShape::Sine)),
            ("Triangle", lfo.shape == LfoShape::Triangle, shape(LfoShape::Triangle)),
            ("Saw", lfo.shape == LfoShape::Saw, shape(LfoShape::Saw)),
            ("Square", lfo.shape == LfoShape::Square, shape(LfoShape::Square)),
            ("Random", lfo.shape == LfoShape::Random, shape(LfoShape::Random)),
        ],
    );
    let synced = matches!(lfo.rate, LfoRate::Sync { .. });
    let rate_mode = segmented(
        ctx,
        "lfo-sync",
        vec![
            (
                "Free",
                !synced,
                change(move |i: &mut Instrument| {
                    let l = target.lfo_mut(&mut i.sampler);
                    if matches!(l.rate, LfoRate::Sync { .. }) {
                        l.rate = LfoRate::Hz(4.0);
                    }
                }),
            ),
            (
                "Tempo",
                synced,
                change(move |i: &mut Instrument| {
                    let l = target.lfo_mut(&mut i.sampler);
                    if matches!(l.rate, LfoRate::Hz(_)) {
                        l.rate = LfoRate::Sync { numerator: 1, denominator: 4 };
                    }
                }),
            ),
        ],
    );
    let rate_control = match lfo.rate {
        LfoRate::Hz(_) => slider_row(ctx, target.hz_spec(), inst, false),
        LfoRate::Sync { numerator, denominator } => {
            let step = |delta: i32| {
                change(move |i: &mut Instrument| {
                    let l = target.lfo_mut(&mut i.sampler);
                    if let LfoRate::Sync { numerator, denominator } = l.rate {
                        let n = param::SYNC_VALUES.len() as i32;
                        let next = (sync_index(numerator, denominator) as i32 + delta).clamp(0, n - 1) as usize;
                        let (numerator, denominator) = param::SYNC_VALUES[next];
                        l.rate = LfoRate::Sync { numerator, denominator };
                    }
                })
            };
            stepper_row(ctx, "Rate", "lfo-rate", param::fmt_sync(numerator, denominator), step(-1), step(1))
        }
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(tabs)
        .child(row("Shape").child(shapes))
        .child(row("Rate mode").child(rate_mode))
        .child(rate_control)
        .children(target.specs().iter().map(|spec| slider_row(ctx, spec, inst, false)))
        .into_any_element()
}

fn voice_section(ctx: &Ctx, inst: &Instrument) -> AnyElement {
    let v = inst.sampler.voice;
    let group = |g: u8| if g == 0 { "none".to_string() } else { g.to_string() };
    let max_group = *limits::CUT_GROUP.end();
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(stepper_row(
            ctx,
            "Polyphony",
            "polyphony",
            v.polyphony.to_string(),
            change(|i| i.sampler.voice.polyphony = i.sampler.voice.polyphony.saturating_sub(1).max(1)),
            change(|i| i.sampler.voice.polyphony = (i.sampler.voice.polyphony + 1).min(*limits::POLYPHONY.end())),
        ))
        .child(row("Mono").child(toggle(ctx, "mono", "Mono", v.mono, change(|i| i.sampler.voice.mono ^= true))))
        .child(slider_row(ctx, &param::GLIDE, inst, !v.mono))
        .child(stepper_row(
            ctx,
            "Cut",
            "cut",
            group(v.cut),
            change(|i| i.sampler.voice.cut = i.sampler.voice.cut.saturating_sub(1)),
            change(move |i| i.sampler.voice.cut = (i.sampler.voice.cut + 1).min(max_group)),
        ))
        .child(stepper_row(
            ctx,
            "Cut by",
            "cut-by",
            group(v.cut_by),
            change(|i| i.sampler.voice.cut_by = i.sampler.voice.cut_by.saturating_sub(1)),
            change(move |i| i.sampler.voice.cut_by = (i.sampler.voice.cut_by + 1).min(max_group)),
        ))
        .into_any_element()
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
    fn sync_index_finds_exact_and_nearest() {
        assert_eq!(param::SYNC_VALUES[sync_index(1, 4)], (1, 4));
        assert_eq!(param::SYNC_VALUES[sync_index(8, 1)], (8, 1));
        // 3/16 is between 1/8 and 1/4; either neighbour is fine, but never far off.
        let (n, d) = param::SYNC_VALUES[sync_index(3, 16)];
        assert!([(1, 8), (1, 4)].contains(&(n, d)));
    }

    #[test]
    fn env_targets_read_and_write_their_own_envelope() {
        let mut s = Sampler::default();
        for (n, t) in EnvTarget::ALL.into_iter().enumerate() {
            t.env_mut(&mut s).attack_ms = 10.0 * (n + 1) as f32;
        }
        assert_eq!(s.volume_env.attack_ms, 10.0);
        assert_eq!(s.cutoff_env.env.attack_ms, 20.0);
        assert_eq!(s.pitch_env.env.attack_ms, 30.0);
        assert_eq!(s.pan_env.env.attack_ms, 40.0);
        for t in EnvTarget::ALL {
            assert_eq!(t.env(&s), *t.env_mut(&mut s.clone()));
        }
    }

    #[test]
    fn lfo_targets_read_and_write_their_own_lfo() {
        let mut s = Sampler::default();
        for (n, t) in LfoTarget::ALL.into_iter().enumerate() {
            t.lfo_mut(&mut s).delay_ms = n as f32;
        }
        assert_eq!((s.volume_lfo.delay_ms, s.pan_lfo.delay_ms, s.cutoff_lfo.delay_ms, s.pitch_lfo.delay_ms), (0.0, 1.0, 2.0, 3.0));
        for t in LfoTarget::ALL {
            assert_eq!(t.lfo(&s), *t.lfo_mut(&mut s.clone()));
        }
    }
}
