use nw_core::*;

fn iid(s: &str) -> InstrumentId { InstrumentId(s.into()) }
fn pid(s: &str) -> PatternId { PatternId(s.into()) }
fn note(pitch: u8, start: Tick) -> Note { Note { pitch, start, length: 240, velocity: 100 } }
fn key(inst: &str, number: ClipNumber) -> ClipKey { ClipKey { instrument: iid(inst), number } }
fn pat(id: &str) -> Pattern { Pattern { id: pid(id), name: id.into(), timeline: Timeline::default() } }
fn in_pat(id: &str) -> Container { Container::Pattern(pid(id)) }

fn clip(inst: &str, number: ClipNumber, notes: Vec<Note>) -> Clip {
    Clip { key: key(inst, number), start: 0, length: 3840, notes }
}

fn lane(id: &str) -> AudioLane {
    AudioLane { id: AudioLaneId(id.into()), path: format!("/s/{id}.wav").into(), volume: 0.78 }
}

/// Two instruments, two patterns; snare 1 placed twice in pattern-1, a loose snare 1 in
/// the song, pattern-1 placed in the song, and one audio lane with one placement.
fn project() -> Project {
    let mut p = Project::default();
    p.instruments.push(Instrument::new(iid("kick-01"), "Kick 01"));
    p.instruments.push(Instrument::new(iid("snare"), "Snare"));
    let mut p1 = pat("pattern-1");
    p1.timeline.clips.push(clip("snare", 1, vec![note(60, 0)]));
    p1.timeline.placements = vec![Placement { clip: key("snare", 1), at: 0 }, Placement { clip: key("snare", 1), at: 3840 }];
    p.patterns.push(p1);
    p.patterns.push(pat("pattern-3"));
    p.song.timeline.clips.push(clip("snare", 1, vec![note(62, 0)]));
    p.song.timeline.placements.push(Placement { clip: key("snare", 1), at: 960 });
    p.song.patterns.push(PatternPlacement { pattern: pid("pattern-1"), at: 0 });
    p.song.audio_lanes.push(lane("tag"));
    p.song.audio.push(AudioPlacement { lane: AudioLaneId("tag".into()), at: 0 });
    p
}

#[test]
fn next_pattern_fills_first_gap() {
    let p = project();
    assert_eq!(next_pattern(&p), (pid("pattern-2"), "Pattern 2".into()));
}

#[test]
fn instrument_slug_and_collision() {
    let p = project();
    assert_eq!(new_instrument(&p, "Kick 01"), (iid("kick-01-2"), "Kick 01 2".into()));
    assert_eq!(new_instrument(&p, "Hat  Closed!"), (iid("hat-closed"), "Hat  Closed!".into()));
    let mut p = p;
    p.instruments.push(Instrument::new(iid("kick-01-2"), "x"));
    assert_eq!(new_instrument(&p, "Kick 01"), (iid("kick-01-3"), "Kick 01 3".into()));
}

#[test]
fn instrument_slug_of_symbols_is_instrument() {
    let p = project();
    assert_eq!(new_instrument(&p, "!!!").0, iid("instrument"));
}

fn all_edits(p: &Project) -> Vec<Edit> {
    let snare1 = p.patterns[0].timeline.clips[0].clone();
    let mut changed = snare1.clone();
    changed.notes = vec![note(72, 960)];
    changed.length = 960;
    let mut with_clips = pat("pattern-9");
    with_clips.timeline.clips.push(clip("kick-01", 1, vec![note(36, 0)]));
    with_clips.timeline.placements.push(Placement { clip: key("kick-01", 1), at: 0 });
    let mut louder = lane("tag");
    louder.volume = 1.0;
    vec![
        Edit::ReplaceClip { container: in_pat("pattern-3"), before: None, after: Some(clip("kick-01", 1, vec![note(36, 0)])) },
        Edit::ReplaceClip { container: in_pat("pattern-1"), before: Some(snare1.clone()), after: Some(changed) },
        Edit::ReplaceClip { container: Container::Song, before: Some(p.song.timeline.clips[0].clone()), after: None },
        Edit::ReplacePlacements {
            container: in_pat("pattern-1"),
            before: vec![Placement { clip: key("snare", 1), at: 3840 }],
            after: vec![Placement { clip: key("snare", 1), at: 7680 }, Placement { clip: key("snare", 1), at: 0 }],
        },
        Edit::ReplacePatternPlacements {
            before: vec![PatternPlacement { pattern: pid("pattern-1"), at: 0 }],
            after: vec![PatternPlacement { pattern: pid("pattern-3"), at: 3840 }],
        },
        Edit::ReplaceAudioPlacements { before: vec![], after: vec![AudioPlacement { lane: AudioLaneId("tag".into()), at: 960 }] },
        Edit::AddAudioLane { lane: lane("vox"), index: 1 },
        Edit::RemoveAudioLane { lane: lane("tag"), index: 0 },
        Edit::UpdateAudioLane { before: lane("tag"), after: louder },
        Edit::AddInstrument { instrument: Box::new(Instrument::new(iid("hat"), "Hat")), index: 1 },
        remove_instrument_edit(p, &iid("snare")).unwrap(),
        Edit::UpdateInstrument { before: Box::new(p.instruments[0].clone()), after: Box::new({ let mut i = p.instruments[0].clone(); i.volume = 0.1; i }) },
        Edit::AddPattern { pattern: pat("pattern-2"), index: 1 },
        Edit::RemovePattern { pattern: p.patterns[0].clone(), index: 0 },
        Edit::RenamePattern { id: pid("pattern-1"), before: "pattern-1".into(), after: "verse".into() },
        Edit::SetTempo { before: 140.0, after: 90.0 },
        Edit::AddPattern { pattern: with_clips, index: 0 },
    ]
}

