//! Offline engine core: transport, sample-accurate scheduler and voice pool.
//!
//! Everything reachable from `EngineCore::handle`, `EngineCore::process` and
//! `run_block` is real-time safe: no allocation, locks, I/O, logging or panics.
//! All buffers are preallocated in `EngineCore::new`.

use basedrop::{Owned, Shared};
use nw_core::{PPQ, Tick};

use crate::sample::SampleBuffer;
use crate::snapshot::{NoteEvent, Snapshot};
use crate::voice::{Voice, VoiceStart, playback_rate};

pub const MAX_VOICES: usize = 64;
/// Largest number of frames rendered in one pass; longer `out` slices are chunked.
pub const MAX_BLOCK_FRAMES: usize = 4096;

const PREVIEW_GAIN: f32 = 0.78;
const PREVIEW_RELEASE_MS: f32 = 20.0;
/// Tolerance (in frames) for float error when locating a tick inside a block.
const FRAME_EPSILON: f64 = 1e-6;

pub enum Command {
    SetSnapshot(Owned<Snapshot>),
    Play,
    Stop,
    Preview(Shared<SampleBuffer>),
    StopPreview,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Feedback {
    Playhead { tick: Tick, playing: bool },
    Load(f32),
}

pub struct EngineCore {
    output_rate: u32,
    snapshot: Owned<Snapshot>,
    playing: bool,
    /// Transport position in ticks.
    position: f64,
    /// Index of the next event to fire in `snapshot.events`.
    cursor: usize,
    voices: Vec<Voice>,
    next_age: u64,
    /// `(voice index, age)` of the current preview; the age guards against stealing.
    preview: Option<(usize, u64)>,
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
}

impl EngineCore {
    pub fn new(output_rate: u32, initial: Owned<Snapshot>) -> EngineCore {
        EngineCore {
            output_rate,
            snapshot: initial,
            playing: false,
            position: 0.0,
            cursor: 0,
            voices: (0..MAX_VOICES).map(|_| Voice::default()).collect(),
            next_age: 0,
            preview: None,
            scratch_l: vec![0.0; MAX_BLOCK_FRAMES],
            scratch_r: vec![0.0; MAX_BLOCK_FRAMES],
        }
    }

    pub fn handle(&mut self, command: Command) {
        match command {
            Command::SetSnapshot(snapshot) => {
                // Events already fired were scheduled at the old tempo.
                let fired_spt = self.samples_per_tick();
                // The old snapshot's drop is deferred to the basedrop collector.
                self.snapshot = snapshot;
                self.position %= f64::from(self.loop_length());
                self.cursor = self.cursor_for(self.position, fired_spt);
            }
            Command::Play => {
                self.playing = true;
                self.position = 0.0;
                self.cursor = 0;
            }
            Command::Stop => {
                for v in &mut self.voices {
                    v.release();
                }
                self.preview = None;
                self.playing = false;
                self.position = 0.0;
                self.cursor = 0;
            }
            Command::Preview(sample) => {
                self.release_preview();
                let rate = f64::from(sample.sample_rate) / f64::from(self.output_rate);
                let start = VoiceStart {
                    sample,
                    rate,
                    gain: PREVIEW_GAIN,
                    pan: 0.0,
                    attack_samples: 0,
                    release_samples: self.ms_to_samples(PREVIEW_RELEASE_MS),
                    duration_samples: None,
                    age: 0,
                };
                self.preview = Some(self.start_voice(start));
            }
            Command::StopPreview => self.release_preview(),
        }
    }

    /// Render interleaved stereo into `out`, overwriting it.
    pub fn process(&mut self, out: &mut [f32]) {
        let (frames, rest) = out.split_at_mut(out.len() / 2 * 2);
        rest.fill(0.0);
        for chunk in frames.chunks_mut(MAX_BLOCK_FRAMES * 2) {
            let n = chunk.len() / 2;
            self.scratch_l[..n].fill(0.0);
            self.scratch_r[..n].fill(0.0);
            self.render_chunk(n);
            for (i, frame) in chunk.chunks_exact_mut(2).enumerate() {
                frame[0] = self.scratch_l[i];
                frame[1] = self.scratch_r[i];
            }
        }
    }

