use nw_core::*;

/// A pattern with one clip per `(instrument, clip start, clip length)`, each placed once.
fn pat_with(clips: &[(&str, Tick, Tick)]) -> (Project, PatternId) {
    let id = PatternId("p".into());
    let mut timeline = Timeline::default();
    for (n, (inst, at, length)) in clips.iter().enumerate() {
        let key = ClipKey { instrument: InstrumentId((*inst).into()), number: n as ClipNumber + 1 };
        timeline.clips.push(Clip::new(key.clone(), *length));
        timeline.placements.push(Placement { clip: key, at: *at });
    }
    let mut p = Project::default();
    p.patterns.push(Pattern { id: id.clone(), name: "P".into(), timeline });
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
fn length_rounds_last_clip_end_up_to_bar() {
    let (p, id) = pat_with(&[("a", 3000, 1000)]);
    assert_eq!(p.pattern_length(&id), 7680);
}
#[test]
fn length_uses_last_end_across_all_instruments() {
    let (p, id) = pat_with(&[("a", 0, 900), ("b", 3000, 841)]);
    assert_eq!(p.pattern_length(&id), 7680);
}
#[test]
fn clip_ending_exactly_on_bar_does_not_add_a_bar() {
    let (p, id) = pat_with(&[("a", 3000, 840)]);
    assert_eq!(p.pattern_length(&id), 3840);
}
#[test]
fn unknown_pattern_length_is_one_bar() {
    assert_eq!(Project::default().pattern_length(&PatternId("nope".into())), 3840);
}
#[test]
fn clip_placed_at_bar_three_makes_three_bars() {
    let (p, id) = pat_with(&[("a", 7680, 3840)]);
    assert_eq!(p.pattern_length(&id), 11520);
}
#[test]
fn timelines_by_container() {
    let (mut p, id) = pat_with(&[("a", 0, 960)]);
    assert_eq!(p.timeline(&Container::Pattern(id.clone())).unwrap().placements.len(), 1);
    assert!(p.timeline(&Container::Pattern(PatternId("nope".into()))).is_none());
    p.timeline_mut(&Container::Song).unwrap().placements.clear();
    assert!(p.timeline(&Container::Song).unwrap().clips.is_empty());
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
