//! DAHDSR envelope stepping one sample at a time. Real-time safe: plain arithmetic on
//! `Copy` state. With no delay, hold or decay and full sustain it reproduces the original
//! linear attack/release envelope sample for sample.

use nw_core::Envelope;

/// Stage lengths in samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvParams {
    pub delay: u32,
    pub attack: u32,
    pub hold: u32,
    pub decay: u32,
    /// 0..=1.
    pub sustain: f32,
    pub release: u32,
    /// −1..=1; 0 is linear.
    pub attack_curve: f32,
    /// Applies to decay and release.
    pub decay_curve: f32,
}

impl Default for EnvParams {
    fn default() -> Self {
        EnvParams::attack_release(0, 0)
    }
}

impl EnvParams {
    /// Only attack and release, full sustain: the original sampler envelope.
    pub fn attack_release(attack: u32, release: u32) -> EnvParams {
        EnvParams { delay: 0, attack, hold: 0, decay: 0, sustain: 1.0, release, attack_curve: 0.0, decay_curve: 0.0 }
    }

    /// Converts a model envelope (milliseconds), clamping anything out of range.
    pub fn from_envelope(e: &Envelope, sample_rate: u32) -> EnvParams {
        let samples = |ms: f32| {
            let ms = if ms.is_finite() { ms.max(0.0) } else { 0.0 };
            (f64::from(ms) * f64::from(sample_rate) / 1000.0) as u32
        };
        let unit = |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        EnvParams {
            delay: samples(e.delay_ms),
            attack: samples(e.attack_ms),
            hold: samples(e.hold_ms),
            decay: samples(e.decay_ms),
            sustain: unit(e.sustain, 0.0, 1.0, 1.0),
            release: samples(e.release_ms),
            attack_curve: unit(e.attack_curve, -1.0, 1.0, 0.0),
            decay_curve: unit(e.decay_curve, -1.0, 1.0, 0.0),
        }
    }
}

/// Bends progress `t` (0..=1) through a stage. 0 is linear; negative rises fast then
/// slowly, positive slowly then fast. Endpoints stay at 0 and 1.
pub fn shape(t: f32, curve: f32) -> f32 {
    if curve == 0.0 {
        return t;
    }
    // Exponent 1/4 (curve −1) to 4 (curve +1).
    t.clamp(0.0, 1.0).powf(4f32.powf(curve))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    #[default]
    Done,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnvState {
    params: EnvParams,
    stage: Stage,
    /// Samples spent in the current stage.
    pos: u32,
    /// Level of the most recent sample.
    level: f32,
    release_from: f32,
}

impl EnvState {
    pub fn start(&mut self, params: EnvParams) {
        self.params = params;
        self.level = 0.0;
        self.release_from = 0.0;
        self.enter(Stage::Delay);
    }

    /// Enters `stage`, skipping any of zero length.
    fn enter(&mut self, mut stage: Stage) {
        let p = self.params;
        loop {
            let len = match stage {
                Stage::Delay => p.delay,
                Stage::Attack => p.attack,
                Stage::Hold => p.hold,
                Stage::Decay => p.decay,
                Stage::Release => p.release,
                Stage::Sustain | Stage::Done => u32::MAX,
            };
            if len > 0 {
                break;
            }
            stage = match stage {
                Stage::Delay => Stage::Attack,
                Stage::Attack => Stage::Hold,
                Stage::Hold => Stage::Decay,
                Stage::Decay => Stage::Sustain,
                Stage::Release => Stage::Done,
                s => s,
            };
        }
        self.stage = stage;
        self.pos = 0;
    }

    /// Starts the release from the current level (from any stage).
    pub fn release(&mut self) {
        if matches!(self.stage, Stage::Release | Stage::Done) {
            return;
        }
        self.release_from = self.level;
        self.enter(Stage::Release);
    }

    /// The level for this sample; then moves on one sample.
    pub fn tick(&mut self) -> f32 {
        let p = self.params;
        let progress = |pos: u32, len: u32| pos as f32 / len as f32;
        let (value, len) = match self.stage {
            Stage::Delay => (0.0, p.delay),
            Stage::Attack => (shape(progress(self.pos, p.attack), p.attack_curve), p.attack),
            Stage::Hold => (1.0, p.hold),
            Stage::Decay => {
                (1.0 + (p.sustain - 1.0) * shape(progress(self.pos, p.decay), p.decay_curve), p.decay)
            }
            Stage::Sustain => (p.sustain, u32::MAX),
            Stage::Release => {
                // Decay-curve shaping of the fall, starting from wherever the release began.
                let t = shape(progress(self.pos, p.release), p.decay_curve);
                (self.release_from * (1.0 - t), p.release)
            }
            Stage::Done => (0.0, u32::MAX),
        };
        self.level = value;
        if len != u32::MAX {
            self.pos += 1;
            if self.pos >= len {
                let next = match self.stage {
                    Stage::Delay => Stage::Attack,
                    Stage::Attack => Stage::Hold,
                    Stage::Hold => Stage::Decay,
                    Stage::Decay => Stage::Sustain,
                    _ => Stage::Done,
                };
                // Release stays put at its end so `is_done` reports it; `Done` is final.
                if self.stage == Stage::Release {
                    self.stage = Stage::Done;
                } else {
                    self.enter(next);
                }
            }
        }
        value
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    /// True once the release has finished (or before `start`).
    pub fn is_done(&self) -> bool {
        self.stage == Stage::Done
    }

    pub fn is_releasing(&self) -> bool {
        self.stage == Stage::Release
    }
}
