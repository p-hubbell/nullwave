//! Envelope graph: draws a DAHDSR shape and turns drags on its points into new values.
//! Layout and drag maths are pure; the instrument tab wires the mouse.

use gpui::{Bounds, Pixels, Window, fill, point, px, rgba, size};
use nw_core::{Envelope, limits};
use nw_engine::env::shape;

use crate::theme;

/// The draggable points of the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvPoint {
    /// End of the attack (x: attack time).
    Attack,
    /// End of the decay (x: decay time, y: sustain level).
    Decay,
    /// End of the release (x: release time).
    Release,
}

/// Inner padding so end points are grabbable.
pub const PAD: f32 = 8.0;

/// Where each stage sits along x, in pixels from the left, for one graph width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub px_per_ms: f32,
    /// x of: delay end, attack end, hold end, decay end, sustain end, release end.
    pub x: [f32; 6],
}

/// Fixed-width stand-in for the sustain stage (it lasts as long as the note).
fn sustain_ms(total: f32) -> f32 {
    (total * 0.3).max(100.0)
}

pub fn layout(e: &Envelope, width: f32) -> Layout {
    let stages = [e.delay_ms, e.attack_ms, e.hold_ms, e.decay_ms].map(|v| v.max(0.0));
    let release = e.release_ms.max(0.0);
    let total = stages.iter().sum::<f32>() + release;
    let span = total + sustain_ms(total);
    let px_per_ms = (width - 2.0 * PAD).max(1.0) / span;
    let mut x = [0.0; 6];
    let mut at = PAD;
    for (i, ms) in stages.iter().chain([sustain_ms(total), release].iter()).enumerate() {
        at += ms * px_per_ms;
        x[i] = at;
    }
    Layout { px_per_ms, x }
}

