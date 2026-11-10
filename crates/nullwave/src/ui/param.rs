//! One table describing every sampler slider: label, how to read and write the value on
//! an `Instrument`, its range (from `nw_core::limits`), how the track maps to values, and
//! how to show it. The instrument tab's sliders are generic over `ParamSpec`.

use nw_core::Instrument;
use nw_core::limits;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Linear,
    /// Squared: more of the track for small values (times).
    Squared,
    /// Logarithmic (frequencies); `min` must be above 0.
    Log,
}

#[derive(Clone, Copy)]
pub struct ParamSpec {
    /// Unique, stable name (the slider's element id).
    pub key: &'static str,
    pub label: &'static str,
    pub get: fn(&Instrument) -> f32,
    pub set: fn(&mut Instrument, f32),
    pub min: f32,
    pub max: f32,
    pub scale: Scale,
    /// Values snap to multiples of this (0 = no rounding).
    pub step: f32,
    pub format: fn(f32) -> String,
}

impl ParamSpec {
    /// Track position (0..=1) to a value in range.
    pub fn value_at(&self, t: f32) -> f32 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        let raw = match self.scale {
            Scale::Linear => self.min + t * (self.max - self.min),
            Scale::Squared => self.min + t * t * (self.max - self.min),
            Scale::Log => self.min * (self.max / self.min).powf(t),
        };
        let stepped = if self.step > 0.0 { (raw / self.step).round() * self.step } else { raw };
        // Snap to 0 when 0 is inside a bipolar range and the value is within a step of it.
        let snapped = if self.min < 0.0 && self.max > 0.0 && stepped.abs() < self.step * 0.5 { 0.0 } else { stepped };
        snapped.clamp(self.min, self.max)
    }

    /// Value to track position (0..=1).
    pub fn fraction_of(&self, value: f32) -> f32 {
        let v = if value.is_finite() { value.clamp(self.min, self.max) } else { self.min };
        let span = self.max - self.min;
        let t = match self.scale {
            Scale::Linear => (v - self.min) / span,
            Scale::Squared => ((v - self.min) / span).max(0.0).sqrt(),
            Scale::Log => (v / self.min).ln() / (self.max / self.min).ln(),
        };
        if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
    }
}

const fn lo(r: std::ops::RangeInclusive<f32>) -> f32 {
    *r.start()
}
const fn hi(r: std::ops::RangeInclusive<f32>) -> f32 {
    *r.end()
}

pub fn fmt_ms(v: f32) -> String {
    if v >= 1000.0 { format!("{:.2} s", v / 1000.0) } else { format!("{v:.0} ms") }
}
pub fn fmt_percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}
pub fn fmt_pan(v: f32) -> String {
    let amount = (v.abs() * 100.0).round();
    if amount == 0.0 {
        "C".into()
    } else if v < 0.0 {
        format!("L{amount:.0}")
    } else {
        format!("R{amount:.0}")
    }
}
pub fn fmt_signed_percent(v: f32) -> String {
    format!("{:+.0}%", v * 100.0)
}
pub fn fmt_octaves(v: f32) -> String {
    format!("{v:+.2} oct")
}
pub fn fmt_semitones(v: f32) -> String {
    format!("{v:+.2} st")
}
pub fn fmt_cents(v: f32) -> String {
    format!("{v:+.0} ct")
}
pub fn fmt_hz(v: f32) -> String {
    if v >= 1000.0 {
        format!("{:.2} kHz", v / 1000.0)
    } else if v >= 10.0 {
        format!("{v:.0} Hz")
    } else {
        format!("{v:.2} Hz")
    }
}
pub fn fmt_curve(v: f32) -> String {
    if v.abs() < 0.005 { "linear".into() } else { format!("{v:+.2}") }
}

macro_rules! spec {
    ($key:expr, $label:expr, $($field:ident).+, $min:expr, $max:expr, $scale:expr, $step:expr, $fmt:expr) => {
        ParamSpec {
            key: $key,
            label: $label,
            get: |i: &Instrument| i.$($field).+,
            set: |i: &mut Instrument, v: f32| i.$($field).+ = v,
            min: $min,
            max: $max,
            scale: $scale,
            step: $step,
            format: $fmt,
        }
    };
}

