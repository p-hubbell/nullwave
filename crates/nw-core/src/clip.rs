//! Clip maths: what a clip's window lets through, and moving its edges.

use crate::model::{Clip, ClipKey, Note};
use crate::time::Tick;

impl Clip {
    /// An empty clip whose window starts at its first tick.
    pub fn new(key: ClipKey, length: Tick) -> Clip {
        Clip { key, start: 0, length, notes: Vec::new() }
    }

    /// The notes that sound, in window-relative ticks. A note sounds if it starts inside
    /// the window; it is cut at the window's end.
    pub fn audible(&self) -> impl Iterator<Item = Note> + '_ {
        let end = self.start.saturating_add(self.length);
        self.notes.iter().filter(move |n| n.start >= self.start && n.start < end).map(move |n| Note {
            start: n.start - self.start,
            length: n.length.min(end - n.start),
            ..n.clone()
        })
    }
}

/// `clip` with its window `length` ticks long (at least 1).
pub fn resize_end(clip: &Clip, length: Tick) -> Clip {
    Clip { length: length.max(1), ..clip.clone() }
}

/// Moves the window's start by `delta` ticks while the notes stay where they are in time.
/// Returns the new clip and the delta actually applied (every block of the clip moves by
/// it). The window keeps at least one tick; moving before the clip's first tick shifts
/// the notes later so the window can start at 0.
pub fn trim_start(clip: &Clip, delta: i64) -> (Clip, i64) {
    let delta = delta.min(i64::from(clip.length) - 1);
    let new_start = i64::from(clip.start) + delta;
    let mut out = clip.clone();
    out.length = (i64::from(clip.length) - delta) as Tick;
    if new_start >= 0 {
        out.start = new_start as Tick;
    } else {
        let shift = (-new_start) as Tick;
        out.start = 0;
        for n in &mut out.notes {
            n.start += shift;
        }
    }
    (out, delta)
}
