use crate::time::{Tick, TimeSignature};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
pub struct Sampler {
    pub sample_path: Option<PathBuf>,
    pub root_note: u8,
    pub attack_ms: f32,
    pub release_ms: f32,
}

impl Default for Sampler {
    fn default() -> Self {
        Self { sample_path: None, root_note: 60, attack_ms: 0.0, release_ms: 20.0 }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: PatternId,
    pub name: String,
    /// Per-instrument note lists, kept sorted by `(start, pitch)`.
    pub notes: BTreeMap<InstrumentId, Vec<Note>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub tempo: f64,
    pub time_signature: TimeSignature,
    pub instruments: Vec<Instrument>,
    pub patterns: Vec<Pattern>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            tempo: 140.0,
            time_signature: TimeSignature::default(),
            instruments: Vec::new(),
            patterns: Vec::new(),
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

    /// Pattern length in ticks: last note end rounded up to a whole bar, minimum one bar.
    pub fn pattern_length(&self, id: &PatternId) -> Tick {
        let bar = self.time_signature.ticks_per_bar();
        let last_end = self
            .pattern(id)
            .into_iter()
            .flat_map(|p| p.notes.values().flatten())
            .map(Note::end)
            .max()
            .unwrap_or(0);
        last_end.div_ceil(bar).max(1) * bar
    }
}