pub const VOLUME: ParamSpec = spec!("volume", "Volume", volume, 0.0, 1.0, Scale::Linear, 0.01, fmt_percent);
pub const PAN: ParamSpec = spec!("pan", "Pan", pan, -1.0, 1.0, Scale::Linear, 0.01, fmt_pan);
pub const CROSSFADE: ParamSpec = spec!(
    "crossfade", "Crossfade", sampler.looping.crossfade_ms,
    lo(limits::CROSSFADE_MS), hi(limits::CROSSFADE_MS), Scale::Squared, 1.0, fmt_ms
);
pub const FINE_TUNE: ParamSpec = spec!(
    "fine", "Fine tune", sampler.tune.fine_cents,
    lo(limits::FINE_CENTS), hi(limits::FINE_CENTS), Scale::Linear, 1.0, fmt_cents
);
pub const CUTOFF: ParamSpec = spec!(
    "cutoff", "Cutoff", sampler.filter.cutoff_hz,
    lo(limits::CUTOFF_HZ), hi(limits::CUTOFF_HZ), Scale::Log, 0.0, fmt_hz
);
pub const RESONANCE: ParamSpec =
    spec!("resonance", "Resonance", sampler.filter.resonance, 0.0, 1.0, Scale::Linear, 0.01, fmt_percent);
pub const VEL_TO_VOLUME: ParamSpec =
    spec!("vel_volume", "To volume", sampler.velocity.to_volume, 0.0, 1.0, Scale::Linear, 0.01, fmt_percent);
pub const VEL_TO_CUTOFF: ParamSpec = spec!(
    "vel_cutoff", "To cutoff", sampler.velocity.to_cutoff_octaves,
    lo(limits::OCTAVES), hi(limits::OCTAVES), Scale::Linear, 0.05, fmt_octaves
);
pub const GLIDE: ParamSpec = spec!(
    "glide", "Glide", sampler.voice.glide_ms,
    lo(limits::GLIDE_MS), hi(limits::GLIDE_MS), Scale::Squared, 1.0, fmt_ms
);

/// The eight sliders of one DAHDSR envelope.
macro_rules! env_specs {
    ($prefix:literal, $($env:ident).+) => {
        [
            spec!(concat!($prefix, "_delay"), "Delay", sampler.$($env).+.delay_ms,
                lo(limits::DELAY_HOLD_MS), hi(limits::DELAY_HOLD_MS), Scale::Squared, 1.0, fmt_ms),
            spec!(concat!($prefix, "_attack"), "Attack", sampler.$($env).+.attack_ms,
                lo(limits::STAGE_MS), hi(limits::STAGE_MS), Scale::Squared, 1.0, fmt_ms),
            spec!(concat!($prefix, "_hold"), "Hold", sampler.$($env).+.hold_ms,
                lo(limits::DELAY_HOLD_MS), hi(limits::DELAY_HOLD_MS), Scale::Squared, 1.0, fmt_ms),
            spec!(concat!($prefix, "_decay"), "Decay", sampler.$($env).+.decay_ms,
                lo(limits::STAGE_MS), hi(limits::STAGE_MS), Scale::Squared, 1.0, fmt_ms),
            spec!(concat!($prefix, "_sustain"), "Sustain", sampler.$($env).+.sustain,
                0.0, 1.0, Scale::Linear, 0.01, fmt_percent),
            spec!(concat!($prefix, "_release"), "Release", sampler.$($env).+.release_ms,
                lo(limits::STAGE_MS), hi(limits::STAGE_MS), Scale::Squared, 1.0, fmt_ms),
            spec!(concat!($prefix, "_attack_curve"), "Attack curve", sampler.$($env).+.attack_curve,
                lo(limits::CURVE), hi(limits::CURVE), Scale::Linear, 0.01, fmt_curve),
            spec!(concat!($prefix, "_decay_curve"), "Decay curve", sampler.$($env).+.decay_curve,
                lo(limits::CURVE), hi(limits::CURVE), Scale::Linear, 0.01, fmt_curve),
        ]
    };
}

