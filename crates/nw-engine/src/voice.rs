//! Sampler voice: plays the `PlayRegion` of a `SampleBuffer` (with forward or ping-pong
//! looping) with pitch, gain, pan and a linear attack/release envelope. `render` is
//! real-time safe (no allocation, locks, I/O or panics).

use basedrop::Shared;
use nw_core::LoopMode;

use crate::process::PlayRegion;
use crate::sample::SampleBuffer;

/// Constant-power pan. `pan` in -1.0 (left) ..= 1.0 (right); returns (left, right) gains.
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let theta = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    (theta.cos(), theta.sin())
}

/// Playback rate in source frames per output frame.
pub fn playback_rate(pitch: u8, root: u8, sample_rate: u32, output_rate: u32) -> f64 {
    tuned_rate(pitch, root, 0, 0.0, sample_rate, output_rate)
}

/// `playback_rate` shifted by the instrument's coarse (semitones) and fine (cents) tune.
pub fn tuned_rate(pitch: u8, root: u8, coarse: i8, fine_cents: f32, sample_rate: u32, output_rate: u32) -> f64 {
    let semitones = f64::from(pitch) - f64::from(root) + f64::from(coarse) + f64::from(fine_cents) / 100.0;
    2f64.powf(semitones / 12.0) * f64::from(sample_rate) / f64::from(output_rate)
}

pub struct VoiceStart {
    pub sample: Shared<SampleBuffer>,
    /// The frames to play (and loop) within `sample`.
    pub region: PlayRegion,
    pub rate: f64,
    /// `velocity/127 * volume`.
    pub gain: f32,
    pub pan: f32,
    pub attack_samples: u32,
    pub release_samples: u32,
    /// Output frames until release begins; `None` for previews.
    pub duration_samples: Option<u64>,
    pub age: u64,
}

#[derive(Default)]
pub struct Voice {
    sample: Option<Shared<SampleBuffer>>,
    region: PlayRegion,
    pos: f64,
    /// +1 forward, −1 backward (ping-pong).
    dir: f64,
    rate: f64,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    attack_samples: u32,
    release_samples: u32,
    remaining: Option<u64>,
    age: u64,
    attack_pos: u32,
    release_pos: u32,
    releasing: bool,
    /// Envelope level of the most recently rendered frame.
    level: f32,
    release_from: f32,
}

impl Default for PlayRegion {
    fn default() -> Self {
        PlayRegion::whole(0)
    }
}

impl Voice {
    pub fn start(&mut self, s: VoiceStart) {
        let (pan_l, pan_r) = pan_gains(s.pan);
        let len = s.sample.left.len().min(s.sample.right.len());
        self.sample = Some(s.sample);
        self.region = s.region;
        self.pos = s.region.start as f64;
        self.dir = 1.0;
        self.rate = s.rate;
        self.gain = s.gain;
        self.pan_l = pan_l;
        self.pan_r = pan_r;
        self.attack_samples = s.attack_samples;
        self.release_samples = s.release_samples;
        self.remaining = s.duration_samples;
        self.age = s.age;
        self.attack_pos = 0;
        self.release_pos = 0;
        self.releasing = false;
        self.level = 0.0;
        self.release_from = 0.0;
        if s.region.start >= s.region.end.min(len) {
            // Nothing to play.
            self.sample = None;
            return;
        }
        if self.remaining == Some(0) {
            self.release();
        }
    }

}

/// Moves a read position on by one output frame, looping inside `r`.
fn advance(pos: &mut f64, dir: &mut f64, rate: f64, r: &PlayRegion) {
    *pos += rate * *dir;
    if !r.is_looping() {
        return;
    }
    let (ls, le) = (r.loop_start as f64, r.loop_end as f64);
    match r.loop_mode {
        LoopMode::Off => {}
        LoopMode::Forward => {
            if *pos >= le {
                *pos = ls + (*pos - le) % (le - ls);
            }
        }
        LoopMode::PingPong => {
            // Turn around at the loop edges; `loop_end` itself is played.
            if *dir > 0.0 && *pos > le {
                *pos = (2.0 * le - *pos).max(ls);
                *dir = -1.0;
            } else if *dir < 0.0 && *pos < ls {
                *pos = (2.0 * ls - *pos).min(le);
                *dir = 1.0;
            }
        }
    }
}

impl Voice {

    /// Begin the release stage from the current envelope level.
    pub fn release(&mut self) {
        if self.sample.is_none() || self.releasing {
            return;
        }
        if self.release_samples == 0 {
            self.sample = None;
            return;
        }
        self.releasing = true;
        self.release_pos = 0;
        self.release_from = self.level;
    }

    pub fn is_active(&self) -> bool {
        self.sample.is_some()
    }

    pub fn age(&self) -> u64 {
        self.age
    }

    /// Add up to `min(out_l.len(), out_r.len())` frames into the outputs.
    pub fn render(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let frames = out_l.len().min(out_r.len());
        let mut done = false;
        if let Some(buf) = self.sample.as_ref() {
            let len = buf.left.len().min(buf.right.len());
            let r = self.region;
            let looping = r.is_looping();
            let forward_loop = looping && r.loop_mode == LoopMode::Forward;
            for n in 0..frames {
                let i = self.pos as usize;
                if len == 0 || (!looping && i + 1 >= r.end.min(len)) {
                    done = true;
                    break;
                }
                // In a forward loop the frame after the last one is the loop start.
                let next = if forward_loop && i + 1 >= r.loop_end { r.loop_start } else { i + 1 };
                let (i0, i1) = (i.min(len - 1), next.min(len - 1));
                let frac = (self.pos - i as f64) as f32;
                let sl = buf.left[i0] + (buf.left[i1] - buf.left[i0]) * frac;
                let sr = buf.right[i0] + (buf.right[i1] - buf.right[i0]) * frac;

                let env = if self.releasing {
                    if self.release_pos >= self.release_samples {
                        done = true;
                        break;
                    }
                    let e = self.release_from
                        * (1.0 - self.release_pos as f32 / self.release_samples as f32);
                    self.release_pos += 1;
                    e
                } else if self.attack_pos < self.attack_samples {
                    let e = self.attack_pos as f32 / self.attack_samples as f32;
                    self.attack_pos += 1;
                    e
                } else {
                    1.0
                };
                self.level = env;

                let g = self.gain * env;
                out_l[n] += sl * g * self.pan_l;
                out_r[n] += sr * g * self.pan_r;
                advance(&mut self.pos, &mut self.dir, self.rate, &r);

                if self.releasing {
                    if self.release_pos >= self.release_samples {
                        done = true;
                        break;
                    }
                } else if let Some(rem) = self.remaining.as_mut() {
                    *rem -= 1;
                    if *rem == 0 {
                        // Release begins from the level just rendered.
                        self.remaining = None;
                        if self.release_samples == 0 {
                            done = true;
                            break;
                        }
                        self.releasing = true;
                        self.release_pos = 0;
                        self.release_from = env;
                    }
                }
            }
        }
        if done {
            self.sample = None;
        }
    }
}
