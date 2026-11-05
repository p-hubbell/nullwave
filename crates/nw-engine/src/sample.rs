//! Sample loading: decode audio files to stereo f32 buffers and cache them
//! as `basedrop::Shared` so the audio thread can hold them without freeing.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use basedrop::{Handle, Shared};

use crate::process::{PlayRegion, ProcessKey, ProcessSettings, process_sample};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Decoded audio, always stereo. Mono sources are duplicated to both channels;
/// sources with more than two channels keep channels 0 and 1.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBuffer {
    pub sample_rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl SampleBuffer {
    /// Length in frames.
    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SampleError {
    #[error("sample not found: {}", .0.display())]
    NotFound(PathBuf),
    #[error("unsupported audio format: {}", .0.display())]
    Unsupported(PathBuf),
    #[error("failed to decode {}: {message}", .path.display())]
    Decode { path: PathBuf, message: String },
}

pub fn load_sample(path: &Path) -> Result<SampleBuffer, SampleError> {
    let decode_err = |message: String| SampleError::Decode { path: path.to_path_buf(), message };

    let file = File::open(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => SampleError::NotFound(path.to_path_buf()),
        _ => decode_err(e.to_string()),
    })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| match e {
            SymError::Unsupported(_) => SampleError::Unsupported(path.to_path_buf()),
            other => decode_err(other.to_string()),
        })?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| decode_err("no audio track".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| decode_err("missing audio codec parameters".into()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| decode_err(e.to_string()))?;

    let mut sample_rate = 0u32;
    let mut left: Vec<f32> = Vec::new();
    let mut right: Vec<f32> = Vec::new();
    let mut planes: Vec<Vec<f32>> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // A truncated stream ends with an unexpected EOF; keep what we have.
            Err(SymError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(decode_err(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                sample_rate = buf.spec().rate();
                buf.copy_to_vecs_planar::<f32>(&mut planes);
                match planes.len() {
                    0 => {}
                    1 => {
                        left.extend_from_slice(&planes[0]);
                        right.extend_from_slice(&planes[0]);
                    }
                    _ => {
                        left.extend_from_slice(&planes[0]);
                        right.extend_from_slice(&planes[1]);
                    }
                }
            }
            // Skip a corrupt packet, as recommended by symphonia.
            Err(SymError::DecodeError(_)) => {}
            Err(e) => return Err(decode_err(e.to_string())),
        }
    }

    if left.is_empty() {
        return Err(decode_err("no audio frames decoded".into()));
    }
    Ok(SampleBuffer { sample_rate, left, right })
}

type ProcessedKey = (PathBuf, ProcessKey);

/// Caches loaded samples (and load failures) keyed on the path as given, plus their
/// processed versions keyed on path and settings. Not for use on the audio thread.
pub struct SampleCache {
    handle: Handle,
    entries: HashMap<PathBuf, Result<Shared<SampleBuffer>, SampleError>>,
    processed: HashMap<ProcessedKey, (Shared<SampleBuffer>, PlayRegion)>,
    /// Processed entries used since `begin_pass`, while a pass is running.
    used: Option<HashSet<ProcessedKey>>,
}

impl SampleCache {
    pub fn new(handle: Handle) -> Self {
        Self { handle, entries: HashMap::new(), processed: HashMap::new(), used: None }
    }

    /// The sample at `path` with `settings` applied, computed once per distinct settings.
    pub fn processed(
        &mut self,
        path: &Path,
        settings: &ProcessSettings,
    ) -> Result<(Shared<SampleBuffer>, PlayRegion), SampleError> {
        let key = (path.to_path_buf(), settings.key());
        if let Some(used) = &mut self.used {
            used.insert(key.clone());
        }
        if let Some((buffer, region)) = self.processed.get(&key) {
            return Ok((buffer.clone(), *region));
        }
        let source = self.get(path)?;
        let out = process_sample(&source, settings);
        let entry = (Shared::new(&self.handle, out.buffer), out.region);
        self.processed.insert(key, entry.clone());
        Ok(entry)
    }

    /// Starts recording which processed entries are used (by one snapshot compile).
    pub fn begin_pass(&mut self) {
        self.used = Some(HashSet::new());
    }

    /// Drops processed entries not used since `begin_pass`, so memory follows the project.
    pub fn end_pass(&mut self) {
        if let Some(used) = self.used.take() {
            self.processed.retain(|k, _| used.contains(k));
        }
    }

    pub fn get(&mut self, path: &Path) -> Result<Shared<SampleBuffer>, SampleError> {
        if let Some(entry) = self.entries.get(path) {
            return entry.clone();
        }
        let loaded = load_sample(path).map(|b| Shared::new(&self.handle, b));
        self.entries.insert(path.to_path_buf(), loaded.clone());
        loaded
    }

    /// `None` if the path has never been requested.
    pub fn status(&self, path: &Path) -> Option<Result<(), SampleError>> {
        self.entries.get(path).map(|e| e.as_ref().map(|_| ()).map_err(Clone::clone))
    }

    pub fn invalidate(&mut self, path: &Path) {
        self.entries.remove(path);
        self.processed.retain(|(p, _), _| p != path);
    }
}