pub const VOLUME_ENV: [ParamSpec; 8] = env_specs!("vol_env", volume_env);
pub const CUTOFF_ENV: [ParamSpec; 8] = env_specs!("cut_env", cutoff_env.env);
pub const PITCH_ENV: [ParamSpec; 8] = env_specs!("pitch_env", pitch_env.env);
pub const PAN_ENV: [ParamSpec; 8] = env_specs!("pan_env", pan_env.env);

pub const CUTOFF_ENV_AMOUNT: ParamSpec = spec!(
    "cut_env_amount", "Amount", sampler.cutoff_env.amount,
    lo(limits::OCTAVES), hi(limits::OCTAVES), Scale::Linear, 0.05, fmt_octaves
);
pub const PITCH_ENV_AMOUNT: ParamSpec = spec!(
    "pitch_env_amount", "Amount", sampler.pitch_env.amount,
    lo(limits::SEMITONES), hi(limits::SEMITONES), Scale::Linear, 0.1, fmt_semitones
);
pub const PAN_ENV_AMOUNT: ParamSpec = spec!(
    "pan_env_amount", "Amount", sampler.pan_env.amount,
    lo(limits::PAN), hi(limits::PAN), Scale::Linear, 0.01, fmt_signed_percent
);

/// Amount and delay of one LFO; the rate has its own control (free Hz or synced).
macro_rules! lfo_specs {
    ($prefix:literal, $lfo:ident, $min:expr, $max:expr, $step:expr, $fmt:expr) => {
        [
            spec!(concat!($prefix, "_amount"), "Amount", sampler.$lfo.amount, $min, $max, Scale::Linear, $step, $fmt),
            spec!(concat!($prefix, "_delay"), "Delay", sampler.$lfo.delay_ms,
                lo(limits::LFO_DELAY_MS), hi(limits::LFO_DELAY_MS), Scale::Squared, 1.0, fmt_ms),
        ]
    };
}

pub const VOLUME_LFO: [ParamSpec; 2] = lfo_specs!("vol_lfo", volume_lfo, 0.0, 1.0, 0.01, fmt_percent);
pub const PAN_LFO: [ParamSpec; 2] = lfo_specs!("pan_lfo", pan_lfo, 0.0, 1.0, 0.01, fmt_percent);
pub const CUTOFF_LFO: [ParamSpec; 2] =
    lfo_specs!("cut_lfo", cutoff_lfo, lo(limits::OCTAVES), hi(limits::OCTAVES), 0.05, fmt_octaves);
pub const PITCH_LFO: [ParamSpec; 2] =
    lfo_specs!("pitch_lfo", pitch_lfo, lo(limits::SEMITONES), hi(limits::SEMITONES), 0.1, fmt_semitones);

/// Free-running LFO rate (only meaningful while the LFO is not tempo-synced). Reads and
/// writes the `Hz` variant; a synced rate reads as the default 4 Hz.
macro_rules! lfo_hz_spec {
    ($key:literal, $lfo:ident) => {
        ParamSpec {
            key: $key,
            label: "Rate",
            get: |i: &Instrument| match i.sampler.$lfo.rate {
                nw_core::LfoRate::Hz(hz) => hz,
                nw_core::LfoRate::Sync { .. } => 4.0,
            },
            set: |i: &mut Instrument, v: f32| i.sampler.$lfo.rate = nw_core::LfoRate::Hz(v),
            min: lo(limits::LFO_HZ),
            max: hi(limits::LFO_HZ),
            scale: Scale::Log,
            step: 0.0,
            format: fmt_hz,
        }
    };
}

pub const VOLUME_LFO_HZ: ParamSpec = lfo_hz_spec!("vol_lfo_hz", volume_lfo);
pub const PAN_LFO_HZ: ParamSpec = lfo_hz_spec!("pan_lfo_hz", pan_lfo);
pub const CUTOFF_LFO_HZ: ParamSpec = lfo_hz_spec!("cut_lfo_hz", cutoff_lfo);
pub const PITCH_LFO_HZ: ParamSpec = lfo_hz_spec!("pitch_lfo_hz", pitch_lfo);

/// Tempo-synced LFO periods offered by the stepper, as whole-note fractions.
pub const SYNC_VALUES: [(u16, u16); 10] =
    [(1, 64), (1, 32), (1, 16), (1, 8), (1, 4), (1, 2), (1, 1), (2, 1), (4, 1), (8, 1)];

