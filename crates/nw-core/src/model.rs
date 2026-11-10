use crate::sampler::Sampler;
use crate::time::{Tick, TimeSignature};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_VELOCITY: u8 = 100;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstrumentId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PatternId(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub start: Tick,
    pub length: Tick,
    pub velocity: u8,
}

impl Note {
    pub fn end(&self) -> Tick {
        self.start + self.length
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    pub id: InstrumentId,
    pub name: String,
    pub volume: f32,
    /// Range -1.0..=1.0.
    pub pan: f32,
    pub sampler: Sampler,
}

impl Instrument {
    pub fn new(id: InstrumentId, name: impl Into<String>) -> Instrument {
        Instrument { id, name: name.into(), volume: 0.78, pan: 0.0, sampler: Sampler::default() }
    }
}

/// Clip numbers count per instrument within one timeline, starting at 1.
pub type ClipNumber = u32;

/// Where a clip lives: inside one pattern, or loose in the song.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Container {
    Pattern(PatternId),
    Song,
}

/// A clip within its timeline: the instrument plus the number shown on its blocks.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClipKey {
    pub instrument: InstrumentId,
    pub number: ClipNumber,
}

/// A clip anywhere in the project.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClipRef {
    pub container: Container,
    pub key: ClipKey,
}

/// A run of notes for one instrument. Notes are in clip-local ticks, sorted by
/// `(start, pitch)`; only the window `[start, start + length)` is heard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub key: ClipKey,
    pub start: Tick,
    pub length: Tick,
    pub notes: Vec<Note>,
}

/// One block of a clip on a timeline; `at` is where the clip's window starts sounding.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Placement {
    pub clip: ClipKey,
    pub at: Tick,
}

/// Clips and where they are placed. Every clip has at least one placement.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Timeline {
    /// Sorted by key.
    pub clips: Vec<Clip>,
    /// Sorted by `(at, clip)`.
    pub placements: Vec<Placement>,
}

impl Timeline {
    pub fn clip(&self, key: &ClipKey) -> Option<&Clip> {
        self.clips.iter().find(|c| &c.key == key)
    }

    pub fn clip_mut(&mut self, key: &ClipKey) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| &c.key == key)
    }

    /// How many blocks show this clip.
    pub fn uses(&self, key: &ClipKey) -> usize {
        self.placements.iter().filter(|p| &p.clip == key).count()
    }

    /// One above the highest number this instrument uses here (1 if none).
    pub fn next_number(&self, instrument: &InstrumentId) -> ClipNumber {
        let in_clips = self.clips.iter().map(|c| &c.key);
        let in_placements = self.placements.iter().map(|p| &p.clip);
        in_clips.chain(in_placements).filter(|k| &k.instrument == instrument).map(|k| k.number).max().unwrap_or(0)
            + 1
    }

    /// End of the last placed block (0 if empty).
    pub fn end(&self) -> Tick {
        self.placements
            .iter()
            .filter_map(|p| self.clip(&p.clip).map(|c| p.at + c.length))
            .max()
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: PatternId,
    pub name: String,
    pub timeline: Timeline,
}

/// A pattern placed in the song.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PatternPlacement {
    pub pattern: PatternId,
    pub at: Tick,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AudioLaneId(pub String);

/// A lane in the song for one audio file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioLane {
    pub id: AudioLaneId,
    pub path: PathBuf,
    /// Range 0.0..=1.0.
    pub volume: f32,
}

/// The lane's file, played from its start at `at`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AudioPlacement {
    pub lane: AudioLaneId,
    pub at: Tick,
}

/// The whole track: loose clips, placed patterns and audio.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Song {
    pub timeline: Timeline,
    /// Sorted by `(at, pattern)`.
    pub patterns: Vec<PatternPlacement>,
    pub audio_lanes: Vec<AudioLane>,
    /// Sorted by `(at, lane)`.
    pub audio: Vec<AudioPlacement>,
}

/// What the transport plays.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayTarget {
    Pattern(PatternId),
    Song,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub tempo: f64,
    pub time_signature: TimeSignature,
    pub instruments: Vec<Instrument>,
    pub patterns: Vec<Pattern>,
    pub song: Song,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            tempo: 140.0,
            time_signature: TimeSignature::default(),
            instruments: Vec::new(),
            patterns: Vec::new(),
            song: Song::default(),
        }
    }
}

impl Project {
    pub fn instrument(&self, id: &InstrumentId) -> Option<&Instrument> {
        self.instruments.iter().find(|i| &i.id == id)
    }

    pub fn instrument_mut(&mut self, id: &InstrumentId) -> Option<&mut Instrument> {
        self.instruments.iter_mut().find(|i| &i.id == id)
    }

    pub fn instrument_index(&self, id: &InstrumentId) -> Option<usize> {
        self.instruments.iter().position(|i| &i.id == id)
    }

    pub fn pattern(&self, id: &PatternId) -> Option<&Pattern> {
        self.patterns.iter().find(|p| &p.id == id)
    }

    pub fn pattern_mut(&mut self, id: &PatternId) -> Option<&mut Pattern> {
        self.patterns.iter_mut().find(|p| &p.id == id)
    }

    pub fn timeline(&self, container: &Container) -> Option<&Timeline> {
        match container {
            Container::Pattern(id) => self.pattern(id).map(|p| &p.timeline),
            Container::Song => Some(&self.song.timeline),
        }
    }

    pub fn timeline_mut(&mut self, container: &Container) -> Option<&mut Timeline> {
        match container {
            Container::Pattern(id) => self.pattern_mut(id).map(|p| &mut p.timeline),
            Container::Song => Some(&mut self.song.timeline),
        }
    }

    /// Pattern length in ticks: its last block's end rounded up to a whole bar, minimum one bar.
    pub fn pattern_length(&self, id: &PatternId) -> Tick {
        let bar = self.time_signature.ticks_per_bar();
        let end = self.pattern(id).map_or(0, |p| p.timeline.end());
        end.div_ceil(bar).max(1) * bar
    }
}
