use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use nw_core::model::{InstrumentId, PatternId};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteItem {
    Command(&'static str),
    Pattern(PatternId),
    Instrument(InstrumentId),
    Sample(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteEntry {
    pub item: PaletteItem,
    pub label: String,
}

/// Indices of entries matching `query` (fuzzy, smart case), best first; ties keep original order.
/// An empty query returns every index in order.
pub fn rank(query: &str, entries: &[PaletteEntry]) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..entries.len()).collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut scored: Vec<(usize, u32)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let h = nucleo_matcher::Utf32Str::new(&e.label, &mut buf);
            pattern.score(h, &mut matcher).map(|s| (i, s))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().map(|(i, _)| i).collect()
}
