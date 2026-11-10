//! FL's "precomputed effects": reverse, normalize, polarity, stereo swap, declick and the
//! loop crossfade are baked into a copy of the sample when settings change, off the audio
//! thread. Voices then only read frames inside a `PlayRegion`.

use nw_core::{Loop, LoopMode, Processing, Region};

use crate::sample::SampleBuffer;

/// Length of the declick fades (and of the seam crossfade a forward loop gets when
/// declick is on and no crossfade is set).
pub const DECLICK_MS: f32 = 2.0;

/// Everything that changes the processed audio or its play region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessSettings {
    pub processing: Processing,
    pub region: Region,
    pub looping: Loop,
}

/// A hashable stand-in for `ProcessSettings` (floats compared bit for bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProcessKey {
    processing: Processing,
    loop_mode: u8,
    bits: [u32; 5],
}

impl ProcessSettings {
    pub(crate) fn key(&self) -> ProcessKey {
        ProcessKey {
            processing: self.processing,
            loop_mode: match self.looping.mode {
                LoopMode::Off => 0,
                LoopMode::Forward => 1,
                LoopMode::PingPong => 2,
            },
            bits: [
                self.region.start.to_bits(),
                self.region.end.to_bits(),
                self.looping.start.to_bits(),
                self.looping.end.to_bits(),
                self.looping.crossfade_ms.to_bits(),
            ],
        }
    }
}

/// Frame indices into a processed buffer. `end` and `loop_end` are exclusive for reading
/// forward; a ping-pong loop turns around at `loop_end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayRegion {
    pub start: usize,
    pub end: usize,
    /// `Off` whenever the loop span is empty.
    pub loop_mode: LoopMode,
    pub loop_start: usize,
    pub loop_end: usize,
}

impl PlayRegion {
    /// The whole buffer, no loop (used for previews of raw files).
    pub fn whole(len: usize) -> PlayRegion {
        PlayRegion { start: 0, end: len, loop_mode: LoopMode::Off, loop_start: 0, loop_end: len }
    }

    pub fn is_looping(&self) -> bool {
        self.loop_mode != LoopMode::Off && self.loop_end > self.loop_start
    }
}

pub struct ProcessedSample {
    pub buffer: SampleBuffer,
    pub region: PlayRegion,
}

fn to_frame(fraction: f32, len: usize) -> usize {
    let f = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    ((f * len as f32).round() as usize).min(len)
}

/// Applies `settings` to a copy of `source`. Not for the audio thread (it allocates).
pub fn process_sample(source: &SampleBuffer, settings: &ProcessSettings) -> ProcessedSample {
    let len = source.left.len().min(source.right.len());
    let mut left = source.left[..len].to_vec();
    let mut right = source.right[..len].to_vec();
    let p = settings.processing;

    if p.swap_stereo {
        std::mem::swap(&mut left, &mut right);
    }
    if p.invert_polarity {
        left.iter_mut().chain(right.iter_mut()).for_each(|v| *v = -*v);
    }
    // Reverse first: region and loop points refer to the audio as shown (and heard).
    if p.reverse {
        left.reverse();
        right.reverse();
    }

    let start = to_frame(settings.region.start, len);
    let end = to_frame(settings.region.end, len).max(start);
    let l = settings.looping;
    let (loop_mode, loop_start, loop_end) = if l.mode == LoopMode::Off {
        (LoopMode::Off, start, end)
    } else {
        let ls = to_frame(l.start, len).clamp(start, end);
        let le = to_frame(l.end, len).clamp(ls, end);
        if le > ls { (l.mode, ls, le) } else { (LoopMode::Off, start, end) }
    };
    let region = PlayRegion { start, end, loop_mode, loop_start, loop_end };

    if p.normalize {
        let peak = left[start..end]
            .iter()
            .chain(&right[start..end])
            .fold(0.0f32, |m, v| m.max(v.abs()));
        if peak > 0.0 {
            let gain = 1.0 / peak;
            left.iter_mut().chain(right.iter_mut()).for_each(|v| *v *= gain);
        }
    }

    let ms_to_frames = |ms: f32| (ms.max(0.0) * source.sample_rate as f32 / 1000.0) as usize;

    // Forward loop seam: blend the audio just before the loop start into the end of the
    // loop, so wrapping from `loop_end - 1` to `loop_start` is continuous.
    if loop_mode == LoopMode::Forward {
        let wanted = if l.crossfade_ms > 0.0 {
            ms_to_frames(l.crossfade_ms)
        } else if p.declick {
            ms_to_frames(DECLICK_MS)
        } else {
            0
        };
        let xf = wanted.min(loop_end - loop_start).min(loop_start);
        for i in 0..xf {
            let g = (i + 1) as f32 / xf as f32;
            let dst = loop_end - xf + i;
            let src = loop_start - xf + i;
            left[dst] = left[dst] * (1.0 - g) + left[src] * g;
            right[dst] = right[dst] * (1.0 - g) + right[src] * g;
        }
    }

    // Declick: fade in at a trimmed region start (never into the loop; an untrimmed start
    // keeps its transient) and, when the sound can reach it, fade out at the region end.
    if p.declick {
        let n = ms_to_frames(DECLICK_MS);
        let fade_in = match (start, region.is_looping()) {
            (0, _) => 0,
            (_, true) => n.min(loop_start - start),
            (_, false) => n,
        };
        for i in 0..fade_in.min(end - start) {
            let g = i as f32 / fade_in as f32;
            left[start + i] *= g;
            right[start + i] *= g;
        }
        if !region.is_looping() {
            for i in 0..n.min(end - start) {
                let g = i as f32 / n as f32;
                left[end - 1 - i] *= g;
                right[end - 1 - i] *= g;
            }
        }
    }

    ProcessedSample { buffer: SampleBuffer { sample_rate: source.sample_rate, left, right }, region }
}
