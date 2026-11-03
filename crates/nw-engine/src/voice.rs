//! Sampler voice: plays a `SampleBuffer` with pitch, gain, pan and a linear
//! attack/release envelope. `render` is real-time safe (no allocation, locks,
//! I/O or panics).

use basedrop::Shared;

use crate::sample::SampleBuffer;

/// Constant-power pan. `pan` in -1.0 (left) ..= 1.0 (right); returns (left, right) gains.
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let theta = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    (theta.cos(), theta.sin())
}

/// Playback rate in source frames per output frame.
pub fn playback_rate(pitch: u8, root: u8, sample_rate: u32, output_rate: u32) -> f64 {
    2f64.powf((f64::from(pitch) - f64::from(root)) / 12.0) * f64::from(sample_rate)
        / f64::from(output_rate)
}

pub struct VoiceStart {
    pub sample: Shared<SampleBuffer>,
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
    pos: f64,
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

impl Voice {
    pub fn start(&mut self, s: VoiceStart) {
        let (pan_l, pan_r) = pan_gains(s.pan);
        self.sample = Some(s.sample);
        self.pos = 0.0;
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
        if self.remaining == Some(0) {
            self.release();
        }
    }

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
            for n in 0..frames {
                let i = self.pos as usize;
                if i + 1 >= len {
                    done = true;
                    break;
                }
                let frac = (self.pos - i as f64) as f32;
                let sl = buf.left[i] + (buf.left[i + 1] - buf.left[i]) * frac;
                let sr = buf.right[i] + (buf.right[i + 1] - buf.right[i]) * frac;

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
                self.pos += self.rate;

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
