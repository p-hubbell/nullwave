//! Pure geometry and note-editing math for the piano roll view.

use nw_core::{Note, Tick, DEFAULT_VELOCITY};

pub const DEFAULT_SNAP: Tick = 240;
pub const EDGE_PX: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PianoRollView {
    pub px_per_tick: f32,
    pub row_height: f32,
    pub top_pitch: u8,
    pub scroll_ticks: f32,
}

impl Default for PianoRollView {
    fn default() -> Self {
        Self { px_per_tick: 0.1, row_height: 14.0, top_pitch: 84, scroll_ticks: 0.0 }
    }
}

impl PianoRollView {
    pub fn tick_at(&self, x: f32) -> f32 {
        self.scroll_ticks + x / self.px_per_tick
    }

    pub fn pitch_at(&self, y: f32) -> Option<u8> {
        if y < 0.0 {
            return None;
        }
        let row = (y / self.row_height).floor() as i64;
        let pitch = self.top_pitch as i64 - row;
        (0..=127).contains(&pitch).then_some(pitch as u8)
    }

    /// (x, y, w, h) of a note in grid coordinates.
    pub fn note_rect(&self, n: &Note) -> (f32, f32, f32, f32) {
        (
            (n.start as f32 - self.scroll_ticks) * self.px_per_tick,
            (self.top_pitch as f32 - n.pitch as f32) * self.row_height,
            n.length as f32 * self.px_per_tick,
            self.row_height,
        )
    }
}

pub fn snap_floor(tick: f32, snap: Tick) -> Tick {
    if tick <= 0.0 {
        return 0;
    }
    if snap == 0 {
        return tick.floor() as Tick;
    }
    (tick / snap as f32).floor() as Tick * snap
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Body(usize),
    RightEdge(usize),
    Empty { tick: Tick, pitch: u8 },
}

pub fn hit_test(notes: &[Note], view: &PianoRollView, x: f32, y: f32, snap: Tick) -> Hit {
    for (i, n) in notes.iter().enumerate().rev() {
        let (rx, ry, w, h) = view.note_rect(n);
        if x >= rx && x <= rx + w && y >= ry && y < ry + h {
            let edge = EDGE_PX.min(w / 3.0);
            return if x >= rx + w - edge { Hit::RightEdge(i) } else { Hit::Body(i) };
        }
    }
    let row = (y / view.row_height).floor() as i64;
    let pitch = (view.top_pitch as i64 - row).clamp(0, 127) as u8;
    Hit::Empty { tick: snap_floor(view.tick_at(x), snap), pitch }
}

pub fn place_note(tick: Tick, pitch: u8, last_length: Tick) -> Note {
    Note { pitch, start: tick, length: last_length, velocity: DEFAULT_VELOCITY }
}

fn valid<'a>(notes: &'a [Note], selected: &'a [usize]) -> impl Iterator<Item = usize> + 'a {
    selected.iter().copied().filter(move |&i| i < notes.len())
}

pub fn moved(notes: &[Note], selected: &[usize], d_ticks: i64, d_pitch: i32) -> Vec<Note> {
    let mut out = notes.to_vec();
    let sel: Vec<usize> = valid(notes, selected).collect();
    if sel.is_empty() {
        return out;
    }
    let min_start = sel.iter().map(|&i| notes[i].start as i64).min().unwrap();
    let min_pitch = sel.iter().map(|&i| notes[i].pitch as i64).min().unwrap();
    let max_pitch = sel.iter().map(|&i| notes[i].pitch as i64).max().unwrap();
    let dt = d_ticks.max(-min_start);
    let dp = (d_pitch as i64).clamp(-min_pitch, 127 - max_pitch);
    for i in sel {
        out[i].start = (notes[i].start as i64).saturating_add(dt).clamp(0, Tick::MAX as i64) as Tick;
        out[i].pitch = (notes[i].pitch as i64 + dp) as u8;
    }
    out
}

pub fn resized(notes: &[Note], selected: &[usize], d_ticks: i64, snap: Tick) -> Vec<Note> {
    let mut out = notes.to_vec();
    let min = snap.max(1) as i64;
    for i in valid(notes, selected) {
        let len = (notes[i].length as i64).saturating_add(d_ticks).max(min);
        out[i].length = len.min(Tick::MAX as i64) as Tick;
    }
    out
}

pub fn duplicated(notes: &[Note], selected: &[usize]) -> (Vec<Note>, Vec<usize>) {
    let mut out = notes.to_vec();
    let sel: Vec<usize> = valid(notes, selected).collect();
    if sel.is_empty() {
        return (out, Vec::new());
    }
    let start = sel.iter().map(|&i| notes[i].start as i64).min().unwrap();
    let end = sel.iter().map(|&i| notes[i].start as i64 + notes[i].length as i64).max().unwrap();
    let offset = end - start;
    let mut idx = Vec::with_capacity(sel.len());
    for i in sel {
        let mut c = notes[i].clone();
        c.start = (c.start as i64 + offset).min(Tick::MAX as i64) as Tick;
        idx.push(out.len());
        out.push(c);
    }
    (out, idx)
}

pub fn with_velocity(notes: &[Note], selected: &[usize], velocity: u8) -> Vec<Note> {
    let mut out = notes.to_vec();
    for i in valid(notes, selected) {
        out[i].velocity = velocity.clamp(1, 127);
    }
    out
}
