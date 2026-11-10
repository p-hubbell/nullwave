//! Waveform view for the sampler: the processed sample's peaks, the region (dimmed
//! outside), loop markers, and dragging those markers. Marker maths is pure and keeps the
//! sampler valid: start before end, loop inside the region.

use gpui::{Bounds, Pixels, Window, fill, point, px, rgba, size};
use nw_core::{LoopMode, Sampler};

use crate::theme;

/// Smallest region or loop, as a fraction of the file.
pub const MIN_SPAN: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Start,
    End,
    LoopStart,
    LoopEnd,
}

/// Pulls the loop inside the region; a loop squeezed to nothing becomes the whole region.
pub fn keep_loop_in_region(s: &mut Sampler) {
    let r = s.region;
    s.looping.start = s.looping.start.max(r.start).min(r.end);
    s.looping.end = s.looping.end.max(r.start).min(r.end);
    if s.looping.end - s.looping.start < MIN_SPAN {
        s.looping.start = r.start;
        s.looping.end = r.end;
    }
}

/// `s` with `marker` moved to `fraction` of the file, clamped so everything stays valid.
pub fn moved_marker(s: &Sampler, marker: Marker, fraction: f32) -> Sampler {
    let mut s = s.clone();
    let f = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    match marker {
        Marker::Start => s.region.start = f.min(s.region.end - MIN_SPAN).max(0.0),
        Marker::End => s.region.end = f.max(s.region.start + MIN_SPAN).min(1.0),
        Marker::LoopStart => s.looping.start = f.min(s.looping.end - MIN_SPAN).max(s.region.start),
        Marker::LoopEnd => s.looping.end = f.max(s.looping.start + MIN_SPAN).min(s.region.end),
    }
    keep_loop_in_region(&mut s);
    s
}

/// The marker within `tolerance` (fraction of the width) of `fraction`, nearest first.
/// Loop markers only count while the loop is on.
pub fn marker_at(s: &Sampler, fraction: f32, tolerance: f32) -> Option<Marker> {
    let mut candidates = vec![(Marker::Start, s.region.start), (Marker::End, s.region.end)];
    if s.looping.mode != LoopMode::Off {
        candidates.push((Marker::LoopStart, s.looping.start));
        candidates.push((Marker::LoopEnd, s.looping.end));
    }
    candidates
        .into_iter()
        .map(|(m, at)| (m, (at - fraction).abs()))
        .filter(|(_, d)| *d <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(m, _)| m)
}

fn marker_color() -> gpui::Rgba {
    theme::accent()
}
fn loop_color() -> gpui::Rgba {
    gpui::rgb(0xffd166)
}

fn rect(b: Bounds<Pixels>, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(b.origin + point(px(x), px(y)), size(px(w.max(0.0)), px(h.max(0.0))))
}

