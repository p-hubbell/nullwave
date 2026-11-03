use nw_core::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn id(s: &str) -> InstrumentId {
    InstrumentId(s.into())
}
fn pid(s: &str) -> PatternId {
    PatternId(s.into())
}
fn note(pitch: u8, start: u32) -> Note {
    Note { pitch, start, length: 240, velocity: 100 }
}

fn sample_project() -> Project {
    let mut kick = Instrument::new(id("kick"), "Kick");
    kick.sampler.sample_path = Some("/Volumes/USB/kits/kick.wav".into());
    let snare = Instrument::new(id("snare"), "Snare");
    let mut notes = BTreeMap::new();
    notes.insert(id("kick"), vec![note(36, 0), note(36, 960)]);
    Project {
        tempo: 128.5,
        time_signature: TimeSignature { numerator: 7, denominator: 8 },
        instruments: vec![kick, snare],
        patterns: vec![
            Pattern { id: pid("pattern-1"), name: "Pattern 1".into(), notes },
            Pattern { id: pid("pattern-2"), name: "Pattern 2".into(), notes: BTreeMap::new() },
        ],
    }
}

fn saved() -> (tempfile::TempDir, Project) {
    let t = tempfile::tempdir().unwrap();
    let p = sample_project();
    save_project(t.path(), &p).unwrap();
    (t, p)
}

fn all_files(dir: &Path, out: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            all_files(&p, out);
        } else {
            out.push((p.clone(), fs::read(&p).unwrap()));
        }
    }
    out.sort();
}

#[test]
fn round_trip_equals() {
    let (t, p) = saved();
    let (loaded, warnings) = load_project(t.path()).unwrap();
    assert_eq!(loaded, p);
    assert!(warnings.is_empty());
}

#[test]
fn layout_matches_spec() {
    let (t, _) = saved();
    let d = t.path();
    assert!(d.join("song.ron").is_file());
    assert!(d.join("instruments/kick.ron").is_file());
    assert!(d.join("patterns/pattern-1/kick.ron").is_file());
    assert_eq!(fs::read_to_string(d.join(".gitignore")).unwrap(), ".nullwave/\n");
}

#[test]
fn empty_note_sequences_write_no_file() {
    let (t, _) = saved();
    assert!(!t.path().join("patterns/pattern-2").exists());
    assert!(!t.path().join("patterns/pattern-1/snare.ron").exists());
}

#[test]
fn second_save_is_byte_identical() {
    let (t, p) = saved();
    let mut a = Vec::new();
    all_files(t.path(), &mut a);
    let report = save_project(t.path(), &p).unwrap();
    let mut b = Vec::new();
    all_files(t.path(), &mut b);
    assert_eq!(a, b);
    for (path, hash) in &report.written {
        assert!(path.is_absolute());
        assert_eq!(*hash, hash_bytes(&fs::read(path).unwrap()));
    }
    assert!(report.written.contains_key(&t.path().join("song.ron")));
}

#[test]
fn deleted_instrument_and_pattern_files_are_removed_on_save() {
    let (t, mut p) = saved();
    p.instruments.retain(|i| i.id != id("kick"));
    p.patterns.retain(|x| x.id != pid("pattern-1"));
    save_project(t.path(), &p).unwrap();
    assert!(!t.path().join("instruments/kick.ron").exists());
    assert!(!t.path().join("patterns/pattern-1").exists());
    assert!(t.path().join("instruments/snare.ron").exists());
}

#[test]
fn existing_gitignore_is_left_alone() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join(".gitignore"), "custom\n").unwrap();
    save_project(t.path(), &sample_project()).unwrap();
    assert_eq!(fs::read_to_string(t.path().join(".gitignore")).unwrap(), "custom\n");
}

#[test]
fn sample_path_is_stored_absolute() {
    let (t, _) = saved();
    let text = fs::read_to_string(t.path().join("instruments/kick.ron")).unwrap();
    assert!(text.contains("/Volumes/USB/kits/kick.wav"));
}

#[test]
fn not_a_project_without_song_ron() {
    let t = tempfile::tempdir().unwrap();
    assert!(matches!(load_project(t.path()), Err(LoadError::NotAProject(_))));
}

