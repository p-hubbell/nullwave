//! The FL-style sampler's settings (spec: "Sampler"). Plain data: the engine turns it into
//! voice parameters and the instrument tab edits it. Every struct is `#[serde(default)]`,
//! so files written by older versions load with the defaults filled in, and the defaults
//! sound exactly like the original attack/release-only sampler.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Ranges for every setting, shared by validation and the instrument tab.
pub mod limits {
    use std::ops::RangeInclusive;

    pub const ROOT_NOTE: RangeInclusive<u8> = 0..=127;
    pub const FRACTION: RangeInclusive<f32> = 0.0..=1.0;
    pub const CROSSFADE_MS: RangeInclusive<f32> = 0.0..=500.0;
    pub const COARSE: RangeInclusive<i8> = -24..=24;
    pub const FINE_CENTS: RangeInclusive<f32> = -100.0..=100.0;
    pub const DELAY_HOLD_MS: RangeInclusive<f32> = 0.0..=2000.0;
    pub const STAGE_MS: RangeInclusive<f32> = 0.0..=5000.0;
    pub const CURVE: RangeInclusive<f32> = -1.0..=1.0;
    pub const CUTOFF_HZ: RangeInclusive<f32> = 20.0..=20_000.0;
    pub const OCTAVES: RangeInclusive<f32> = -8.0..=8.0;
    pub const SEMITONES: RangeInclusive<f32> = -24.0..=24.0;
    pub const PAN: RangeInclusive<f32> = -1.0..=1.0;
    pub const LFO_HZ: RangeInclusive<f32> = 0.01..=50.0;
    pub const LFO_DELAY_MS: RangeInclusive<f32> = 0.0..=5000.0;
    /// Tempo-synced LFO period, in whole notes: a 1/64 note up to 8 bars of 4/4.
    pub const LFO_SYNC_WHOLE_NOTES: RangeInclusive<f32> = (1.0 / 64.0)..=8.0;
    pub const POLYPHONY: RangeInclusive<u8> = 1..=64;
    pub const GLIDE_MS: RangeInclusive<f32> = 0.0..=2000.0;
    /// 0 means "no group".
    pub const CUT_GROUP: RangeInclusive<u8> = 0..=16;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "SamplerFile")]
pub struct Sampler {
    pub sample_path: Option<PathBuf>,
    pub root_note: u8,
    pub region: Region,
    pub looping: Loop,
    /// Play the whole region regardless of note length (ignored while looping).
    pub one_shot: bool,
    pub processing: Processing,
    pub tune: Tune,
    pub volume_env: Envelope,
    pub filter: Filter,
    /// Amount in octaves.
    pub cutoff_env: ModEnvelope,
    /// Amount in semitones.
    pub pitch_env: ModEnvelope,
    /// Amount in pan units (−1..=1).
    pub pan_env: ModEnvelope,
    /// Amount is the depth of the dip below full volume (0..=1).
    pub volume_lfo: Lfo,
    /// Amount in pan units (0..=1).
    pub pan_lfo: Lfo,
    /// Amount in octaves.
    pub cutoff_lfo: Lfo,
    /// Amount in semitones.
    pub pitch_lfo: Lfo,
    pub velocity: VelocityMap,
    pub voice: VoiceSettings,
}

impl Default for Sampler {
    fn default() -> Self {
        Sampler {
            sample_path: None,
            root_note: 60,
            region: Region::default(),
            looping: Loop::default(),
            one_shot: false,
            processing: Processing::default(),
            tune: Tune::default(),
            volume_env: Envelope::default(),
            filter: Filter::default(),
            cutoff_env: ModEnvelope::default(),
            pitch_env: ModEnvelope::default(),
            pan_env: ModEnvelope::default(),
            volume_lfo: Lfo::default(),
            pan_lfo: Lfo::default(),
            cutoff_lfo: Lfo::default(),
            pitch_lfo: Lfo::default(),
            velocity: VelocityMap::default(),
            voice: VoiceSettings::default(),
        }
    }
}

/// Start and end of the played part of the file, as fractions of its length.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Region {
    pub start: f32,
    pub end: f32,
}

