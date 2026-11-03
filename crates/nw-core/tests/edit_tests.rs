use nw_core::*;

fn iid(s: &str) -> InstrumentId { InstrumentId(s.into()) }
fn pid(s: &str) -> PatternId { PatternId(s.into()) }
fn note(pitch: u8, start: Tick) -> Note { Note { pitch, start, length: 240, velocity: 100 } }
fn pat(id: &str) -> Pattern { Pattern { id: pid(id), name: id.into(), notes: Default::default() } }

fn project() -> Project {
    let mut p = Project::default();
    p.instruments.push(Instrument::new(iid("kick-01"), "Kick 01"));
    p.instruments.push(Instrument::new(iid("snare"), "Snare"));
    p.patterns.push(pat("pattern-1"));
    p.patterns.push(pat("pattern-3"));
    p
}

fn sp(p: &Project, pat: &str, inst: &str) -> Vec<(Tick, u8)> {
    p.pattern(&pid(pat)).unwrap().notes[&iid(inst)].iter().map(|n| (n.start, n.pitch)).collect()
}

#[test]
fn next_pattern_fills_first_gap() {
    let p = project();
    assert_eq!(next_pattern(&p), (pid("pattern-2"), "pattern 2".into()));
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

#[test]
fn add_notes_keeps_sorted_by_start_then_pitch() {
    let mut p = project();
    Edit::AddNotes { pattern: pid("pattern-1"), instrument: iid("snare"),
        notes: vec![note(60, 480), note(64, 0), note(62, 0)] }.apply(&mut p);
    assert_eq!(sp(&p, "pattern-1", "snare"), vec![(0, 62), (0, 64), (480, 60)]);
}

fn all_edits(p: &Project) -> Vec<Edit> {
    let mut with_notes = pat("pattern-9");
    with_notes.notes.insert(iid("snare"), vec![note(60, 0)]);
    vec![
        Edit::AddNotes { pattern: pid("pattern-1"), instrument: iid("snare"), notes: vec![note(61, 5), note(60, 0)] },
        Edit::RemoveNotes { pattern: pid("pattern-1"), instrument: iid("snare"), notes: vec![note(60, 0)] },
        Edit::ReplaceNotes { pattern: pid("pattern-1"), instrument: iid("snare"), before: vec![note(60, 0)], after: vec![note(72, 960)] },
        Edit::AddInstrument { instrument: Instrument::new(iid("hat"), "Hat"), index: 1 },
        remove_instrument_edit(p, &iid("snare")).unwrap(),
        Edit::UpdateInstrument { before: p.instruments[0].clone(), after: { let mut i = p.instruments[0].clone(); i.volume = 0.1; i } },
        Edit::AddPattern { pattern: pat("pattern-2"), index: 1 },
        Edit::RemovePattern { pattern: p.patterns[0].clone(), index: 0 },
        Edit::RenamePattern { id: pid("pattern-1"), before: "pattern-1".into(), after: "verse".into() },
        Edit::SetTempo { before: 140.0, after: 90.0 },
        Edit::AddPattern { pattern: with_notes, index: 0 },
    ]
}

#[test]
fn every_edit_inverse_restores_project() {
    let mut base = project();
    base.pattern_mut(&pid("pattern-1")).unwrap().notes.insert(iid("snare"), vec![note(60, 0)]);
    for e in all_edits(&base) {
        let mut p = base.clone();
        e.apply(&mut p);
        e.inverse().apply(&mut p);
        assert_eq!(p, base, "edit {e:?}");
    }
}

#[test]
fn removing_last_note_removes_map_entry() {
    let mut p = project();
    p.pattern_mut(&pid("pattern-1")).unwrap().notes.insert(iid("snare"), vec![note(60, 0)]);
    Edit::RemoveNotes { pattern: pid("pattern-1"), instrument: iid("snare"), notes: vec![note(60, 0)] }.apply(&mut p);
    assert!(!p.pattern(&pid("pattern-1")).unwrap().notes.contains_key(&iid("snare")));
}

#[test]
fn remove_notes_removes_one_instance_per_listed_note() {
    let mut p = project();
    p.pattern_mut(&pid("pattern-1")).unwrap().notes.insert(iid("snare"), vec![note(60, 0), note(60, 0)]);
    Edit::RemoveNotes { pattern: pid("pattern-1"), instrument: iid("snare"), notes: vec![note(60, 0)] }.apply(&mut p);
    assert_eq!(sp(&p, "pattern-1", "snare"), vec![(0, 60)]);
}

#[test]
fn remove_instrument_captures_and_restores_notes_in_all_patterns() {
    let mut p = project();
    p.pattern_mut(&pid("pattern-1")).unwrap().notes.insert(iid("snare"), vec![note(60, 0)]);
    p.pattern_mut(&pid("pattern-3")).unwrap().notes.insert(iid("snare"), vec![note(62, 10)]);
    let before = p.clone();
    let mut h = History::new();
    let e = remove_instrument_edit(&p, &iid("snare")).unwrap();
    h.commit(&mut p, vec![e]);
    assert!(p.instrument(&iid("snare")).is_none());
    assert!(p.patterns.iter().all(|pt| pt.notes.is_empty()));
    h.undo(&mut p).unwrap();
    assert_eq!(p, before);
    assert!(remove_instrument_edit(&p, &iid("nope")).is_none());
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
    assert_eq!(
        Edit::AddNotes { pattern: pid("p"), instrument: iid("i"), notes: vec![] }.touched(),
        vec![DocPath::Notes(pid("p"), iid("i"))]
    );
    let inst = Instrument::new(iid("i"), "I");
    assert_eq!(
        Edit::UpdateInstrument { before: inst.clone(), after: inst.clone() }.touched(),
        vec![DocPath::Instrument(iid("i"))]
    );
    assert_eq!(
        Edit::AddInstrument { instrument: inst.clone(), index: 0 }.touched(),
        vec![DocPath::Song, DocPath::Instrument(iid("i"))]
    );
    let mut pt = pat("p");
    pt.notes.insert(iid("a"), vec![note(60, 0)]);
    pt.notes.insert(iid("b"), vec![note(60, 0)]);
    assert_eq!(
        Edit::RemovePattern { pattern: pt, index: 0 }.touched(),
        vec![DocPath::Song, DocPath::Notes(pid("p"), iid("a")), DocPath::Notes(pid("p"), iid("b"))]
    );
}