    pub fn playhead(&self) -> Tick {
        self.position as Tick
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    fn loop_length(&self) -> Tick {
        self.snapshot.loop_length.max(1)
    }

    fn samples_per_tick(&self) -> f64 {
        let tempo = if self.snapshot.tempo > 0.0 { self.snapshot.tempo } else { 120.0 };
        f64::from(self.output_rate) * 60.0 / (tempo * f64::from(PPQ))
    }

    fn ms_to_samples(&self, ms: f32) -> u32 {
        (f64::from(ms.max(0.0)) * f64::from(self.output_rate) / 1000.0) as u32
    }

    /// First event not yet fired at `position`. Matches the firing rule in
    /// `render_chunk`: an event fires on the first frame where `frames_until` is 0,
    /// and the position then advances at least one frame. So an event counts as
    /// fired only once the position is a full frame past its tick; one that is
    /// less than a frame behind is still pending (possible when samples-per-tick
    /// is fractional and a block ends exactly at its firing frame).
    fn cursor_for(&self, position: f64, spt: f64) -> usize {
        self.snapshot
            .events
            .partition_point(|e| (position - f64::from(e.tick)) * spt >= 1.0 - FRAME_EPSILON)
    }

    /// Frames from the current position until `tick` is reached (0 if already reached).
    fn frames_until(&self, tick: Tick, spt: f64) -> usize {
        let f = ((f64::from(tick) - self.position) * spt - FRAME_EPSILON).ceil();
        if f > 0.0 { f as usize } else { 0 }
    }

    /// Render `n <= MAX_BLOCK_FRAMES` frames into the scratch buffers, firing
    /// events at their exact frame and wrapping at the loop end.
    fn render_chunk(&mut self, n: usize) {
        if !self.playing {
            self.render_voices(0, n);
            return;
        }
        let spt = self.samples_per_tick();
        let loop_length = self.loop_length();
        let mut done = 0;
        while done < n {
            while let Some(&event) = self.snapshot.events.get(self.cursor) {
                if event.tick >= loop_length || self.frames_until(event.tick, spt) > 0 {
                    break;
                }
                self.fire(event, spt);
                self.cursor += 1;
            }
            if self.wrap_at_loop_end(loop_length, spt) {
                continue;
            }
            let to_loop_end = self.frames_until(loop_length, spt);
            let to_event = match self.snapshot.events.get(self.cursor) {
                Some(e) if e.tick < loop_length => self.frames_until(e.tick, spt),
                _ => usize::MAX,
            };
            let seg = (n - done).min(to_loop_end).min(to_event);
            self.render_voices(done, done + seg);
            self.position += seg as f64 / spt;
            done += seg;
            // Wrap eagerly so the playhead never reports `loop_length`.
            self.wrap_at_loop_end(loop_length, spt);
        }
    }

    /// Wrap to the loop start if the loop end has been reached. Returns whether it wrapped.
    fn wrap_at_loop_end(&mut self, loop_length: Tick, spt: f64) -> bool {
        if self.frames_until(loop_length, spt) > 0 {
            return false;
        }
        self.position = (self.position - f64::from(loop_length)).max(0.0);
        self.cursor = 0;
        true
    }

    fn render_voices(&mut self, from: usize, to: usize) {
        let l = &mut self.scratch_l[from..to];
        let r = &mut self.scratch_r[from..to];
        for v in &mut self.voices {
            if v.is_active() {
                v.render(l, r);
            }
        }
    }

    fn fire(&mut self, event: NoteEvent, spt: f64) {
        let Some(inst) = self.snapshot.instruments.get(event.instrument) else { return };
        let Some(sample) = inst.sample.as_ref() else { return };
        let start = VoiceStart {
            sample: sample.clone(),
            rate: playback_rate(event.pitch, inst.root_note, sample.sample_rate, self.output_rate),
            gain: f32::from(event.velocity) / 127.0 * inst.volume,
            pan: inst.pan,
            attack_samples: self.ms_to_samples(inst.attack_ms),
            release_samples: self.ms_to_samples(inst.release_ms),
            duration_samples: Some((f64::from(event.length) * spt).round() as u64),
            age: 0,
        };
        self.start_voice(start);
    }

    /// Start a voice in a free slot, or steal the oldest. Returns `(index, age)`.
    fn start_voice(&mut self, mut start: VoiceStart) -> (usize, u64) {
        let index = match self.voices.iter().position(|v| !v.is_active()) {
            Some(i) => i,
            None => {
                let mut oldest = 0;
                for (i, v) in self.voices.iter().enumerate() {
                    if v.age() < self.voices[oldest].age() {
                        oldest = i;
                    }
                }
                oldest
            }
        };
        let age = self.next_age;
        self.next_age += 1;
        start.age = age;
        self.voices[index].start(start);
        (index, age)
    }

    fn release_preview(&mut self) {
        if let Some((index, age)) = self.preview.take() {
            let v = &mut self.voices[index];
            if v.is_active() && v.age() == age {
                v.release();
            }
        }
    }
}

/// One audio callback: apply all pending commands, render `out`, report the playhead.
pub fn run_block(
    core: &mut EngineCore,
    commands: &mut rtrb::Consumer<Command>,
    feedback: &mut rtrb::Producer<Feedback>,
    out: &mut [f32],
) {
    while let Ok(command) = commands.pop() {
        core.handle(command);
    }
    core.process(out);
    // A full feedback queue just drops this update.
    let _ = feedback.push(Feedback::Playhead { tick: core.playhead(), playing: core.is_playing() });
}