impl Default for Region {
    fn default() -> Self {
        Region { start: 0.0, end: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LoopMode {
    #[default]
    Off,
    Forward,
    PingPong,
}

/// Loop points are fractions of the file, inside the region.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Loop {
    pub mode: LoopMode,
    pub start: f32,
    pub end: f32,
    pub crossfade_ms: f32,
}

impl Default for Loop {
    fn default() -> Self {
        Loop { mode: LoopMode::Off, start: 0.0, end: 1.0, crossfade_ms: 0.0 }
    }
}

/// FL's "precomputed effects": applied to the sample data off the audio thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Processing {
    pub reverse: bool,
    pub normalize: bool,
    pub invert_polarity: bool,
    pub swap_stereo: bool,
    /// Short fades at the region and loop edges.
    pub declick: bool,
}

impl Default for Processing {
    fn default() -> Self {
        Processing { reverse: false, normalize: false, invert_polarity: false, swap_stereo: false, declick: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tune {
    pub coarse: i8,
    pub fine_cents: f32,
}

/// DAHDSR. Curves bend a stage: 0 is linear, negative is fast-then-slow, positive is
/// slow-then-fast.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Envelope {
    pub delay_ms: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub decay_ms: f32,
    /// 0..=1.
    pub sustain: f32,
    pub release_ms: f32,
    pub attack_curve: f32,
    /// Applies to decay and release.
    pub decay_curve: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Envelope {
            delay_ms: 0.0,
            attack_ms: 0.0,
            hold_ms: 0.0,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms: 20.0,
            attack_curve: 0.0,
            decay_curve: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilterKind {
    #[default]
    Off,
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    pub kind: FilterKind,
    pub cutoff_hz: f32,
    /// 0..=1.
    pub resonance: f32,
}

impl Default for Filter {
    fn default() -> Self {
        Filter { kind: FilterKind::Off, cutoff_hz: 20_000.0, resonance: 0.0 }
    }
}

/// An envelope driving one target by `amount` (units depend on the target; 0 = off).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModEnvelope {
    pub env: Envelope,
    pub amount: f32,
}

impl Default for ModEnvelope {
    fn default() -> Self {
        ModEnvelope { env: Envelope::default(), amount: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
    /// Sample and hold: a new random value each cycle.
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LfoRate {
    Hz(f32),
    /// One cycle every `numerator / denominator` whole notes.
    Sync { numerator: u16, denominator: u16 },
}

impl Default for LfoRate {
    fn default() -> Self {
        LfoRate::Hz(4.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lfo {
    pub shape: LfoShape,
    pub rate: LfoRate,
    /// 0 = off; units depend on the target.
    pub amount: f32,
    pub delay_ms: f32,
}

impl Default for Lfo {
    fn default() -> Self {
        Lfo { shape: LfoShape::Sine, rate: LfoRate::default(), amount: 0.0, delay_ms: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VelocityMap {
    /// How much velocity scales volume: 0 ignores velocity, 1 is fully proportional.
    pub to_volume: f32,
    /// Cutoff shift at full velocity, scaled down for softer notes.
    pub to_cutoff_octaves: f32,
}

impl Default for VelocityMap {
    fn default() -> Self {
        VelocityMap { to_volume: 1.0, to_cutoff_octaves: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    pub polyphony: u8,
    pub mono: bool,
    pub glide_ms: f32,
    /// The group this instrument's notes cut (0 = none).
    pub cut: u8,
    /// The group that cuts this instrument (0 = none).
    pub cut_by: u8,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings { polyphony: 64, mono: false, glide_ms: 0.0, cut: 0, cut_by: 0 }
    }
}

/// The on-disk shape: the current fields plus the original sampler's flat
/// `attack_ms` / `release_ms`, which move into the volume envelope.
#[derive(Deserialize)]
#[serde(default)]
struct SamplerFile {
    sample_path: Option<PathBuf>,
    root_note: u8,
    region: Region,
    looping: Loop,
    one_shot: bool,
    processing: Processing,
    tune: Tune,
    volume_env: Envelope,
    filter: Filter,
    cutoff_env: ModEnvelope,
    pitch_env: ModEnvelope,
    pan_env: ModEnvelope,
    volume_lfo: Lfo,
    pan_lfo: Lfo,
    cutoff_lfo: Lfo,
    pitch_lfo: Lfo,
    velocity: VelocityMap,
    voice: VoiceSettings,
    // Old files hold bare numbers here, not `Some(..)`.
    #[serde(deserialize_with = "bare_f32")]
    attack_ms: Option<f32>,
    #[serde(deserialize_with = "bare_f32")]
    release_ms: Option<f32>,
}

fn bare_f32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<f32>, D::Error> {
    f32::deserialize(d).map(Some)
}

impl Default for SamplerFile {
    fn default() -> Self {
        let s = Sampler::default();
        SamplerFile {
            sample_path: s.sample_path,
            root_note: s.root_note,
            region: s.region,
            looping: s.looping,
            one_shot: s.one_shot,
            processing: s.processing,
            tune: s.tune,
            volume_env: s.volume_env,
            filter: s.filter,
            cutoff_env: s.cutoff_env,
            pitch_env: s.pitch_env,
            pan_env: s.pan_env,
            volume_lfo: s.volume_lfo,
            pan_lfo: s.pan_lfo,
            cutoff_lfo: s.cutoff_lfo,
            pitch_lfo: s.pitch_lfo,
            velocity: s.velocity,
            voice: s.voice,
            attack_ms: None,
            release_ms: None,
        }
    }
}

impl From<SamplerFile> for Sampler {
    fn from(f: SamplerFile) -> Sampler {
        let mut volume_env = f.volume_env;
        if let Some(a) = f.attack_ms {
            volume_env.attack_ms = a;
        }
        if let Some(r) = f.release_ms {
            volume_env.release_ms = r;
        }
        Sampler {
            sample_path: f.sample_path,
            root_note: f.root_note,
            region: f.region,
            looping: f.looping,
            one_shot: f.one_shot,
            processing: f.processing,
            tune: f.tune,
            volume_env,
            filter: f.filter,
            cutoff_env: f.cutoff_env,
            pitch_env: f.pitch_env,
            pan_env: f.pan_env,
            volume_lfo: f.volume_lfo,
            pan_lfo: f.pan_lfo,
            cutoff_lfo: f.cutoff_lfo,
            pitch_lfo: f.pitch_lfo,
            velocity: f.velocity,
            voice: f.voice,
        }
    }
}

fn check<T: PartialOrd + std::fmt::Display>(
    name: &str,
    value: T,
    range: &std::ops::RangeInclusive<T>,
) -> Result<(), String> {
    // `contains` is false for NaN, so non-finite floats are rejected too.
    if range.contains(&value) {
        Ok(())
    } else {
        Err(format!("{name} {value} outside {}..={}", range.start(), range.end()))
    }
}

fn check_env(name: &str, e: &Envelope) -> Result<(), String> {
    use limits::*;
    check(&format!("{name}.delay_ms"), e.delay_ms, &DELAY_HOLD_MS)?;
    check(&format!("{name}.attack_ms"), e.attack_ms, &STAGE_MS)?;
    check(&format!("{name}.hold_ms"), e.hold_ms, &DELAY_HOLD_MS)?;
    check(&format!("{name}.decay_ms"), e.decay_ms, &STAGE_MS)?;
    check(&format!("{name}.sustain"), e.sustain, &FRACTION)?;
    check(&format!("{name}.release_ms"), e.release_ms, &STAGE_MS)?;
    check(&format!("{name}.attack_curve"), e.attack_curve, &CURVE)?;
    check(&format!("{name}.decay_curve"), e.decay_curve, &CURVE)
}

fn check_lfo(name: &str, l: &Lfo, amount: &std::ops::RangeInclusive<f32>) -> Result<(), String> {
    use limits::*;
    check(&format!("{name}.amount"), l.amount, amount)?;
    check(&format!("{name}.delay_ms"), l.delay_ms, &LFO_DELAY_MS)?;
    match l.rate {
        LfoRate::Hz(hz) => check(&format!("{name}.rate"), hz, &LFO_HZ),
        LfoRate::Sync { numerator, denominator } => {
            let bad = numerator == 0
                || !denominator.is_power_of_two()
                || denominator > 64
                || !LFO_SYNC_WHOLE_NOTES.contains(&(f32::from(numerator) / f32::from(denominator)));
            if bad {
                Err(format!("{name}.rate {numerator}/{denominator} must be a 1/64 note to 8 bars"))
            } else {
                Ok(())
            }
        }
    }
}

impl Sampler {
    /// Checks every range in the spec. The message names the offending field.
    pub fn validate(&self) -> Result<(), String> {
        use limits::*;
        check("root_note", self.root_note, &ROOT_NOTE)?;
        let r = self.region;
        check("region.start", r.start, &FRACTION)?;
        check("region.end", r.end, &FRACTION)?;
        if r.start >= r.end {
            return Err(format!("region start {} must be before end {}", r.start, r.end));
        }
        let l = self.looping;
        check("looping.crossfade_ms", l.crossfade_ms, &CROSSFADE_MS)?;
        if l.mode != LoopMode::Off && !(r.start <= l.start && l.start < l.end && l.end <= r.end) {
            return Err(format!(
                "loop {}..{} must be a non-empty span inside the region {}..{}",
                l.start, l.end, r.start, r.end
            ));
        }
        check("tune.coarse", self.tune.coarse, &COARSE)?;
        check("tune.fine_cents", self.tune.fine_cents, &FINE_CENTS)?;
        check_env("volume_env", &self.volume_env)?;
        check("filter.cutoff_hz", self.filter.cutoff_hz, &CUTOFF_HZ)?;
        check("filter.resonance", self.filter.resonance, &FRACTION)?;
        for (name, m, range) in [
            ("cutoff_env", &self.cutoff_env, &OCTAVES),
            ("pitch_env", &self.pitch_env, &SEMITONES),
            ("pan_env", &self.pan_env, &PAN),
        ] {
            check(&format!("{name}.amount"), m.amount, range)?;
            check_env(&format!("{name}.env"), &m.env)?;
        }
        check_lfo("volume_lfo", &self.volume_lfo, &FRACTION)?;
        check_lfo("pan_lfo", &self.pan_lfo, &FRACTION)?;
        check_lfo("cutoff_lfo", &self.cutoff_lfo, &OCTAVES)?;
        check_lfo("pitch_lfo", &self.pitch_lfo, &SEMITONES)?;
        check("velocity.to_volume", self.velocity.to_volume, &FRACTION)?;
        check("velocity.to_cutoff_octaves", self.velocity.to_cutoff_octaves, &OCTAVES)?;
        let v = self.voice;
        check("voice.polyphony", v.polyphony, &POLYPHONY)?;
        check("voice.glide_ms", v.glide_ms, &GLIDE_MS)?;
        check("voice.cut", v.cut, &CUT_GROUP)?;
        check("voice.cut_by", v.cut_by, &CUT_GROUP)
    }
}
