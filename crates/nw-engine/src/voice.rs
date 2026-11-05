//! Sampler voice: plays the `PlayRegion` of a `SampleBuffer` (with forward or ping-pong
//! looping) through a DAHDSR volume envelope, a state-variable filter and modulation
//! (cutoff/pitch/pan envelopes and volume/pan/cutoff/pitch LFOs).
//!
//! Volume runs per sample; modulation is computed every `CONTROL_FRAMES` frames. A voice
//! with nothing modulated skips that work, and with default settings renders exactly what
//! the original attack/release voice did. `render` is real-time safe (no allocation,
//! locks, I/O or panics).

use basedrop::Shared;
use nw_core::{FilterKind, LoopMode};

use crate::env::{EnvParams, EnvState};
use crate::filter::Svf;
use crate::lfo::{LfoParams, LfoState};
use crate::process::PlayRegion;
use crate::sample::SampleBuffer;

/// Frames between modulation updates (about 0.3 ms at 48 kHz).
pub const CONTROL_FRAMES: u32 = 16;

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

/// One modulation source and how far it moves its target (0 = off).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModEnv {
    pub env: EnvParams,
    pub amount: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModLfo {
    pub lfo: LfoParams,
    pub amount: f32,
}

/// Everything a voice needs to know about its instrument and note. `Copy`, so starting a
/// voice never allocates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams {
    /// Volume after velocity mapping.
    pub gain: f32,
    pub pan: f32,
    pub volume_env: EnvParams,
    pub filter: FilterKind,
    pub cutoff_hz: f32,
    pub resonance: f32,
    /// Fixed cutoff shift for this note, in octaves (velocity → cutoff).
    pub cutoff_offset: f32,
    /// Octaves.
    pub cutoff_env: ModEnv,
    /// Semitones.
    pub pitch_env: ModEnv,
    /// Pan units.
    pub pan_env: ModEnv,
    /// Depth of the dip below full volume (0..=1).
    pub volume_lfo: ModLfo,
    pub pan_lfo: ModLfo,
    /// Octaves.
    pub cutoff_lfo: ModLfo,
    /// Semitones.
    pub pitch_lfo: ModLfo,
    pub output_rate: u32,
}

impl VoiceParams {
    /// Gain, pan and a plain attack/release envelope; nothing else (previews and tests).
    pub fn simple(gain: f32, pan: f32, attack: u32, release: u32) -> VoiceParams {
        VoiceParams {
            gain,
            pan,
            volume_env: EnvParams::attack_release(attack, release),
            filter: FilterKind::Off,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            cutoff_offset: 0.0,
            cutoff_env: ModEnv::default(),
            pitch_env: ModEnv::default(),
            pan_env: ModEnv::default(),
            volume_lfo: ModLfo::default(),
            pan_lfo: ModLfo::default(),
            cutoff_lfo: ModLfo::default(),
            pitch_lfo: ModLfo::default(),
            output_rate: 48_000,
        }
    }

    /// Whether any control-rate work is needed.
    fn is_modulated(&self) -> bool {
        self.filter != FilterKind::Off
            || [self.cutoff_env, self.pitch_env, self.pan_env].iter().any(|m| m.amount != 0.0)
            || [self.volume_lfo, self.pan_lfo, self.cutoff_lfo, self.pitch_lfo].iter().any(|m| m.amount != 0.0)
    }
}

pub struct VoiceStart {
    pub sample: Shared<SampleBuffer>,
    /// Index of the instrument in the snapshot (for polyphony, mono and cut groups).
    pub instrument: usize,
    pub pitch: u8,
    /// The frames to play (and loop) within `sample`.
    pub region: PlayRegion,
    /// Playback rate before pitch modulation.
    pub rate: f64,
    pub params: VoiceParams,
    /// Output frames until release begins; `None` to play to the region end.
    pub duration_samples: Option<u64>,
    pub age: u64,
}

impl Default for PlayRegion {
    fn default() -> Self {
        PlayRegion::whole(0)
    }
}

/// Modulation state, kept apart from the sample so `render` can borrow both.
#[derive(Clone, Copy, Debug)]
struct Modulation {
    cutoff_env: EnvState,
    pitch_env: EnvState,
    pan_env: EnvState,
    volume_lfo: LfoState,
    pan_lfo: LfoState,
    cutoff_lfo: LfoState,
    pitch_lfo: LfoState,
    filter: Svf,
    /// Frames until the next modulation update.
    countdown: u32,
    active: bool,
    /// Current outputs of the last update.
    rate_factor: f64,
    pan_l: f32,
    pan_r: f32,
    volume: f32,
}

