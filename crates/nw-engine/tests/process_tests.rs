//! Sample processing (FL's "precomputed effects") and play-region frames.

use nw_core::{Loop, LoopMode, Processing, Region};
use nw_engine::process::{PlayRegion, ProcessSettings, process_sample};
use nw_engine::sample::SampleBuffer;

/// 10 kHz, so 1 ms is 10 frames.
const SR: u32 = 10_000;

/// Left is a rising ramp 0..1, right is its negative.
fn ramp(frames: usize) -> SampleBuffer {
    let left: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
    let right = left.iter().map(|v| -v).collect();
    SampleBuffer { sample_rate: SR, left, right }
}

fn dc(frames: usize, value: f32) -> SampleBuffer {
    SampleBuffer { sample_rate: SR, left: vec![value; frames], right: vec![value; frames] }
}

/// No processing at all (declick off), whole file, no loop.
fn plain() -> ProcessSettings {
    ProcessSettings {
        processing: Processing { declick: false, ..Processing::default() },
        region: Region::default(),
        looping: Loop::default(),
    }
}

#[test]
fn plain_settings_keep_the_audio() {
    let src = ramp(1000);
    let out = process_sample(&src, &plain());
    assert_eq!(out.buffer, src);
    assert_eq!(out.region, PlayRegion::whole(1000));
}

#[test]
fn region_fractions_become_frames() {
    let mut s = plain();
    s.region = Region { start: 0.25, end: 0.75 };
    let out = process_sample(&ramp(1000), &s);
    assert_eq!((out.region.start, out.region.end), (250, 750));
    assert_eq!(out.region.loop_mode, LoopMode::Off);
}

#[test]
fn loop_fractions_become_frames_inside_region() {
    let mut s = plain();
    s.region = Region { start: 0.1, end: 0.9 };
    s.looping = Loop { mode: LoopMode::PingPong, start: 0.2, end: 0.6, crossfade_ms: 0.0 };
    let out = process_sample(&ramp(1000), &s);
    assert_eq!(out.region.loop_mode, LoopMode::PingPong);
    assert_eq!((out.region.loop_start, out.region.loop_end), (200, 600));
}

#[test]
fn reverse_plays_backwards_and_region_applies_after() {
    let src = ramp(1000);
    let mut s = plain();
    s.processing.reverse = true;
    s.region = Region { start: 0.0, end: 0.5 };
    let out = process_sample(&src, &s);
    assert_eq!(out.buffer.left[0], src.left[999]);
    assert_eq!(out.buffer.left[999], src.left[0]);
    // The region is on the reversed audio: its first half is the original's end.
    assert_eq!(out.buffer.left[out.region.start], src.left[999]);
}

#[test]
fn normalize_scales_region_peak_to_one() {
    let src = dc(1000, 0.25);
    let mut s = plain();
    s.processing.normalize = true;
    let out = process_sample(&src, &s);
    assert!(out.buffer.left.iter().all(|v| (v - 1.0).abs() < 1e-6));
    // Silence stays silence (no division by zero).
    let out = process_sample(&dc(100, 0.0), &s);
    assert!(out.buffer.left.iter().all(|v| *v == 0.0));
}

#[test]
fn invert_and_swap_stereo() {
    let src = ramp(100);
    let mut s = plain();
    s.processing.invert_polarity = true;
    let out = process_sample(&src, &s);
    assert_eq!(out.buffer.left[50], -src.left[50]);
    assert_eq!(out.buffer.right[50], -src.right[50]);
    s.processing.invert_polarity = false;
    s.processing.swap_stereo = true;
    let out = process_sample(&src, &s);
    assert_eq!(out.buffer.left, src.right);
    assert_eq!(out.buffer.right, src.left);
}

