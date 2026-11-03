use nw_core::*;
use std::collections::BTreeMap;

fn pat_with(notes: &[(&str, Tick, Tick)]) -> (Project, PatternId) {
    let id = PatternId("p".into());
    let mut map: BTreeMap<InstrumentId, Vec<Note>> = BTreeMap::new();
    for (inst, start, length) in notes {
        map.entry(InstrumentId((*inst).into())).or_default().push(Note {
            pitch: 60,
            start: *start,
            length: *length,
            velocity: DEFAULT_VELOCITY,
        });
    }
    let mut p = Project::default();
    p.patterns.push(Pattern { id: id.clone(), name: "P".into(), notes: map });
    (p, id)
}

#[test]
fn bar_is_3840_ticks_in_4_4() {
    assert_eq!(TimeSignature::default().ticks_per_bar(), 3840);
}
#[test]
fn bar_is_2880_ticks_in_3_4() {
    assert_eq!(TimeSignature { numerator: 3, denominator: 4 }.ticks_per_bar(), 2880);
}
#[test]
fn empty_pattern_is_one_bar() {
    let (p, id) = pat_with(&[]);
    assert_eq!(p.pattern_length(&id), 3840);
}
#[test]
fn length_rounds_last_note_end_up_to_bar() {
    let (p, id) = pat_with(&[("a", 3000, 1000)]);
    assert_eq!(p.pattern_length(&id), 7680);
}
#[test]
fn length_uses_last_end_across_all_instruments() {
    let (p, id) = pat_with(&[("a", 0, 900), ("b", 3000, 841)]);
    assert_eq!(p.pattern_length(&id), 7680);
}
#[test]
fn note_ending_exactly_on_bar_does_not_add_a_bar() {
    let (p, id) = pat_with(&[("a", 3000, 840)]);
    assert_eq!(p.pattern_length(&id), 3840);
}
#[test]
fn unknown_pattern_length_is_one_bar() {
    assert_eq!(Project::default().pattern_length(&PatternId("nope".into())), 3840);
}
#[test]
fn defaults() {
    assert_eq!(Project::default().tempo, 140.0);
    assert_eq!(Instrument::new(InstrumentId("k".into()), "Kick").volume, 0.78);
    assert_eq!(Sampler::default().root_note, 60);
}
#[test]
fn lookups_and_note_end() {
    let mut p = Project::default();
    let id = InstrumentId("k".into());
    p.instruments.push(Instrument::new(id.clone(), "Kick"));
    assert_eq!(p.instrument_index(&id), Some(0));
    assert!(p.instrument(&id).is_some());
    p.instrument_mut(&id).unwrap().pan = -1.0;
    assert_eq!(p.instrument(&id).unwrap().pan, -1.0);
    assert!(p.pattern(&PatternId("x".into())).is_none());
    assert_eq!(Note { pitch: 60, start: 10, length: 5, velocity: 100 }.end(), 15);
}