#[test]
fn conflict_markers_name_file_and_line() {
    let (t, _) = saved();
    let f = t.path().join("patterns/pattern-1/kick.ron");
    let text = fs::read_to_string(&f).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    lines.insert(2, "<<<<<<< HEAD");
    fs::write(&f, lines.join("\n")).unwrap();
    let err = load_project(t.path()).unwrap_err();
    assert!(
        matches!(&err, LoadError::Parse { path, line: 3, .. } if path.ends_with("patterns/pattern-1/kick.ron")),
        "{err:?}"
    );
    assert!(err.to_string().starts_with("patterns/pattern-1/kick.ron:3: "), "{err}");
}

#[test]
fn failed_load_leaves_files_untouched() {
    let (t, _) = saved();
    fs::write(t.path().join("patterns/pattern-1/kick.ron"), "<<<<<<< HEAD\n").unwrap();
    let mut before = Vec::new();
    all_files(t.path(), &mut before);
    assert!(load_project(t.path()).is_err());
    let mut after = Vec::new();
    all_files(t.path(), &mut after);
    assert_eq!(before, after);
}

fn invalid_with(mutate: impl FnOnce(&mut Project), file: &str) {
    let t = tempfile::tempdir().unwrap();
    let mut p = sample_project();
    mutate(&mut p);
    save_project(t.path(), &p).unwrap();
    let err = load_project(t.path()).unwrap_err();
    assert!(
        matches!(&err, LoadError::Invalid { path, .. } if path.ends_with(file)),
        "{err:?}"
    );
}

#[test]
fn out_of_range_values_are_invalid() {
    let k = "patterns/pattern-1/kick.ron";
    invalid_with(|p| p.patterns[0].notes.get_mut(&id("kick")).unwrap()[0].pitch = 200, k);
    invalid_with(|p| p.patterns[0].notes.get_mut(&id("kick")).unwrap()[0].velocity = 0, k);
    invalid_with(|p| p.patterns[0].notes.get_mut(&id("kick")).unwrap()[0].length = 0, k);
    invalid_with(|p| p.instruments[0].pan = 3.0, "instruments/kick.ron");
    invalid_with(|p| p.instruments[0].volume = 1.5, "instruments/kick.ron");
    invalid_with(|p| p.tempo = 5.0, "song.ron");
    invalid_with(|p| p.time_signature.numerator = 0, "song.ron");
    invalid_with(|p| p.time_signature.denominator = 0, "song.ron");
    invalid_with(|p| p.time_signature.denominator = 3, "song.ron");
}

#[test]
fn missing_instrument_file_is_io_error_naming_it() {
    let (t, _) = saved();
    fs::remove_file(t.path().join("instruments/snare.ron")).unwrap();
    let err = load_project(t.path()).unwrap_err();
    assert!(
        matches!(&err, LoadError::Io { path, .. } if path.ends_with("instruments/snare.ron")),
        "{err:?}"
    );
}

#[test]
fn orphan_file_is_warning() {
    let (t, p) = saved();
    fs::write(t.path().join("instruments/ghost.ron"), "garbage").unwrap();
    fs::create_dir_all(t.path().join("patterns/pattern-1")).unwrap();
    fs::write(t.path().join("patterns/pattern-1/ghost.ron"), "garbage").unwrap();
    let (loaded, mut warnings) = load_project(t.path()).unwrap();
    assert_eq!(loaded, p);
    warnings.sort_by_key(|w| format!("{w:?}"));
    assert_eq!(
        warnings,
        vec![
            LoadWarning::OrphanFile(t.path().join("instruments/ghost.ron")),
            LoadWarning::OrphanFile(t.path().join("patterns/pattern-1/ghost.ron")),
        ]
    );
}

#[cfg(unix)]
#[test]
fn unreadable_notes_location_is_io_error_not_empty() {
    use std::os::unix::fs::PermissionsExt;
    let (t, _) = saved();
    let dir = t.path().join("patterns/pattern-1");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
    let result = load_project(t.path());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    // Running as root bypasses permissions; only assert when the block took effect.
    if let Err(err) = result {
        assert!(
            matches!(&err, LoadError::Io { path, .. } if path.ends_with("patterns/pattern-1/kick.ron")),
            "{err:?}"
        );
    } else {
        assert!(fs::metadata(dir.join("kick.ron")).is_ok(), "load succeeded despite blocked dir");
    }
}
