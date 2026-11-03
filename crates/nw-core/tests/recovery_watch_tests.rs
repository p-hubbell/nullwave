use nw_core::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

fn project() -> Project {
    let mut p = Project::default();
    p.tempo = 99.0;
    p.instruments.push(Instrument::new(InstrumentId("kick".into()), "Kick"));
    p
}

#[test]
fn recovery_round_trip_and_clear() {
    let dir = tempfile::tempdir().unwrap();
    let p = project();
    write_recovery(dir.path(), &p).unwrap();
    assert!(dir.path().join(".nullwave/recovery.ron").is_file());
    assert_eq!(read_recovery(dir.path()), Some(p));
    clear_recovery(dir.path()).unwrap();
    assert_eq!(read_recovery(dir.path()), None);
    clear_recovery(dir.path()).unwrap();
}

#[test]
fn corrupt_recovery_reads_as_none() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(read_recovery(dir.path()), None);
    fs::create_dir_all(dir.path().join(".nullwave")).unwrap();
    fs::write(dir.path().join(".nullwave/recovery.ron"), "(tempo: oops").unwrap();
    assert_eq!(read_recovery(dir.path()), None);
}

fn saved(path: &str, content: &[u8]) -> HashMap<PathBuf, u64> {
    HashMap::from([(PathBuf::from(path), hash_bytes(content))])
}

#[test]
fn own_write_is_not_external() {
    let last = saved("/p/song.ron", b"abc");
    let r = external_changes(&[PathBuf::from("/p/song.ron")], &last, |_| Some(b"abc".to_vec()));
    assert!(r.is_empty());
}

#[test]
fn changed_bytes_are_external() {
    let last = saved("/p/song.ron", b"abc");
    let paths = [PathBuf::from("/p/song.ron"), PathBuf::from("/p/instruments/new.ron")];
    let r = external_changes(&paths, &last, |_| Some(b"xyz".to_vec()));
    assert_eq!(r, paths.to_vec());
}

#[test]
fn deleted_saved_file_is_external() {
    let last = saved("/p/song.ron", b"abc");
    let paths = [PathBuf::from("/p/song.ron"), PathBuf::from("/p/other.ron")];
    let r = external_changes(&paths, &last, |_| None);
    assert_eq!(r, vec![PathBuf::from("/p/song.ron")]);
}

#[test]
fn nullwave_dir_is_ignored() {
    let paths = [
        PathBuf::from("/p/.nullwave/recovery.ron"),
        PathBuf::from("/p/notes.txt"),
        PathBuf::from("/p/.gitignore"),
    ];
    let r = external_changes(&paths, &HashMap::new(), |_| Some(b"x".to_vec()));
    assert_eq!(r, vec![PathBuf::from("/p/.gitignore")]);
}

#[test]
fn watcher_reports_hand_edit() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let _w = ProjectWatcher::new(dir.path(), move |paths| {
        let _ = tx.send(paths);
    })
    .unwrap();
    let canon = fs::canonicalize(dir.path()).unwrap();
    fs::write(dir.path().join("song.ron"), "hand edit").unwrap();
    let mut seen: Vec<PathBuf> = Vec::new();
    while let Ok(batch) = rx.recv_timeout(Duration::from_secs(2)) {
        seen.extend(batch);
        if seen.iter().any(|p| p.ends_with("song.ron")) {
            break;
        }
    }
    assert!(seen.iter().any(|p| p == &canon.join("song.ron")), "got {seen:?}");
    assert!(seen.iter().all(|p: &PathBuf| Path::new(p).starts_with(&canon)));
}
