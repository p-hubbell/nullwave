//! State-variable filter: response shapes, resonance and the bypass.

use nw_core::FilterKind;
use nw_engine::filter::Svf;

const SR: u32 = 48_000;

/// RMS of the filtered sine after the filter settles (left channel).
fn gain_at(kind: FilterKind, cutoff: f32, resonance: f32, freq: f32) -> f32 {
    let mut f = Svf::default();
    f.set(kind, cutoff, resonance, SR);
    let n = SR as usize / 2;
    let (mut sum_in, mut sum_out) = (0.0f64, 0.0f64);
    for i in 0..n {
        let x = (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin();
        let (y, _) = f.process(x, x);
        if i > n / 2 {
            sum_in += f64::from(x * x);
            sum_out += f64::from(y * y);
        }
    }
    (sum_out / sum_in).sqrt() as f32
}

#[test]
fn lowpass_attenuates_above_cutoff() {
    assert!(gain_at(FilterKind::LowPass, 1000.0, 0.0, 100.0) > 0.9);
    assert!(gain_at(FilterKind::LowPass, 1000.0, 0.0, 8000.0) < 0.05);
}

#[test]
fn highpass_attenuates_below_cutoff() {
    assert!(gain_at(FilterKind::HighPass, 1000.0, 0.0, 8000.0) > 0.9);
    assert!(gain_at(FilterKind::HighPass, 1000.0, 0.0, 100.0) < 0.05);
}

#[test]
fn bandpass_and_notch_shapes() {
    let bp_center = gain_at(FilterKind::BandPass, 1000.0, 0.5, 1000.0);
    assert!(bp_center > 0.9, "{bp_center}");
    assert!(gain_at(FilterKind::BandPass, 1000.0, 0.5, 60.0) < 0.2);
    assert!(gain_at(FilterKind::BandPass, 1000.0, 0.5, 15_000.0) < 0.2);
    assert!(gain_at(FilterKind::Notch, 1000.0, 0.0, 1000.0) < 0.05);
    assert!(gain_at(FilterKind::Notch, 1000.0, 0.0, 60.0) > 0.9);
    assert!(gain_at(FilterKind::Notch, 1000.0, 0.0, 15_000.0) > 0.9);
}

#[test]
fn resonance_peaks_at_cutoff() {
    let flat = gain_at(FilterKind::LowPass, 1000.0, 0.0, 1000.0);
    let peaky = gain_at(FilterKind::LowPass, 1000.0, 0.9, 1000.0);
    assert!(peaky > 2.0 * flat && peaky > 1.5, "{flat} → {peaky}");
}

#[test]
fn filter_off_is_bit_exact_passthrough() {
    let mut f = Svf::default();
    f.set(FilterKind::Off, 500.0, 1.0, SR);
    for x in [0.0f32, 0.123_456, -0.999, 1e-30, 0.5] {
        assert_eq!(f.process(x, -x), (x, -x));
    }
}

#[test]
fn extreme_settings_stay_finite() {
    for kind in [FilterKind::LowPass, FilterKind::HighPass, FilterKind::BandPass, FilterKind::Notch] {
        let mut f = Svf::default();
        // Above Nyquist and full resonance: clamped, never NaN or blowing up.
        f.set(kind, 30_000.0, 1.0, SR);
        for i in 0..SR {
            let x = if i % 2 == 0 { 1.0 } else { -1.0 };
            let (l, r) = f.process(x, x);
            assert!(l.is_finite() && r.is_finite() && l.abs() < 100.0);
        }
    }
}

#[test]
fn channels_are_independent() {
    let mut f = Svf::default();
    f.set(FilterKind::LowPass, 1000.0, 0.3, SR);
    let mut last = (0.0, 0.0);
    for _ in 0..100 {
        last = f.process(1.0, 0.0);
    }
    assert!(last.0 > 0.9);
    assert_eq!(last.1, 0.0);
}
