use crate::edit::DocPath;
use crate::model::{Instrument, InstrumentId, Note, Pattern, PatternId, Project};
use crate::time::TimeSignature;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::hash::{DefaultHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct SongFile {
    tempo: f64,
    time_signature: TimeSignature,
    instruments: Vec<InstrumentId>,
    patterns: Vec<PatternEntry>,
}

#[derive(Serialize, Deserialize)]
struct PatternEntry {
    id: PatternId,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct NoteFile {
    notes: Vec<Note>,
}

impl DocPath {
    /// Location of this document relative to the project directory.
    pub fn relative_path(&self) -> PathBuf {
        match self {
            DocPath::Song => PathBuf::from("song.ron"),
            DocPath::Instrument(i) => Path::new("instruments").join(format!("{}.ron", i.0)),
            DocPath::Notes(p, i) => {
                Path::new("patterns").join(&p.0).join(format!("{}.ron", i.0))
            }
        }
    }
}

#[derive(Debug)]
pub struct SaveReport {
    /// Absolute path of every file written, with the hash of its content.
    pub written: HashMap<PathBuf, u64>,
}

#[derive(Debug, thiserror::Error)]
#[error("{}: {message}", path.display())]
pub struct SaveError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum LoadError {
    #[error("{} is not a nullwave project (no song.ron)", .0.display())]
    NotAProject(PathBuf),
    #[error("{}: {message}", path.display())]
    Io { path: PathBuf, message: String },
    #[error("{}:{line}: {message}", path.display())]
    Parse { path: PathBuf, line: usize, message: String },
    #[error("{}: {message}", path.display())]
    Invalid { path: PathBuf, message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadWarning {
    /// A file under `instruments/` or `patterns/` that `song.ron` does not reference.
    OrphanFile(PathBuf),
}

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    h.write(bytes);
    h.finish()
}

fn to_ron<T: Serialize>(value: &T) -> String {
    let config = ron::ser::PrettyConfig::new().depth_limit(4).struct_names(false);
    let mut s = ron::ser::to_string_pretty(value, config).expect("model types serialize to RON");
    s.push('\n');
    s
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

pub fn save_project(dir: &Path, project: &Project) -> Result<SaveReport, SaveError> {
    let mut docs: Vec<(DocPath, String)> = Vec::new();
    let song = SongFile {
        tempo: project.tempo,
        time_signature: project.time_signature,
        instruments: project.instruments.iter().map(|i| i.id.clone()).collect(),
        patterns: project
            .patterns
            .iter()
            .map(|p| PatternEntry { id: p.id.clone(), name: p.name.clone() })
            .collect(),
    };
    docs.push((DocPath::Song, to_ron(&song)));
    for inst in &project.instruments {
        docs.push((DocPath::Instrument(inst.id.clone()), to_ron(inst)));
    }
    for pat in &project.patterns {
        for (inst, notes) in &pat.notes {
            if !notes.is_empty() {
                let file = NoteFile { notes: notes.clone() };
                docs.push((DocPath::Notes(pat.id.clone(), inst.clone()), to_ron(&file)));
            }
        }
    }

    let err = |path: &Path, e: io::Error| SaveError { path: path.to_path_buf(), message: e.to_string() };
    fs::create_dir_all(dir).map_err(|e| err(dir, e))?;
    let mut written = HashMap::new();
    for (doc, text) in &docs {
        let path = dir.join(doc.relative_path());
        write_atomic(&path, text.as_bytes()).map_err(|e| err(&path, e))?;
        written.insert(path, hash_bytes(text.as_bytes()));
    }

    for sub in ["instruments", "patterns"] {
        remove_stale(&dir.join(sub), &written).map_err(|e| err(&dir.join(sub), e))?;
    }

    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        write_atomic(&gitignore, b".nullwave/\n").map_err(|e| err(&gitignore, e))?;
    }
    Ok(SaveReport { written })
}

/// Deletes `.ron` files under `root` that were not just written, then empty subfolders.
fn remove_stale(root: &Path, keep: &HashMap<PathBuf, u64>) -> io::Result<()> {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            remove_stale(&path, keep)?;
            if fs::read_dir(&path)?.next().is_none() {
                fs::remove_dir(&path)?;
            }
        } else if path.extension().is_some_and(|e| e == "ron") && !keep.contains_key(&path) {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn read_doc<T: DeserializeOwned>(dir: &Path, rel: &Path) -> Result<T, LoadError> {
    let text = fs::read_to_string(dir.join(rel))
        .map_err(|e| LoadError::Io { path: rel.to_path_buf(), message: e.to_string() })?;
    ron::from_str(&text).map_err(|e| LoadError::Parse {
        path: rel.to_path_buf(),
        line: e.span.start.line,
        message: e.code.to_string(),
    })
}

fn invalid(rel: &Path, message: impl Into<String>) -> LoadError {
    LoadError::Invalid { path: rel.to_path_buf(), message: message.into() }
}

fn validate_song(rel: &Path, song: &SongFile) -> Result<(), LoadError> {
    if !(20.0..=999.0).contains(&song.tempo) {
        return Err(invalid(rel, format!("tempo {} outside 20..=999", song.tempo)));
    }
    let ts = song.time_signature;
    if ts.numerator == 0 || !matches!(ts.denominator, 1 | 2 | 4 | 8 | 16 | 32) {
        return Err(invalid(rel, format!("invalid time signature {}/{}", ts.numerator, ts.denominator)));
    }
    Ok(())
}

fn validate_instrument(rel: &Path, inst: &Instrument) -> Result<(), LoadError> {
    if !(0.0..=1.0).contains(&inst.volume) {
        return Err(invalid(rel, format!("volume {} outside 0..=1", inst.volume)));
    }
    if !(-1.0..=1.0).contains(&inst.pan) {
        return Err(invalid(rel, format!("pan {} outside -1..=1", inst.pan)));
    }
    Ok(())
}

fn validate_notes(rel: &Path, notes: &[Note]) -> Result<(), LoadError> {
    for n in notes {
        if n.pitch > 127 {
            return Err(invalid(rel, format!("pitch {} above 127", n.pitch)));
        }
        if !(1..=127).contains(&n.velocity) {
            return Err(invalid(rel, format!("velocity {} outside 1..=127", n.velocity)));
        }
        if n.length == 0 {
            return Err(invalid(rel, "note length must be greater than 0"));
        }
    }
    Ok(())
}

pub fn load_project(dir: &Path) -> Result<(Project, Vec<LoadWarning>), LoadError> {
    let song_rel = DocPath::Song.relative_path();
    if !dir.join(&song_rel).is_file() {
        return Err(LoadError::NotAProject(dir.to_path_buf()));
    }
    let song: SongFile = read_doc(dir, &song_rel)?;
    validate_song(&song_rel, &song)?;

    let mut instruments = Vec::new();
    for id in &song.instruments {
        let rel = DocPath::Instrument(id.clone()).relative_path();
        let inst: Instrument = read_doc(dir, &rel)?;
        validate_instrument(&rel, &inst)?;
        instruments.push(inst);
    }

    let mut known: HashSet<PathBuf> = HashSet::new();
    known.insert(dir.join(&song_rel));
    for id in &song.instruments {
        known.insert(dir.join(DocPath::Instrument(id.clone()).relative_path()));
    }

    let mut patterns = Vec::new();
    for entry in &song.patterns {
        let mut notes = BTreeMap::new();
        for id in &song.instruments {
            let rel = DocPath::Notes(entry.id.clone(), id.clone()).relative_path();
            let abs = dir.join(&rel);
            known.insert(abs.clone());
            let exists = abs
                .try_exists()
                .map_err(|e| LoadError::Io { path: rel.clone(), message: e.to_string() })?;
            if !exists {
                continue;
            }
            let file: NoteFile = read_doc(dir, &rel)?;
            validate_notes(&rel, &file.notes)?;
            if !file.notes.is_empty() {
                notes.insert(id.clone(), file.notes);
            }
        }
        patterns.push(Pattern { id: entry.id.clone(), name: entry.name.clone(), notes });
    }

    let mut warnings = Vec::new();
    for sub in ["instruments", "patterns"] {
        collect_orphans(&dir.join(sub), &known, &mut warnings);
    }
    warnings.sort_by(|a, b| match (a, b) {
        (LoadWarning::OrphanFile(a), LoadWarning::OrphanFile(b)) => a.cmp(b),
    });

    let project = Project {
        tempo: song.tempo,
        time_signature: song.time_signature,
        instruments,
        patterns,
    };
    Ok((project, warnings))
}

fn collect_orphans(root: &Path, known: &HashSet<PathBuf>, out: &mut Vec<LoadWarning>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            collect_orphans(&path, known, out);
        } else if path.extension().is_some_and(|e| e == "ron") && !known.contains(&path) {
            out.push(LoadWarning::OrphanFile(path));
        }
    }
}