impl Default for Modulation {
    fn default() -> Self {
        Modulation {
            cutoff_env: EnvState::default(),
            pitch_env: EnvState::default(),
            pan_env: EnvState::default(),
            volume_lfo: LfoState::default(),
            pan_lfo: LfoState::default(),
            cutoff_lfo: LfoState::default(),
            pitch_lfo: LfoState::default(),
            filter: Svf::default(),
            countdown: 0,
            active: false,
            rate_factor: 1.0,
            pan_l: std::f32::consts::FRAC_1_SQRT_2,
            pan_r: std::f32::consts::FRAC_1_SQRT_2,
            volume: 1.0,
        }
    }
}

/// Runs an envelope `n` samples and returns its level at the start.
fn step_env(env: &mut EnvState, n: u32) -> f32 {
    let first = env.tick();
    for _ in 1..n {
        env.tick();
    }
    first
}

impl Modulation {
    fn start(&mut self, p: &VoiceParams, seed: u32) {
        let rate = p.output_rate;
        self.active = p.is_modulated();
        self.cutoff_env.start(p.cutoff_env.env);
        self.pitch_env.start(p.pitch_env.env);
        self.pan_env.start(p.pan_env.env);
        self.volume_lfo.start(p.volume_lfo.lfo, rate, seed);
        self.pan_lfo.start(p.pan_lfo.lfo, rate, seed.wrapping_add(1));
        self.cutoff_lfo.start(p.cutoff_lfo.lfo, rate, seed.wrapping_add(2));
        self.pitch_lfo.start(p.pitch_lfo.lfo, rate, seed.wrapping_add(3));
        self.filter.reset();
        self.filter.set(p.filter, p.cutoff_hz, p.resonance, rate);
        self.countdown = 0;
        self.rate_factor = 1.0;
        let (l, r) = pan_gains(p.pan);
        self.pan_l = l;
        self.pan_r = r;
        self.volume = 1.0;
    }

    fn release(&mut self) {
        self.cutoff_env.release();
        self.pitch_env.release();
        self.pan_env.release();
    }

    /// Recomputes rate, pan, volume and filter for the next `CONTROL_FRAMES` frames.
    fn update(&mut self, p: &VoiceParams) {
        let n = CONTROL_FRAMES;
        let cutoff_env = step_env(&mut self.cutoff_env, n);
        let pitch_env = step_env(&mut self.pitch_env, n);
        let pan_env = step_env(&mut self.pan_env, n);
        let volume_lfo = self.volume_lfo.step(n);
        let pan_lfo = self.pan_lfo.step(n);
        let cutoff_lfo = self.cutoff_lfo.step(n);
        let pitch_lfo = self.pitch_lfo.step(n);

        let semitones = pitch_env * p.pitch_env.amount + pitch_lfo * p.pitch_lfo.amount;
        self.rate_factor = if semitones == 0.0 { 1.0 } else { 2f64.powf(f64::from(semitones) / 12.0) };

        let pan = (p.pan + pan_env * p.pan_env.amount + pan_lfo * p.pan_lfo.amount).clamp(-1.0, 1.0);
        let (l, r) = pan_gains(pan);
        self.pan_l = l;
        self.pan_r = r;

        self.volume = 1.0 - p.volume_lfo.amount * (1.0 - volume_lfo) / 2.0;

        if p.filter != FilterKind::Off {
            let octaves = p.cutoff_offset + cutoff_env * p.cutoff_env.amount + cutoff_lfo * p.cutoff_lfo.amount;
            self.filter.set(p.filter, p.cutoff_hz * 2f32.powf(octaves), p.resonance, p.output_rate);
        }
    }
}

pub struct Voice {
    sample: Option<Shared<SampleBuffer>>,
    region: PlayRegion,
    pos: f64,
    /// +1 forward, −1 backward (ping-pong).
    dir: f64,
    rate: f64,
    params: VoiceParams,
    volume_env: EnvState,
    modulation: Modulation,
    remaining: Option<u64>,
    age: u64,
    instrument: usize,
    pitch: u8,
    /// Whether any frame has been rendered since `start`.
    sounded: bool,
    /// A fast fade-out in progress: frames left and total.
    fade: Option<(u32, u32)>,
    /// A legato pitch glide in progress: frames left, per-frame rate factor, final rate.
    glide: Option<(u32, f64, f64)>,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            sample: None,
            region: PlayRegion::default(),
            pos: 0.0,
            dir: 1.0,
            rate: 1.0,
            params: VoiceParams::simple(0.0, 0.0, 0, 0),
            volume_env: EnvState::default(),
            modulation: Modulation::default(),
            remaining: None,
            age: 0,
            instrument: usize::MAX,
            pitch: 0,
            sounded: false,
            fade: None,
            glide: None,
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
    pub fn start(&mut self, s: VoiceStart) {
        let len = s.sample.left.len().min(s.sample.right.len());
        self.sample = Some(s.sample);
        self.region = s.region;
        self.pos = s.region.start as f64;
        self.dir = 1.0;
        self.rate = s.rate;
        self.params = s.params;
        self.remaining = s.duration_samples;
        self.age = s.age;
        self.instrument = s.instrument;
        self.pitch = s.pitch;
        self.sounded = false;
        self.fade = None;
        self.glide = None;
        self.volume_env.start(s.params.volume_env);
        self.modulation.start(&s.params, s.age as u32);
        if s.region.start >= s.region.end.min(len) {
            // Nothing to play.
            self.sample = None;
            return;
        }
        if self.remaining == Some(0) {
            self.release();
        }
    }