#[test]
fn declick_fades_region_edges() {
    let mut s = plain();
    s.processing.declick = true;
    s.region = Region { start: 0.1, end: 0.9 };
    let out = process_sample(&dc(1000, 1.0), &s);
    let (a, b) = (out.region.start, out.region.end);
    // 2 ms = 20 frames of fade in and out; the middle is untouched.
    assert_eq!(out.buffer.left[a], 0.0);
    assert!(out.buffer.left[a + 10] > 0.3 && out.buffer.left[a + 10] < 0.7);
    assert_eq!(out.buffer.left[a + 20], 1.0);
    assert_eq!(out.buffer.left[500], 1.0);
    assert!(out.buffer.left[b - 1] < 0.1);
    assert_eq!(out.buffer.left[b - 21], 1.0);
}

#[test]
fn declick_keeps_the_loop_body_at_full_level() {
    // With a loop, the sound never plays to the region end, so no fade-out there,
    // and the fade-in at a trimmed start must not reach into the loop.
    let mut s = plain();
    s.processing.declick = true;
    s.region = Region { start: 0.1, end: 1.0 };
    s.looping = Loop { mode: LoopMode::PingPong, start: 0.105, end: 1.0, crossfade_ms: 0.0 };
    let out = process_sample(&dc(1000, 1.0), &s);
    let r = out.region;
    assert!(out.buffer.left[r.loop_start..r.loop_end].iter().all(|v| *v == 1.0));
}

/// The jump heard when a forward loop wraps from its last frame to its first.
fn seam_jump(out: &nw_engine::process::ProcessedSample) -> f32 {
    let r = out.region;
    (out.buffer.left[r.loop_end - 1] - out.buffer.left[r.loop_start]).abs()
}

#[test]
fn crossfade_removes_loop_discontinuity() {
    let mut s = plain();
    s.looping = Loop { mode: LoopMode::Forward, start: 0.3, end: 0.8, crossfade_ms: 0.0 };
    let hard = seam_jump(&process_sample(&ramp(1000), &s));
    assert!(hard > 0.4);
    s.looping.crossfade_ms = 10.0; // 100 frames
    let soft = seam_jump(&process_sample(&ramp(1000), &s));
    assert!(soft < hard / 20.0, "seam jump {soft} vs {hard}");
}

#[test]
fn crossfade_is_limited_by_audio_before_the_loop() {
    // Loop starts at frame 20, so at most 20 frames can be blended in; no panic.
    let mut s = plain();
    s.looping = Loop { mode: LoopMode::Forward, start: 0.02, end: 0.5, crossfade_ms: 500.0 };
    let out = process_sample(&ramp(1000), &s);
    assert!(seam_jump(&out) < 0.48);
}

#[test]
fn declick_smooths_a_forward_loop_seam_without_crossfade() {
    let mut s = plain();
    s.looping = Loop { mode: LoopMode::Forward, start: 0.3, end: 0.8, crossfade_ms: 0.0 };
    let hard = seam_jump(&process_sample(&ramp(1000), &s));
    s.processing.declick = true;
    let soft = seam_jump(&process_sample(&ramp(1000), &s));
    assert!(soft < hard / 10.0, "seam jump {soft} vs {hard}");
}

#[test]
fn tiny_and_empty_buffers_do_not_panic() {
    let mut s = plain();
    s.processing = Processing { reverse: true, normalize: true, invert_polarity: true, swap_stereo: true, declick: true };
    s.region = Region { start: 0.4, end: 0.6 };
    s.looping = Loop { mode: LoopMode::Forward, start: 0.45, end: 0.55, crossfade_ms: 500.0 };
    for frames in [0, 1, 2, 3, 10] {
        let out = process_sample(&ramp(frames), &s);
        let r = out.region;
        assert!(r.start <= r.end && r.end <= frames.max(r.end));
        assert!(r.end <= out.buffer.left.len());
    }
}

#[test]
fn declick_keeps_the_transient_of_an_untrimmed_start() {
    // Fading in only helps when the start is trimmed mid-waveform; at frame 0 it would
    // soften a drum's attack.
    let mut s = plain();
    s.processing.declick = true;
    let out = process_sample(&dc(1000, 1.0), &s);
    assert_eq!(out.buffer.left[0], 1.0);
    assert_eq!(out.buffer.left[10], 1.0);
}
