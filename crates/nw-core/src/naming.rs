use crate::model::{InstrumentId, PatternId, Project};

/// Smallest N >= 1 such that `pattern-N` is unused.
pub fn next_pattern(project: &Project) -> (PatternId, String) {
    let n = (1u32..)
        .find(|n| project.pattern(&PatternId(format!("pattern-{n}"))).is_none())
        .expect("pattern numbers exhausted");
    (PatternId(format!("pattern-{n}")), format!("pattern {n}"))
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
