use std::path::{Path, PathBuf};

use basedrop::Collector;
use nw_engine::sample::{SampleCache, SampleError, load_sample};
use tempfile::TempDir;

fn write_wav(dir: &TempDir, name: &str, rate: u32, channels: u16, frames: usize) -> PathBuf {
    let path = dir.path().join(name);
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..frames {
        for c in 0..channels {
            // Distinct, deterministic value per channel.
            let v = (((i % 100) as i32 - 50) * 100 * (c as i32 + 1)) as i16;
            w.write_sample(v).unwrap();
        }
    }
    w.finalize().unwrap();
    path
}

#[test]
fn mono_wav_duplicates_channels() {
    let dir = TempDir::new().unwrap();
    let p = write_wav(&dir, "mono.wav", 44100, 1, 4410);
    let b = load_sample(&p).unwrap();
    assert_eq!(b.sample_rate, 44100);
    assert_eq!(b.left, b.right);
    assert_eq!(b.len(), 4410);
    // 16-bit ints normalized to -1..1: frame 0 is -5000 / 32768.
    assert!((b.left[0] - (-5000.0 / 32768.0)).abs() < 1e-6);
}

#[test]
fn stereo_keeps_channels() {
    let dir = TempDir::new().unwrap();
    let p = write_wav(&dir, "stereo.wav", 48000, 2, 1000);
    let b = load_sample(&p).unwrap();
    assert_eq!(b.sample_rate, 48000);
    assert_eq!(b.len(), 1000);
    assert_ne!(b.left, b.right);
    assert!((b.left[0] - (-5000.0 / 32768.0)).abs() < 1e-6);
    assert!((b.right[0] - (-10000.0 / 32768.0)).abs() < 1e-6);
}

#[test]
fn multichannel_takes_first_two() {
    let dir = TempDir::new().unwrap();
    let p = write_wav(&dir, "six.wav", 48000, 6, 500);
    let b = load_sample(&p).unwrap();
    assert_eq!(b.len(), 500);
    assert!((b.left[0] - (-5000.0 / 32768.0)).abs() < 1e-6);
    assert!((b.right[0] - (-10000.0 / 32768.0)).abs() < 1e-6);
}

#[test]
fn missing_file_is_not_found() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("nope.wav");
    assert!(matches!(load_sample(&p), Err(SampleError::NotFound(_))));
}

#[test]
fn garbage_bytes_are_decode_or_unsupported_not_panic() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("garbage.wav");
    let bytes: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    std::fs::write(&p, bytes).unwrap();
    assert!(matches!(
        load_sample(&p),
        Err(SampleError::Unsupported(_)) | Err(SampleError::Decode { .. })
    ));
}

#[test]
fn text_file_is_unsupported() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("notes.txt");
    std::fs::write(&p, "this is definitely not audio, just some plain text\n".repeat(50)).unwrap();
    assert!(matches!(load_sample(&p), Err(SampleError::Unsupported(_))));
}

#[test]
fn errors_display_and_clone() {
    let e = SampleError::Decode { path: Path::new("a.wav").into(), message: "bad".into() };
    assert!(e.to_string().contains("a.wav"));
    assert!(e.to_string().contains("bad"));
    let _ = e.clone();
}

#[test]
fn cache_returns_same_buffer_and_caches_failure() {
    let dir = TempDir::new().unwrap();
    let p = write_wav(&dir, "c.wav", 44100, 1, 100);
    let collector = Collector::new();
    let mut cache = SampleCache::new(collector.handle());

    assert!(cache.status(&p).is_none());
    let a = cache.get(&p).unwrap();
    let b = cache.get(&p).unwrap();
    assert!(std::ptr::eq(&*a, &*b));
    assert!(matches!(cache.status(&p), Some(Ok(()))));

    // Failure is cached: creating the file afterwards doesn't change the result.
    let missing = dir.path().join("late.wav");
    assert!(matches!(cache.get(&missing), Err(SampleError::NotFound(_))));
    write_wav(&dir, "late.wav", 44100, 1, 10);
    assert!(matches!(cache.get(&missing), Err(SampleError::NotFound(_))));
    assert!(matches!(cache.status(&missing), Some(Err(SampleError::NotFound(_)))));

    // Invalidate forces a reload.
    cache.invalidate(&missing);
    assert!(cache.status(&missing).is_none());
    assert_eq!(cache.get(&missing).unwrap().len(), 10);
    cache.invalidate(&p);
    let c = cache.get(&p).unwrap();
    assert!(!std::ptr::eq(&*a, &*c));
}

#[test]
fn processed_cache_reuses_and_drops_unused() {
    use nw_core::{Loop, Processing, Region};
    use nw_engine::process::ProcessSettings;

    let dir = TempDir::new().unwrap();
    let p = write_wav(&dir, "p.wav", 44100, 1, 1000);
    let collector = Collector::new();
    let mut cache = SampleCache::new(collector.handle());
    let plain = ProcessSettings { processing: Processing::default(), region: Region::default(), looping: Loop::default() };
    let reversed = ProcessSettings { processing: Processing { reverse: true, ..Processing::default() }, ..plain };

    let (a, ra) = cache.processed(&p, &plain).unwrap();
    let (b, _) = cache.processed(&p, &plain).unwrap();
    assert!(std::ptr::eq(&*a, &*b), "same settings share one processed buffer");
    let (r, rr) = cache.processed(&p, &reversed).unwrap();
    assert!(!std::ptr::eq(&*a, &*r));
    assert_eq!(ra, rr);
    // Away from the declicked edges, the reversed buffer mirrors the plain one.
    assert_eq!(r.left[300], a.left[699]);

    // A compile pass that only uses `plain` drops `reversed`.
    cache.begin_pass();
    let (a2, _) = cache.processed(&p, &plain).unwrap();
    cache.end_pass();
    assert!(std::ptr::eq(&*a, &*a2));
    let (r2, _) = cache.processed(&p, &reversed).unwrap();
    assert!(!std::ptr::eq(&*r, &*r2), "reversed was dropped and rebuilt");

    // Invalidating the file drops its processed versions too.
    cache.invalidate(&p);
    let (a3, _) = cache.processed(&p, &plain).unwrap();
    assert!(!std::ptr::eq(&*a, &*a3));

    // Missing files report the load error.
    let missing = dir.path().join("nope.wav");
    assert!(matches!(cache.processed(&missing, &plain), Err(SampleError::NotFound(_))));
}

#[test]
fn peaks_min_max_per_bucket() {
    use nw_engine::sample::{SampleBuffer, peaks};
    let left = vec![0.1, -0.5, 0.3, 0.9, -0.2, 0.0, 0.4, -0.8];
    let right = vec![0.0, 0.2, -0.6, 0.1, 0.5, 0.0, -0.1, 0.3];
    let buf = SampleBuffer { sample_rate: 44100, left, right };
    // Min and max over both channels, per quarter of the buffer.
    assert_eq!(peaks(&buf, 4), vec![(-0.5, 0.2), (-0.6, 0.9), (-0.2, 0.5), (-0.8, 0.4)]);
    // More buckets than frames: each frame gets one, the rest repeat the nearest frame.
    let p = peaks(&buf, 16);
    assert_eq!(p.len(), 16);
    assert_eq!(p[0], (0.0, 0.1));
}

#[test]
fn peaks_of_empty_buffer() {
    use nw_engine::sample::{SampleBuffer, peaks};
    let buf = SampleBuffer { sample_rate: 44100, left: vec![], right: vec![] };
    assert_eq!(peaks(&buf, 8), vec![(0.0, 0.0); 8]);
    assert!(peaks(&buf, 0).is_empty());
}