    /// Begins the release of every envelope from its current level.
    pub fn release(&mut self) {
        if self.sample.is_none() || self.volume_env.is_releasing() {
            return;
        }
        self.volume_env.release();
        self.modulation.release();
        if self.volume_env.is_done() {
            self.sample = None;
        }
    }

    pub fn is_active(&self) -> bool {
        self.sample.is_some()
    }

    pub fn age(&self) -> u64 {
        self.age
    }

    pub fn instrument(&self) -> usize {
        self.instrument
    }

    pub fn pitch(&self) -> u8 {
        self.pitch
    }

    /// Fading out after a steal, cut or polyphony limit (no longer counts as sounding).
    pub fn is_fading(&self) -> bool {
        self.fade.is_some()
    }

    /// Released, or fading: no longer the note's main sound.
    pub fn is_ending(&self) -> bool {
        self.fade.is_some() || self.volume_env.is_releasing()
    }

    /// Fades out linearly over `frames` from the current level, ignoring the release
    /// time. A voice that has not rendered a frame yet simply stops.
    pub fn fade_out(&mut self, frames: u32) {
        if self.sample.is_none() || self.fade.is_some() {
            return;
        }
        if !self.sounded || frames == 0 {
            self.sample = None;
            return;
        }
        self.fade = Some((frames, frames));
    }

    /// Legato: slides the playback rate to `rate` over `frames` (exponentially, so evenly
    /// in pitch), keeps the envelope running and takes the new note's length and pitch.
    pub fn glide_to(&mut self, rate: f64, frames: u32, duration: Option<u64>, pitch: u8) {
        self.pitch = pitch;
        self.remaining = duration;
        if frames == 0 || self.rate <= 0.0 || rate <= 0.0 {
            self.rate = rate;
            self.glide = None;
        } else {
            let step = (rate / self.rate).powf(1.0 / f64::from(frames));
            self.glide = Some((frames, step, rate));
        }
        if self.remaining == Some(0) {
            self.release();
        }
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
            let p = self.params;
            let m = &mut self.modulation;
            for n in 0..frames {
                if m.active {
                    if m.countdown == 0 {
                        m.update(&p);
                        m.countdown = CONTROL_FRAMES;
                    }
                    m.countdown -= 1;
                }
                let i = self.pos as usize;
                if len == 0 || (!looping && i + 1 >= r.end.min(len)) || self.volume_env.is_done() {
                    done = true;
                    break;
                }
                // In a forward loop the frame after the last one is the loop start.
                let next = if forward_loop && i + 1 >= r.loop_end { r.loop_start } else { i + 1 };
                let (i0, i1) = (i.min(len - 1), next.min(len - 1));
                let frac = (self.pos - i as f64) as f32;
                let sl = buf.left[i0] + (buf.left[i1] - buf.left[i0]) * frac;
                let sr = buf.right[i0] + (buf.right[i1] - buf.right[i0]) * frac;
                let (sl, sr) = m.filter.process(sl, sr);

                let env = self.volume_env.tick();
                let fade = match self.fade {
                    Some((left, total)) => left as f32 / total as f32,
                    None => 1.0,
                };
                let g = p.gain * env * m.volume * fade;
                out_l[n] += sl * g * m.pan_l;
                out_r[n] += sr * g * m.pan_r;
                self.sounded = true;
                advance(&mut self.pos, &mut self.dir, self.rate * m.rate_factor, &r);
                if let Some((left, step, target)) = self.glide.as_mut() {
                    *left -= 1;
                    self.rate = if *left == 0 { *target } else { self.rate * *step };
                    if *left == 0 {
                        self.glide = None;
                    }
                }

                if let Some((left, _)) = self.fade.as_mut() {
                    *left -= 1;
                    if *left == 0 {
                        done = true;
                        break;
                    }
                }
                if self.volume_env.is_done() {
                    done = true;
                    break;
                }
                if !self.volume_env.is_releasing()
                    && let Some(rem) = self.remaining.as_mut()
                {
                    *rem -= 1;
                    if *rem == 0 {
                        // Release begins from the level just rendered.
                        self.remaining = None;
                        self.volume_env.release();
                        m.release();
                        if self.volume_env.is_done() {
                            done = true;
                            break;
                        }
                    }
                }
            }
        }
        if done {
            self.sample = None;
        }
    }
}
