use crate::model::{AudioLaneId, InstrumentId, PatternId, Project};
use std::path::Path;

/// Smallest N >= 1 such that `pattern-N` is unused.
pub fn next_pattern(project: &Project) -> (PatternId, String) {
    let n = (1u32..)
        .find(|n| project.pattern(&PatternId(format!("pattern-{n}"))).is_none())
        .expect("pattern numbers exhausted");
    (PatternId(format!("pattern-{n}")), format!("Pattern {n}"))
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(c.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "instrument".to_string()
    } else {
        slug
    }
}

/// Unique id (slug of `base_name`) and display name; collisions get `-2`/` 2`, `-3`/` 3`...
pub fn new_instrument(project: &Project, base_name: &str) -> (InstrumentId, String) {
    let slug = slugify(base_name);
    let id = InstrumentId(slug.clone());
    if project.instrument(&id).is_none() {
        return (id, base_name.to_string());
    }
    let n = (2u32..)
        .find(|n| project.instrument(&InstrumentId(format!("{slug}-{n}"))).is_none())
        .expect("instrument numbers exhausted");
    (InstrumentId(format!("{slug}-{n}")), format!("{base_name} {n}"))
}

/// Unique audio lane id from the file's name (`My Tag.wav` → `my-tag`, then `my-tag-2`...).
pub fn new_audio_lane(project: &Project, path: &Path) -> AudioLaneId {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let slug = match slugify(&stem).as_str() {
        "instrument" => "audio".to_string(),
        s => s.to_string(),
    };
    let taken = |id: &str| project.song.audio_lanes.iter().any(|l| l.id.0 == id);
    if !taken(&slug) {
        return AudioLaneId(slug);
    }
    let n = (2u32..).find(|n| !taken(&format!("{slug}-{n}"))).expect("lane numbers exhausted");
    AudioLaneId(format!("{slug}-{n}"))
}