#[test]
fn every_edit_inverse_restores_project() {
    let base = project();
    for e in all_edits(&base) {
        let mut p = base.clone();
        e.apply(&mut p);
        assert_ne!(p, base, "edit {e:?} changed nothing");
        e.inverse().apply(&mut p);
        assert_eq!(p, base, "edit {e:?}");
    }
}

#[test]
fn placements_stay_sorted() {
    let mut p = project();
    Edit::ReplacePlacements {
        container: in_pat("pattern-1"),
        before: vec![],
        after: vec![Placement { clip: key("snare", 1), at: 1920 }],
    }
    .apply(&mut p);
    let at: Vec<Tick> = p.patterns[0].timeline.placements.iter().map(|pl| pl.at).collect();
    assert_eq!(at, vec![0, 1920, 3840]);
}

#[test]
fn replace_placements_removes_one_instance_per_listed_placement() {
    let mut p = project();
    let one = Placement { clip: key("snare", 1), at: 0 };
    p.patterns[0].timeline.placements.push(one.clone());
    Edit::ReplacePlacements { container: in_pat("pattern-1"), before: vec![one], after: vec![] }.apply(&mut p);
    assert_eq!(p.patterns[0].timeline.uses(&key("snare", 1)), 2);
}

#[test]
fn clips_stay_sorted_by_key() {
    let mut p = project();
    let tl = in_pat("pattern-1");
    Edit::ReplaceClip { container: tl.clone(), before: None, after: Some(clip("snare", 3, vec![])) }.apply(&mut p);
    Edit::ReplaceClip { container: tl, before: None, after: Some(clip("kick-01", 2, vec![])) }.apply(&mut p);
    let keys: Vec<ClipKey> = p.patterns[0].timeline.clips.iter().map(|c| c.key.clone()).collect();
    assert_eq!(keys, vec![key("kick-01", 2), key("snare", 1), key("snare", 3)]);
}

#[test]
fn undo_redo_group_is_one_step() {
    let mut p = project();
    let mut h = History::new();
    h.commit(&mut p, vec![
        Edit::SetTempo { before: 140.0, after: 100.0 },
        Edit::RenamePattern { id: pid("pattern-1"), before: "pattern-1".into(), after: "a".into() },
    ]);
    assert_eq!(p.tempo, 100.0);
    h.undo(&mut p).unwrap();
    assert_eq!(p, project());
    assert!(h.undo(&mut p).is_none());
    h.redo(&mut p).unwrap();
    assert_eq!(p.tempo, 100.0);
    assert_eq!(p.patterns[0].name, "a");
}

#[test]
fn commit_clears_redo() {
    let mut p = project();
    let mut h = History::new();
    h.commit(&mut p, vec![Edit::SetTempo { before: 140.0, after: 100.0 }]);
    h.undo(&mut p).unwrap();
    h.commit(&mut p, vec![Edit::SetTempo { before: 140.0, after: 90.0 }]);
    assert!(h.redo(&mut p).is_none());
    h.clear();
    assert!(h.undo(&mut p).is_none());
}

#[test]
fn touched_paths() {
    let song = vec![DocPath::Song];
    assert_eq!(Edit::RenamePattern { id: pid("p"), before: "a".into(), after: "b".into() }.touched(), song);
    assert_eq!(Edit::SetTempo { before: 1.0, after: 2.0 }.touched(), song);
    let c = clip("i", 2, vec![]);
    assert_eq!(
        Edit::ReplaceClip { container: in_pat("p"), before: None, after: Some(c.clone()) }.touched(),
        vec![DocPath::Clip(ClipRef { container: in_pat("p"), key: key("i", 2) })]
    );
    assert_eq!(
        Edit::ReplacePlacements { container: in_pat("p"), before: vec![], after: vec![] }.touched(),
        vec![DocPath::PatternLayout(pid("p"))]
    );
    assert_eq!(
        Edit::ReplacePlacements { container: Container::Song, before: vec![], after: vec![] }.touched(),
        vec![DocPath::Arrangement]
    );
    assert_eq!(Edit::ReplacePatternPlacements { before: vec![], after: vec![] }.touched(), vec![DocPath::Arrangement]);
    assert_eq!(Edit::AddAudioLane { lane: lane("t"), index: 0 }.touched(), vec![DocPath::Arrangement]);
    let inst = Instrument::new(iid("i"), "I");
    assert_eq!(
        Edit::UpdateInstrument { before: Box::new(inst.clone()), after: Box::new(inst.clone()) }.touched(),
        vec![DocPath::Instrument(iid("i"))]
    );
    assert_eq!(
        Edit::AddInstrument { instrument: Box::new(inst.clone()), index: 0 }.touched(),
        vec![DocPath::Song, DocPath::Instrument(iid("i"))]
    );
    let mut pt = pat("p");
    pt.timeline.clips.push(clip("a", 1, vec![]));
    pt.timeline.clips.push(clip("b", 1, vec![]));
    assert_eq!(
        Edit::RemovePattern { pattern: pt, index: 0 }.touched(),
        vec![
            DocPath::Song,
            DocPath::PatternLayout(pid("p")),
            DocPath::Clip(ClipRef { container: in_pat("p"), key: key("a", 1) }),
            DocPath::Clip(ClipRef { container: in_pat("p"), key: key("b", 1) }),
        ]
    );
    let p = project();
    let touched = remove_instrument_edit(&p, &iid("snare")).unwrap().touched();
    assert!(touched.contains(&DocPath::Song));
    assert!(touched.contains(&DocPath::Arrangement));
    assert!(touched.contains(&DocPath::PatternLayout(pid("pattern-1"))));
    assert!(touched.contains(&DocPath::Clip(ClipRef { container: Container::Song, key: key("snare", 1) })));
}
