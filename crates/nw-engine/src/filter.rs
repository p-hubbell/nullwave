//! Stereo state-variable filter (Simper's trapezoidal "TPT" SVF, 12 dB/oct). Stable under
//! fast cutoff changes, so it can be modulated at control rate. Real-time safe.

use nw_core::FilterKind;

/// Highest cutoff as a fraction of the sample rate, kept below Nyquist.
const MAX_CUTOFF_FRACTION: f32 = 0.45;
const MIN_CUTOFF_HZ: f32 = 20.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    kind: FilterKind,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    /// Integrator state per channel: [ic1eq, ic2eq].
    state: [[f32; 2]; 2],
}

impl Svf {
    /// Sets the response. `resonance` 0..=1 maps Q from 0.707 (no peak) to about 18.
    pub fn set(&mut self, kind: FilterKind, cutoff_hz: f32, resonance: f32, sample_rate: u32) {
        self.kind = kind;
        if kind == FilterKind::Off {
            return;
        }
        let sr = sample_rate as f32;
        let fc = if cutoff_hz.is_finite() { cutoff_hz } else { MIN_CUTOFF_HZ };
        let fc = fc.clamp(MIN_CUTOFF_HZ, MAX_CUTOFF_FRACTION * sr);
        let res = if resonance.is_finite() { resonance.clamp(0.0, 1.0) } else { 0.0 };
        let q = std::f32::consts::FRAC_1_SQRT_2 * 25f32.powf(res);
        let g = (std::f32::consts::PI * fc / sr).tan();
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// Clears the filter memory (a new note starts from silence).
    pub fn reset(&mut self) {
        self.state = [[0.0; 2]; 2];
    }

    #[inline]
    fn tick(&mut self, ch: usize, v0: f32) -> f32 {
        let [ic1, ic2] = self.state[ch];
        let v3 = v0 - ic2;
        let v1 = self.a1 * ic1 + self.a2 * v3;
        let v2 = ic2 + self.a2 * ic1 + self.a3 * v3;
        self.state[ch] = [2.0 * v1 - ic1, 2.0 * v2 - ic2];
        let low = v2;
        let band = v1;
        let high = v0 - self.k * v1 - v2;
        match self.kind {
            FilterKind::Off => v0,
            FilterKind::LowPass => low,
            FilterKind::HighPass => high,
            // Normalised so the centre frequency passes at unity gain.
            FilterKind::BandPass => band * self.k,
            FilterKind::Notch => low + high,
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        if self.kind == FilterKind::Off {
            return (l, r);
        }
        (self.tick(0, l), self.tick(1, r))
    }
}
