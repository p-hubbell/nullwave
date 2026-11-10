//! Low-frequency oscillator: sine, triangle, saw, square and sample-and-hold random, at a
//! free or tempo-synced rate, with an onset delay. Real-time safe (the random source is a
//! per-voice xorshift, no allocation).

use nw_core::{Lfo, LfoRate, LfoShape};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LfoParams {
    pub shape: LfoShape,
    pub hz: f32,
    /// Samples of silence (output 0) before the LFO starts.
    pub delay: u32,
}

impl Default for LfoParams {
    fn default() -> Self {
        LfoParams { shape: LfoShape::Sine, hz: 0.0, delay: 0 }
    }
}

/// The LFO frequency for `rate` at `tempo` (BPM, quarter notes per minute).
pub fn lfo_hz(rate: LfoRate, tempo: f64) -> f32 {
    match rate {
        LfoRate::Hz(hz) => hz,
        LfoRate::Sync { numerator, denominator } => {
            // One cycle lasts numerator/denominator whole notes = 4 × that many beats.
            let whole_notes = f64::from(numerator.max(1)) / f64::from(denominator.max(1));
            let seconds = whole_notes * 4.0 * 60.0 / tempo.max(1.0);
            (1.0 / seconds) as f32
        }
    }
}

impl LfoParams {
    pub fn from_lfo(lfo: &Lfo, tempo: f64, sample_rate: u32) -> LfoParams {
        let hz = lfo_hz(lfo.rate, tempo);
        let delay_ms = if lfo.delay_ms.is_finite() { lfo.delay_ms.max(0.0) } else { 0.0 };
        LfoParams {
            shape: lfo.shape,
            hz: if hz.is_finite() { hz.clamp(0.0, 1000.0) } else { 0.0 },
            delay: (f64::from(delay_ms) * f64::from(sample_rate) / 1000.0) as u32,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LfoState {
    shape: LfoShape,
    /// 0..1 through the current cycle.
    phase: f64,
    inc: f64,
    delay_left: u32,
    rng: u32,
    held: f32,
}

impl Default for LfoState {
    fn default() -> Self {
        LfoState { shape: LfoShape::Sine, phase: 0.0, inc: 0.0, delay_left: 0, rng: 1, held: 0.0 }
    }
}

impl LfoState {
    /// Restarts at phase 0. `seed` varies the random shape between voices.
    pub fn start(&mut self, params: LfoParams, sample_rate: u32, seed: u32) {
        self.shape = params.shape;
        self.phase = 0.0;
        self.inc = f64::from(params.hz) / f64::from(sample_rate.max(1));
        self.delay_left = params.delay;
        // xorshift must not start at 0.
        self.rng = seed.wrapping_mul(0x9E37_79B9) | 1;
        self.held = self.random();
    }

    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn value(&self) -> f32 {
        let p = self.phase as f32;
        match self.shape {
            LfoShape::Sine => (p * std::f32::consts::TAU).sin(),
            // −1 at phase 0, +1 at phase 0.5.
            LfoShape::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            LfoShape::Saw => 2.0 * p - 1.0,
            LfoShape::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::Random => self.held,
        }
    }

    /// The value for this sample (−1..=1, or 0 during the delay); then advances one sample.
    pub fn tick(&mut self) -> f32 {
        self.step(1)
    }

    /// The value now; then advances `n` samples (for control-rate updates).
    pub fn step(&mut self, n: u32) -> f32 {
        let mut n = n;
        if self.delay_left > 0 {
            let waited = n.min(self.delay_left);
            self.delay_left -= waited;
            n -= waited;
            if n == 0 || self.delay_left > 0 {
                return 0.0;
            }
            // The delay ended inside this step: start from phase 0 now.
            let v = self.value();
            self.advance(n);
            return v;
        }
        let v = self.value();
        self.advance(n);
        v
    }

    fn advance(&mut self, n: u32) {
        self.phase += self.inc * f64::from(n);
        if self.phase >= 1.0 {
            self.phase = self.phase.fract();
            // Each new cycle draws a new held value (several if the step spans many cycles,
            // but one is enough to sound random).
            self.held = self.random();
        }
    }
}