/// The point (if any) within `radius` px of (`x`, `y`), for a graph `height` px tall.
pub fn point_at(e: &Envelope, width: f32, height: f32, x: f32, y: f32, radius: f32) -> Option<EnvPoint> {
    let l = layout(e, width);
    let level_y = |level: f32| PAD + (1.0 - level) * (height - 2.0 * PAD);
    let points = [
        (EnvPoint::Attack, l.x[1], level_y(1.0)),
        (EnvPoint::Decay, l.x[3], level_y(e.sustain)),
        (EnvPoint::Release, l.x[5], level_y(0.0)),
    ];
    points
        .into_iter()
        .map(|(p, px, py)| (p, ((px - x).powi(2) + (py - y).powi(2)).sqrt()))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

/// `start` with `point` dragged by `d_ms` along time and `d_level` (0..=1 units) up.
pub fn drag_env_point(start: &Envelope, point: EnvPoint, d_ms: f32, d_level: f32) -> Envelope {
    let mut e = *start;
    let ms = |v: f32| {
        let v = v.round();
        if v.is_finite() { v.clamp(*limits::STAGE_MS.start(), *limits::STAGE_MS.end()) } else { 0.0 }
    };
    match point {
        EnvPoint::Attack => e.attack_ms = ms(start.attack_ms + d_ms),
        EnvPoint::Decay => {
            e.decay_ms = ms(start.decay_ms + d_ms);
            let level = start.sustain + d_level;
            e.sustain = if level.is_finite() { (level * 100.0).round().clamp(0.0, 100.0) / 100.0 } else { start.sustain };
        }
        EnvPoint::Release => e.release_ms = ms(start.release_ms + d_ms),
    }
    e
}

fn rect(b: Bounds<Pixels>, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(b.origin + point(px(x), px(y)), size(px(w.max(0.0)), px(h.max(0.0))))
}

/// The envelope's level at `x` px (0..=1), following the same curves as the engine.
fn level_at(e: &Envelope, l: &Layout, x: f32) -> f32 {
    let seg = |from: f32, to: f32| if to > from { ((x - from) / (to - from)).clamp(0.0, 1.0) } else { 1.0 };
    if x < l.x[0] {
        0.0
    } else if x < l.x[1] {
        shape(seg(l.x[0], l.x[1]), e.attack_curve)
    } else if x < l.x[2] {
        1.0
    } else if x < l.x[3] {
        1.0 + (e.sustain - 1.0) * shape(seg(l.x[2], l.x[3]), e.decay_curve)
    } else if x < l.x[4] {
        e.sustain
    } else if x < l.x[5] {
        e.sustain * (1.0 - shape(seg(l.x[4], l.x[5]), e.decay_curve))
    } else {
        0.0
    }
}

pub fn paint(bounds: Bounds<Pixels>, e: &Envelope, window: &mut Window) {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    window.paint_quad(fill(bounds, theme::chrome_bg()));
    let l = layout(e, w);
    let inner = h - 2.0 * PAD;
    let y_of = |level: f32| PAD + (1.0 - level) * inner;
    // Area under the curve, then a 2 px line on top.
    let start = PAD as usize;
    let end = (w - PAD).max(PAD) as usize;
    for x in start..end {
        let level = level_at(e, &l, x as f32 + 0.5);
        let y = y_of(level);
        window.paint_quad(fill(rect(bounds, x as f32, y, 1.0, PAD + inner - y), rgba(0x6aa0ff26)));
        window.paint_quad(fill(rect(bounds, x as f32, y - 1.0, 1.0, 2.0), theme::accent()));
    }
    // Sustain stretch is drawn dashed-ish by a faint marker band.
    window.paint_quad(fill(rect(bounds, l.x[3], 0.0, l.x[4] - l.x[3], h), rgba(0xffffff08)));
    for (x, level) in [(l.x[1], 1.0), (l.x[3], e.sustain), (l.x[5], 0.0)] {
        let y = y_of(level);
        window.paint_quad(fill(rect(bounds, x - 4.0, y - 4.0, 8.0, 8.0), theme::text_strong()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Envelope {
        Envelope { delay_ms: 10.0, attack_ms: 40.0, hold_ms: 0.0, decay_ms: 100.0, sustain: 0.5, release_ms: 200.0, ..Envelope::default() }
    }

    #[test]
    fn layout_points_increase_and_fit() {
        let l = layout(&env(), 400.0);
        assert!(l.x.windows(2).all(|w| w[1] >= w[0]));
        assert!((l.x[5] - (400.0 - PAD)).abs() < 1e-3, "the release ends at the right edge");
        assert!((l.x[1] - l.x[0] - 40.0 * l.px_per_ms).abs() < 1e-3);
        // An all-zero envelope still lays out (sustain gets the width).
        let zero = Envelope { release_ms: 0.0, ..Envelope::default() };
        let l = layout(&zero, 200.0);
        assert!(l.px_per_ms.is_finite() && l.px_per_ms > 0.0);
    }

    #[test]
    fn drag_moves_only_its_stage() {
        let e = env();
        let a = drag_env_point(&e, EnvPoint::Attack, 25.0, 0.3);
        assert_eq!(a.attack_ms, 65.0);
        assert_eq!(Envelope { attack_ms: 40.0, ..a }, e, "level is ignored for the attack point");
        let d = drag_env_point(&e, EnvPoint::Decay, -30.0, 0.2);
        assert_eq!((d.decay_ms, d.sustain), (70.0, 0.7));
        let r = drag_env_point(&e, EnvPoint::Release, 50.4, 0.0);
        assert_eq!(r.release_ms, 250.0);
    }

    #[test]
    fn drag_clamps_to_valid_ranges() {
        let e = env();
        assert_eq!(drag_env_point(&e, EnvPoint::Attack, -500.0, 0.0).attack_ms, 0.0);
        assert_eq!(drag_env_point(&e, EnvPoint::Release, 1e9, 0.0).release_ms, 5000.0);
        assert_eq!(drag_env_point(&e, EnvPoint::Decay, 0.0, 3.0).sustain, 1.0);
        assert_eq!(drag_env_point(&e, EnvPoint::Decay, 0.0, -3.0).sustain, 0.0);
        let nan = drag_env_point(&e, EnvPoint::Decay, f32::NAN, f32::NAN);
        assert_eq!((nan.decay_ms, nan.sustain), (0.0, 0.5));
        for p in [EnvPoint::Attack, EnvPoint::Decay, EnvPoint::Release] {
            let s = nw_core::Sampler { volume_env: drag_env_point(&e, p, 1e6, 10.0), ..Default::default() };
            assert_eq!(s.validate(), Ok(()));
        }
    }

    #[test]
    fn points_are_hit_near_their_handles() {
        let e = env();
        let l = layout(&e, 400.0);
        let height = 100.0;
        assert_eq!(point_at(&e, 400.0, height, l.x[1], PAD, 8.0), Some(EnvPoint::Attack));
        let sustain_y = PAD + 0.5 * (height - 2.0 * PAD);
        assert_eq!(point_at(&e, 400.0, height, l.x[3] + 3.0, sustain_y, 8.0), Some(EnvPoint::Decay));
        assert_eq!(point_at(&e, 400.0, height, l.x[5], height - PAD, 8.0), Some(EnvPoint::Release));
        assert_eq!(point_at(&e, 400.0, height, 200.0, 5.0, 8.0), None);
    }

    #[test]
    fn drawn_level_matches_stage_values() {
        let e = env();
        let l = layout(&e, 400.0);
        assert_eq!(level_at(&e, &l, PAD - 1.0), 0.0);
        assert!((level_at(&e, &l, l.x[1] + 0.1) - 1.0).abs() < 1e-3);
        assert!((level_at(&e, &l, (l.x[3] + l.x[4]) / 2.0) - 0.5).abs() < 1e-6);
        assert_eq!(level_at(&e, &l, l.x[5] + 1.0), 0.0);
    }
}
