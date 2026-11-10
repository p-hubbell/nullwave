use nw_workspace::*;
use std::fs;
use std::path::PathBuf;

#[test]
fn settings_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/settings.ron");
    let s = AppSettings {
        sample_folders: vec![PathBuf::from("/Volumes/USB/Drums"), PathBuf::from("/tmp/x")],
        last_project: Some(PathBuf::from("/tmp/proj")),
    };
    save_settings(&path, &s).unwrap();
    assert_eq!(load_settings(&path), s);
    assert_eq!(load_settings(&dir.path().join("missing.ron")), AppSettings::default());
}

#[test]
fn corrupt_settings_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.ron");
    fs::write(&path, "((( not ron").unwrap();
    assert_eq!(load_settings(&path), AppSettings::default());
}

#[test]
fn lists_dirs_then_audio_sorted_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir(p.join("beta")).unwrap();
    fs::create_dir(p.join("Alpha")).unwrap();
    fs::write(p.join("snare.wav"), b"").unwrap();
    fs::write(p.join("Kick.flac"), b"").unwrap();
    let e = list_dir(p).unwrap();
    let names: Vec<_> = e.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Alpha", "beta", "Kick.flac", "snare.wav"]);
    assert!(e[0].is_dir && e[1].is_dir && !e[2].is_dir);
    assert_eq!(e[2].path, p.join("Kick.flac"));
}

#[test]
fn skips_hidden_and_non_audio() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(p.join(".hidden.wav"), b"").unwrap();
    fs::create_dir(p.join(".git")).unwrap();
    fs::write(p.join("notes.txt"), b"").unwrap();
    fs::write(p.join("noext"), b"").unwrap();
    fs::write(p.join("a.ogg"), b"").unwrap();
    let names: Vec<_> = list_dir(p).unwrap().into_iter().map(|b| b.name).collect();
    assert_eq!(names, ["a.ogg"]);
}

#[test]
fn uppercase_extension_counts() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("KICK.WAV"), b"").unwrap();
    assert_eq!(list_dir(dir.path()).unwrap().len(), 1);
}

#[test]
fn missing_folder_is_err() {
    let dir = tempfile::tempdir().unwrap();
    assert!(list_dir(&dir.path().join("nope")).is_err());
    fs::write(dir.path().join("f.wav"), b"").unwrap();
    assert!(list_dir(&dir.path().join("f.wav")).is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_followed_broken_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir(p.join("real")).unwrap();
    std::os::unix::fs::symlink(p.join("real"), p.join("link")).unwrap();
    std::os::unix::fs::symlink(p.join("gone"), p.join("broken")).unwrap();
    let e = list_dir(p).unwrap();
    let names: Vec<_> = e.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["link", "real"]);
    assert!(e.iter().all(|b| b.is_dir));
}