/// Paints peaks (min/max per bucket, −1..=1), the region and the loop markers.
pub fn paint(bounds: Bounds<Pixels>, peaks: &[(f32, f32)], s: &Sampler, window: &mut Window) {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    let mid = h / 2.0;
    window.paint_quad(fill(bounds, theme::chrome_bg()));
    window.paint_quad(fill(rect(bounds, 0.0, mid, w, 1.0), rgba(0xffffff14)));
    if !peaks.is_empty() {
        let columns = w.max(1.0) as usize;
        for x in 0..columns {
            let i = (x * peaks.len() / columns).min(peaks.len() - 1);
            let (lo, hi) = peaks[i];
            let top = mid - hi.clamp(-1.0, 1.0) * mid;
            let bottom = mid - lo.clamp(-1.0, 1.0) * mid;
            window.paint_quad(fill(rect(bounds, x as f32, top, 1.0, (bottom - top).max(1.0)), gpui::rgb(0x6f8fbf)));
        }
    }
    // Dim what is outside the region.
    let (a, b) = (s.region.start * w, s.region.end * w);
    window.paint_quad(fill(rect(bounds, 0.0, 0.0, a, h), rgba(0x000000a0)));
    window.paint_quad(fill(rect(bounds, b, 0.0, w - b, h), rgba(0x000000a0)));
    for x in [a, b] {
        window.paint_quad(fill(rect(bounds, x - 1.0, 0.0, 2.0, h), marker_color()));
        window.paint_quad(fill(rect(bounds, x - 4.0, 0.0, 8.0, 6.0), marker_color()));
    }
    if s.looping.mode != LoopMode::Off {
        let (ls, le) = (s.looping.start * w, s.looping.end * w);
        window.paint_quad(fill(rect(bounds, ls, 0.0, le - ls, h), rgba(0xffd16614)));
        for x in [ls, le] {
            window.paint_quad(fill(rect(bounds, x - 1.0, 0.0, 2.0, h), loop_color()));
            window.paint_quad(fill(rect(bounds, x - 4.0, h - 6.0, 8.0, 6.0), loop_color()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_core::{Loop, Region};

    fn looping() -> Sampler {
        Sampler {
            region: Region { start: 0.1, end: 0.9 },
            looping: Loop { mode: LoopMode::Forward, start: 0.3, end: 0.6, crossfade_ms: 0.0 },
            ..Sampler::default()
        }
    }

    #[test]
    fn marker_drag_keeps_start_before_end() {
        let s = looping();
        let moved = moved_marker(&s, Marker::Start, 0.95);
        assert!(moved.region.start < moved.region.end);
        let moved = moved_marker(&s, Marker::End, 0.0);
        assert!(moved.region.start < moved.region.end);
        let moved = moved_marker(&s, Marker::LoopEnd, 0.0);
        assert!(moved.looping.start < moved.looping.end);
    }

    #[test]
    fn trimming_the_region_pulls_the_loop_inside() {
        let s = moved_marker(&looping(), Marker::Start, 0.45);
        assert_eq!(s.region.start, 0.45);
        assert_eq!(s.looping.start, 0.45);
        assert_eq!(s.looping.end, 0.6);
        // Squeezing the loop out of the region resets it to the whole region.
        let s = moved_marker(&looping(), Marker::End, 0.2);
        assert_eq!((s.looping.start, s.looping.end), (s.region.start, s.region.end));
    }

    #[test]
    fn loop_markers_stay_inside_the_region() {
        let s = moved_marker(&looping(), Marker::LoopStart, 0.0);
        assert_eq!(s.looping.start, 0.1);
        let s = moved_marker(&looping(), Marker::LoopEnd, 1.0);
        assert_eq!(s.looping.end, 0.9);
    }

    #[test]
    fn every_drag_leaves_a_valid_sampler() {
        for marker in [Marker::Start, Marker::End, Marker::LoopStart, Marker::LoopEnd] {
            for mode in [LoopMode::Forward, LoopMode::PingPong] {
                let mut s = looping();
                s.looping.mode = mode;
                for k in -5..=105 {
                    s = moved_marker(&s, marker, k as f32 / 100.0);
                    assert_eq!(s.validate(), Ok(()), "{marker:?} to {k}%");
                }
            }
        }
        // Garbage input does not panic or break validity either.
        let s = moved_marker(&looping(), Marker::Start, f32::NAN);
        assert_eq!(s.validate(), Ok(()));
    }

    #[test]
    fn marker_hit_testing() {
        let s = looping();
        assert_eq!(marker_at(&s, 0.105, 0.01), Some(Marker::Start));
        assert_eq!(marker_at(&s, 0.59, 0.02), Some(Marker::LoopEnd));
        assert_eq!(marker_at(&s, 0.45, 0.01), None);
        let off = Sampler { looping: Loop { mode: LoopMode::Off, ..s.looping }, ..s };
        assert_eq!(marker_at(&off, 0.3, 0.01), None, "loop markers are hidden while the loop is off");
    }
}