pub fn fmt_sync(numerator: u16, denominator: u16) -> String {
    match (numerator, denominator) {
        (1, 1) => "1 bar".into(),
        (n, 1) => format!("{n} bars"),
        (n, d) => format!("{n}/{d}"),
    }
}

/// Every spec, for tests.
#[cfg(test)]
pub fn all() -> Vec<ParamSpec> {
    let mut v = vec![
        VOLUME, PAN, CROSSFADE, FINE_TUNE, CUTOFF, RESONANCE, VEL_TO_VOLUME, VEL_TO_CUTOFF, GLIDE,
        CUTOFF_ENV_AMOUNT, PITCH_ENV_AMOUNT, PAN_ENV_AMOUNT, VOLUME_LFO_HZ, PAN_LFO_HZ, CUTOFF_LFO_HZ,
        PITCH_LFO_HZ,
    ];
    for group in [VOLUME_ENV, CUTOFF_ENV, PITCH_ENV, PAN_ENV] {
        v.extend(group);
    }
    for group in [VOLUME_LFO, PAN_LFO, CUTOFF_LFO, PITCH_LFO] {
        v.extend(group);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_core::InstrumentId;

    fn instrument() -> Instrument {
        Instrument::new(InstrumentId("t".into()), "T")
    }

    #[test]
    fn keys_are_unique() {
        let specs = all();
        let mut keys: Vec<&str> = specs.iter().map(|s| s.key).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }

    #[test]
    fn param_specs_stay_in_range() {
        for s in all() {
            for k in 0..=100 {
                let v = s.value_at(k as f32 / 100.0);
                assert!((s.min..=s.max).contains(&v), "{} gave {v}", s.key);
            }
            assert_eq!(s.value_at(0.0), s.min, "{}", s.key);
            assert_eq!(s.value_at(1.0), s.max, "{}", s.key);
            assert_eq!(s.value_at(f32::NAN), s.min, "{}", s.key);
        }
    }

    #[test]
    fn param_specs_round_trip_through_fraction() {
        for s in all() {
            for k in 0..=50 {
                let v = s.value_at(k as f32 / 50.0);
                let back = s.value_at(s.fraction_of(v));
                let tolerance = s.step.max((s.max - s.min).abs() * 1e-4);
                assert!((back - v).abs() <= tolerance, "{}: {v} → {back}", s.key);
            }
        }
    }

    #[test]
    fn get_and_set_touch_the_same_field() {
        for s in all() {
            let mut i = instrument();
            let target = s.value_at(0.37);
            (s.set)(&mut i, target);
            assert_eq!((s.get)(&i), target, "{}", s.key);
        }
    }

    #[test]
    fn default_instrument_values_are_in_range() {
        let i = instrument();
        for s in all() {
            let v = (s.get)(&i);
            assert!((s.min..=s.max).contains(&v), "{} default {v}", s.key);
        }
    }

    #[test]
    fn bipolar_values_snap_to_zero() {
        assert_eq!(PAN.value_at(0.5), 0.0);
        assert_eq!(PAN.value_at(0.502), 0.0);
        assert_eq!(FINE_TUNE.value_at(0.5), 0.0);
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_ms(20.0), "20 ms");
        assert_eq!(fmt_ms(1250.0), "1.25 s");
        assert_eq!(fmt_hz(4.0), "4.00 Hz");
        assert_eq!(fmt_hz(440.0), "440 Hz");
        assert_eq!(fmt_hz(12_500.0), "12.50 kHz");
        assert_eq!(fmt_pan(-0.35), "L35");
        assert_eq!(fmt_sync(1, 16), "1/16");
        assert_eq!(fmt_sync(1, 1), "1 bar");
        assert_eq!(fmt_sync(4, 1), "4 bars");
        assert_eq!(fmt_curve(0.0), "linear");
    }

    #[test]
    fn sync_values_are_valid() {
        let mut i = instrument();
        for (n, d) in SYNC_VALUES {
            i.sampler.pitch_lfo.rate = nw_core::LfoRate::Sync { numerator: n, denominator: d };
            assert_eq!(i.sampler.validate(), Ok(()), "{n}/{d}");
        }
    }
}
