//! Status bar. Left: play/stop, playhead, active pattern, tempo (click to edit), time
//! signature. Right: sample rate, buffer size and CPU load, or "No audio output".

use gpui::{Context, Div, div, prelude::*, px};
use nw_core::{PPQ, Tick, TimeSignature};

use crate::actions::transport;
use crate::theme;
use crate::ui::shell::{Shell, format_tempo};

/// `bar.beat.sixteenth`, all 1-based.
pub fn format_position(tick: Tick, ts: TimeSignature) -> String {
    let bar_ticks = ts.ticks_per_bar().max(1);
    let beat_ticks = (PPQ * 4 / u32::from(ts.denominator.max(1))).max(1);
    let sixteenth_ticks = PPQ / 4;
    let in_bar = tick % bar_ticks;
    let bar = tick / bar_ticks + 1;
    let beat = in_bar / beat_ticks + 1;
    let sixteenth = (in_bar % beat_ticks) / sixteenth_ticks + 1;
    format!("{bar}.{beat}.{sixteenth}")
}

fn item() -> Div {
    div().flex().items_center().h_full().whitespace_nowrap()
}

pub fn render(shell: &Shell, cx: &Context<Shell>) -> impl IntoElement {
    let state = shell.state.read(cx);
    let has_project = state.project_dir.is_some();
    let ts = state.project.time_signature;
    let pattern = state.status_pattern().map(|p| p.name.clone());

    let play = div()
        .id("play-stop")
        .w(px(18.))
        .flex()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.text_color(theme::text_strong()))
        .on_click(|_, window, cx| window.dispatch_action(Box::new(transport::PlayStop), cx))
        .child(if state.playing { "■" } else { "▶" });

    let tempo = match &shell.tempo_field {
        Some(field) => item().w(px(60.)).child(field.clone()).into_any_element(),
        None => item()
            .id("tempo")
            .when(has_project, |d| {
                d.cursor_pointer()
                    .hover(|s| s.text_color(theme::text_strong()))
                    .on_click(cx.listener(|shell, _, window, cx| shell.begin_tempo_edit(window, cx)))
            })
            .child(format!("{} BPM", format_tempo(state.project.tempo)))
            .into_any_element(),
    };

    let left = div()
        .flex()
        .items_center()
        .h_full()
        .gap(px(18.))
        .child(play)
        .child(item().child(format_position(state.playhead, ts)))
        .when_some(pattern, |d, name| d.child(item().child(name)))
        .when(has_project, |d| {
            d.child(tempo).child(item().child(format!("{}/{}", ts.numerator, ts.denominator)))
        });

    let right = match &state.engine {
        Some(engine) => div()
            .flex()
            .items_center()
            .h_full()
            .gap(px(18.))
            .child(item().child(format!("{} kHz", format_khz(engine.sample_rate))))
            .child(item().child(match engine.buffer_size {
                Some(b) => format!("{b} samples"),
                None => "default buffer".to_string(),
            }))
            .child(item().child(format!("CPU {:.0}%", (state.cpu_load * 100.0).max(0.0)))),
        None => div().child(item().text_color(theme::warning()).child("No audio output")),
    };

    div()
        .h(px(theme::STATUS_BAR_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(12.))
        .bg(theme::status_bg())
        .text_color(theme::status_text())
        .text_size(px(11.))
        .child(left)
        .child(right)
}

/// `48`, `44.1`.
fn format_khz(rate: u32) -> String {
    let khz = f64::from(rate) / 1000.0;
    if khz.fract() == 0.0 { format!("{khz:.0}") } else { format!("{khz:.1}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOUR_FOUR: TimeSignature = TimeSignature { numerator: 4, denominator: 4 };

    #[test]
    fn position_is_one_based() {
        assert_eq!(format_position(0, FOUR_FOUR), "1.1.1");
    }

    #[test]
    fn position_counts_sixteenths_beats_and_bars() {
        assert_eq!(format_position(PPQ / 4, FOUR_FOUR), "1.1.2");
        assert_eq!(format_position(PPQ + PPQ / 2, FOUR_FOUR), "1.2.3");
        assert_eq!(format_position(4 * PPQ, FOUR_FOUR), "2.1.1");
        assert_eq!(format_position(4 * PPQ - 1, FOUR_FOUR), "1.4.4");
    }

    #[test]
    fn position_uses_the_time_signature() {
        let six_eight = TimeSignature { numerator: 6, denominator: 8 };
        // A 6/8 beat is an eighth note (two sixteenths); a bar is six of them.
        assert_eq!(format_position(PPQ / 2, six_eight), "1.2.1");
        assert_eq!(format_position(3 * PPQ, six_eight), "2.1.1");
    }

    #[test]
    fn khz_formatting() {
        assert_eq!(format_khz(48_000), "48");
        assert_eq!(format_khz(44_100), "44.1");
    }
}
