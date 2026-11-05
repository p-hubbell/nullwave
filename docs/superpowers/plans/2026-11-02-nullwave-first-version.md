# Nullwave First Version Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A macOS DAW shaped like VS Code where the user creates instruments (samplers) and patterns, writes notes in a piano roll, hears patterns loop, and saves everything as a folder of RON files.

**Architecture:** A Cargo workspace with three pure-Rust library crates and one GPUI app. `nw-core` owns the project model, edits/undo and file I/O. `nw-engine` owns the real-time audio engine (sample loading, sampler voices, scheduler, cpal output). `nw-workspace` owns UI state that needs no GPUI: tabs, panels, dirty tracking, commands, keymap, palette ranking, sample browsing and piano-roll geometry. The `nullwave` binary is a thin GPUI layer over those crates. Nearly all logic is unit-tested headless. GPUI views are verified by running the app.

**Tech Stack:** Rust 1.97 (edition 2024), `gpui =0.2.2`, `cpal 0.18`, `rtrb 0.4`, `basedrop 0.1`, `assert_no_alloc 1.1`, `symphonia 0.6`, `serde 1` + `ron 0.12`, `notify 8`, `nucleo-matcher 0.3`, `thiserror 2`, `dirs 6`.

**Spec:** `docs/superpowers/specs/2026-11-02-nullwave-design.md` (background: `references/initial-concept.md`). Read both before starting any task.

## Global Constraints

- macOS only for this version; don't use APIs that rule out Windows/Linux later.
- One window. **Nothing ever opens as a floating window:** instruments, settings and prompts are tabs, panels or in-window overlays. The only exceptions are the native file open/save panels for picking a project folder.
- Opening something that's already open focuses its existing tab and never duplicates it. Nothing splits, appears or rearranges on its own.
- Audio thread: no allocation, locks, file/network I/O or logging. It talks to the UI only through preallocated `rtrb` queues. Old data is freed off-thread via `basedrop`.
- Project format: RON, one file per instrument and one per non-empty pattern×instrument note sequence. `.nullwave/` is git-ignored workspace/recovery state.
- Samples are referenced by absolute path and never copied into the project.
- Pattern loop length is the end of the last note across all instruments, rounded up to a whole bar, with a minimum of one bar.
- Saving is explicit (⌘S). Undo/redo lasts for one session only.
- Default keybindings follow VS Code: ⌘⇧P palette, ⌘P quick open, ⌘W close tab, ⌘B sidebar, ⌘J bottom panel, Space play/stop. Everything is rebindable.
- Dark theme by default, calm and VS Code/Zed-like. Don't use `gpui-component`; build a small in-house component layer (concept doc: keep UI code behind your own components).
- Not in this version: plugins, MIDI input, a record button, mixer, synth, effects, arrangement, export, git UI and a right-dock panel (the layout leaves room for one, but it's empty).

## Review Focus

1. **A sample file is missing, unreadable or in an unsupported format** (USB unplugged, a project opened on another machine). The project still opens, that instrument is silent, its rack row shows "missing sample", and nothing crashes. *Pinned in Task 5 and Task 7.*
2. **A project file is hand-edited badly or has git conflict markers.** Opening fails with a message naming the file and line, and nothing on disk is overwritten. *Pinned in Task 3.*
3. **Notes are edited or deleted while the pattern is playing, then Stop is pressed.** No note hangs and no voice keeps sounding after its length or after Stop. *Pinned in Task 7.*
4. **More simultaneous notes than the voice pool holds** (chords on many instruments). The oldest voice is stolen, nothing allocates, and nothing panics. *Pinned in Task 7.*
5. **An instrument or pattern is deleted while it's open in tabs or listed in a saved `workspace.ron`.** Its tabs close, the restore skips stale tabs, and nothing crashes. *Pinned in Task 9.*

---

## File Structure

```
Cargo.toml                         # workspace; shared dependency versions
.gitignore                         # target/, .superpowers/
crates/nw-core/src/
  lib.rs
  time.rs                          # PPQ, Tick, TimeSignature
  model.rs                         # Note, Instrument, Sampler, Pattern, Project, ids, pattern_length
  naming.rs                        # new pattern / instrument ids and names
  edit.rs                          # Edit enum: apply, inverse, touched DocPaths
  history.rs                       # undo/redo stacks
  io/mod.rs
  io/files.rs                      # RON file shapes, save_project, load_project
  io/recovery.rs                   # crash-recovery snapshot
  io/watch.rs                      # notify watcher + external-change classification
crates/nw-engine/src/
  lib.rs
  sample.rs                        # SampleBuffer, load_sample, SampleCache
  voice.rs                         # Voice, VoiceParams, pan_gains, playback_rate
  process.rs                       # sample processing (region, reverse, normalize, declick, loop crossfade), ProcessedCache
  env.rs                           # DAHDSR envelope with curves
  filter.rs                        # state-variable filter (LP/HP/BP/notch)
  lfo.rs                           # LFO shapes, free and tempo-synced rates
  snapshot.rs                      # Snapshot, compile_snapshot
  core.rs                          # EngineCore: transport, scheduler, voice pool, process
  handle.rs                        # EngineHandle: queues, collector, cpal stream
crates/nw-workspace/src/
  lib.rs
  tabs.rs                          # TabKind, Pane, WorkspaceState, tab_title, persistence
  dirty.rs                         # DirtyTracker
  commands.rs                      # command ids, default bindings, user keymap merge
  palette.rs                       # PaletteEntry, rank
  settings.rs                      # AppSettings (sample folders, last project)
  browser.rs                       # list_dir for sample folders
  piano_roll.rs                    # PianoRollView geometry, hit_test, moved, resized, place_note
crates/nullwave/src/
  main.rs                          # app start, global allocator, window, keybindings
  app_state.rs                     # AppState entity: glue between model, history, engine, workspace
  theme.rs                         # colors, sizes
  ui/mod.rs
  ui/components.rs                 # small in-house components (list row, icon button, tab)
  ui/shell.rs                      # root layout: activity bar | sidebar | editor+panel, status bar
  ui/sidebar.rs                    # project / presets / sample folders sections
  ui/editor_area.rs                # panes, tab bar, active editor dispatch
  ui/pattern_tab.rs                # channel rack
  ui/instrument_tab.rs             # sampler settings
  ui/piano_roll.rs                 # piano roll editor
  ui/bottom_panel.rs               # velocity lane, Log
  ui/status_bar.rs
  ui/palette.rs                    # command palette / quick open overlay
  ui/modal.rs                      # in-window prompts (unsaved quit, reload conflict, recovery, errors)
```

Test files live in each crate's `tests/` directory under the names given in each task.

---

### Task 1: Workspace scaffold, time and project model

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/nw-core/Cargo.toml`, `crates/nw-core/src/{lib.rs,time.rs,model.rs}`, stub `Cargo.toml` + `src/lib.rs` for `nw-engine` and `nw-workspace`, stub `crates/nullwave/src/main.rs` (prints nothing, exits)
- Test: `crates/nw-core/tests/model_tests.rs`

**Interfaces:**
- Produces (in `nw_core`):
  - `pub const PPQ: u32 = 960; pub type Tick = u32;`
  - `TimeSignature { numerator: u8, denominator: u8 }` with `Default` 4/4 and `fn ticks_per_bar(&self) -> Tick` (= `PPQ * 4 * numerator / denominator`)
  - `InstrumentId(pub String)`, `PatternId(pub String)`: newtypes with `#[serde(transparent)]` that derive `Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord`
  - `Note { pitch: u8, start: Tick, length: Tick, velocity: u8 }` with `fn end(&self) -> Tick`; `pub const DEFAULT_VELOCITY: u8 = 100;`
  - `Sampler { sample_path: Option<PathBuf>, root_note: u8, attack_ms: f32, release_ms: f32 }`, with defaults `None, 60, 0.0, 20.0`
  - `Instrument { id: InstrumentId, name: String, volume: f32, pan: f32, sampler: Sampler }`, with defaults volume `0.78` and pan `0.0` (range −1..=1)
  - `Pattern { id: PatternId, name: String, notes: BTreeMap<InstrumentId, Vec<Note>> }`. Note vectors are kept sorted by `(start, pitch)`.
  - `Project { tempo: f64, time_signature: TimeSignature, instruments: Vec<Instrument>, patterns: Vec<Pattern> }`, with `Project::default()` tempo `140.0`, 4/4, empty
  - `impl Project { fn instrument(&self, &InstrumentId) -> Option<&Instrument>; fn instrument_mut(..); fn instrument_index(&self, &InstrumentId) -> Option<usize>; fn pattern(&self, &PatternId) -> Option<&Pattern>; fn pattern_mut(..); fn pattern_length(&self, &PatternId) -> Tick }`
  - All model types derive `Clone, Debug, PartialEq, Serialize, Deserialize`.

- [ ] **Step 1: Run `git init`, write `.gitignore` (`target/`, `.superpowers/`, `.DS_Store`) and the workspace `Cargo.toml`** listing the four crates under `crates/*`, `resolver = "3"`, edition 2024, with `[workspace.dependencies]` pinning the tech-stack versions above. `gpui` must be pinned as `"=0.2.2"`.

- [ ] **Step 2: Write the failing tests**

```rust
#[test] fn bar_is_3840_ticks_in_4_4() { assert_eq!(TimeSignature::default().ticks_per_bar(), 3840); }
#[test] fn bar_is_2880_ticks_in_3_4() { assert_eq!(TimeSignature { numerator: 3, denominator: 4 }.ticks_per_bar(), 2880); }
#[test] fn empty_pattern_is_one_bar() { /* pattern with no notes */ assert_eq!(p.pattern_length(&id), 3840); }
#[test] fn length_rounds_last_note_end_up_to_bar() { /* note start 3000 len 1000 → end 4000 */ assert_eq!(len, 7680); }
#[test] fn length_uses_last_end_across_all_instruments() { /* inst a ends 900, inst b ends 3841 */ assert_eq!(len, 7680); }
#[test] fn note_ending_exactly_on_bar_does_not_add_a_bar() { /* end 3840 */ assert_eq!(len, 3840); }
#[test] fn unknown_pattern_length_is_one_bar() { assert_eq!(Project::default().pattern_length(&PatternId("nope".into())), 3840); }
#[test] fn defaults() { assert_eq!(Project::default().tempo, 140.0); assert_eq!(Instrument::new(id, "Kick").volume, 0.78); assert_eq!(Sampler::default().root_note, 60); }
```

- [ ] **Step 3: Run** `cargo test -p nw-core`. Expected: compile errors or FAIL.
- [ ] **Step 4: Implement `time.rs` and `model.rs`.** Also add `Instrument::new(id: InstrumentId, name: impl Into<String>) -> Instrument` with the defaults above.
- [ ] **Step 5: Run** `cargo test -p nw-core` and `cargo build --workspace`. Expected: all pass, and the workspace builds.
- [ ] **Step 6: Commit** the scaffold, `docs/` and `references/` with the message `chore: scaffold workspace and project model`.

---

### Task 2: Naming, edits and undo history

**Files:**
- Create: `crates/nw-core/src/{naming.rs,edit.rs,history.rs}`
- Test: `crates/nw-core/tests/edit_tests.rs`

**Interfaces:**
- Consumes: Task 1 model.
- Produces:
  - `naming::next_pattern(&Project) -> (PatternId, String)`: smallest N ≥ 1 where `pattern-N` is unused, giving `(PatternId("pattern-N"), "pattern N")`
  - `naming::new_instrument(&Project, base_name: &str) -> (InstrumentId, String)`: slugifies the id (lowercase ASCII alphanumerics; every other run of characters becomes one `-`; trimmed; empty becomes `instrument`). On collision it appends `-2`, `-3`… to the id and ` 2`, ` 3`… to the name.
  - `DocPath`, the unit of dirtiness and of on-disk files: `enum DocPath { Song, Instrument(InstrumentId), Notes(PatternId, InstrumentId) }`
  - `enum Edit`:
    - `AddNotes { pattern, instrument, notes: Vec<Note> }`
    - `RemoveNotes { pattern, instrument, notes: Vec<Note> }`
    - `ReplaceNotes { pattern, instrument, before: Vec<Note>, after: Vec<Note> }` (move, resize, velocity)
    - `AddInstrument { instrument: Instrument, index: usize }`
    - `RemoveInstrument { instrument: Instrument, index: usize, notes: Vec<(PatternId, Vec<Note>)> }`
    - `UpdateInstrument { before: Instrument, after: Instrument }`
    - `AddPattern { pattern: Pattern, index: usize }`
    - `RemovePattern { pattern: Pattern, index: usize }`
    - `RenamePattern { id: PatternId, before: String, after: String }`
    - `SetTempo { before: f64, after: f64 }`
  - `impl Edit { fn apply(&self, &mut Project); fn inverse(&self) -> Edit; fn touched(&self) -> Vec<DocPath> }`
  - `History` with `fn commit(&mut self, &mut Project, Vec<Edit>) -> Vec<DocPath>` (applies the group, clears redo), `fn undo(&mut self, &mut Project) -> Option<Vec<DocPath>>`, `fn redo(..) -> Option<Vec<DocPath>>` and `fn clear(&mut self)`. One `commit` call is one undo step.
  - `pub fn remove_instrument_edit(&Project, &InstrumentId) -> Option<Edit>`: builds a `RemoveInstrument` that captures the instrument's notes in every pattern.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn next_pattern_fills_first_gap() { /* has pattern-1, pattern-3 */ assert_eq!(next_pattern(&p), (PatternId("pattern-2".into()), "pattern 2".into())); }
#[test] fn instrument_slug_and_collision() { /* existing id "kick-01" */ assert_eq!(new_instrument(&p, "Kick 01"), (InstrumentId("kick-01-2".into()), "Kick 01 2".into())); }
#[test] fn instrument_slug_of_symbols_is_instrument() { assert_eq!(new_instrument(&p, "!!!").0, InstrumentId("instrument".into())); }
#[test] fn add_notes_keeps_sorted_by_start_then_pitch() { /* add (480,60),(0,64),(0,62) */ assert_eq!(starts_pitches, vec![(0,62),(0,64),(480,60)]); }
#[test] fn every_edit_inverse_restores_project() { /* for each Edit variant: apply then inverse().apply → project == original */ }
#[test] fn removing_last_note_removes_map_entry() { assert!(!pattern.notes.contains_key(&inst)); }
#[test] fn remove_instrument_captures_and_restores_notes_in_all_patterns() { /* undo restores notes in pattern-1 and pattern-2 */ }
#[test] fn undo_redo_group_is_one_step() { /* commit 2 edits; undo once → both reverted; redo → both back */ }
#[test] fn commit_clears_redo() { assert!(h.redo(&mut p).is_none()); }
#[test] fn touched_paths() { assert_eq!(Edit::RenamePattern{..}.touched(), vec![DocPath::Song]); /* AddNotes → Notes(p,i); UpdateInstrument → Instrument(i); AddInstrument → [Song, Instrument(i)]; RemovePattern → [Song] + Notes(p,i) for each instrument with notes */ }
```

- [ ] **Step 2: Run** `cargo test -p nw-core --test edit_tests`. Expected: FAIL.
- [ ] **Step 3: Implement `naming.rs`, `edit.rs` and `history.rs`.** `RemoveNotes` removes by value equality, one instance per listed note.
- [ ] **Step 4: Run** `cargo test -p nw-core`. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(core): edits, undo history, naming`.

---

### Task 3: Project folder save and load

**Files:**
- Create: `crates/nw-core/src/io/{mod.rs,files.rs}`
- Test: `crates/nw-core/tests/io_tests.rs` (use the `tempfile` dev-dependency)

**Interfaces:**
- Consumes: Task 1 model, Task 2 `DocPath`.
- Produces:
  - On-disk shapes (private serde structs):
    - `song.ron` = `Song { tempo, time_signature, instruments: Vec<InstrumentId>, patterns: Vec<PatternEntry { id, name }> }`
    - `instruments/<id>.ron` = `Instrument`
    - `patterns/<pattern-id>/<instrument-id>.ron` = `NoteFile { notes: Vec<Note> }`
  - `DocPath::relative_path(&self) -> PathBuf`
  - `save_project(dir: &Path, &Project) -> Result<SaveReport, SaveError>`. It writes every file atomically (temp file + rename), deletes `.ron` files under `instruments/` and `patterns/` that no longer correspond to the model (including now-empty pattern folders), and writes `.gitignore` containing `.nullwave/\n` only if it's absent. `SaveReport { written: HashMap<PathBuf, u64> }` maps each absolute path to a content hash (std `DefaultHasher` over the bytes).
  - `load_project(dir: &Path) -> Result<(Project, Vec<LoadWarning>), LoadError>`
  - `enum LoadError { NotAProject(PathBuf), Io { path: PathBuf, message: String }, Parse { path: PathBuf, line: usize, message: String }, Invalid { path: PathBuf, message: String } }`, which implements `Display` as `"<relative path>:<line>: <message>"` for parse errors
  - `enum LoadWarning { OrphanFile(PathBuf) }` for note/instrument files not referenced by `song.ron`; these are ignored
  - `pub fn hash_bytes(&[u8]) -> u64`
- RON output uses `ron::ser::PrettyConfig::new().depth_limit(4).struct_names(false)`, so saving the same project twice is byte-identical.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn round_trip_equals() { save_project(d, &p)?; assert_eq!(load_project(d)?.0, p); }
#[test] fn layout_matches_spec() { /* exists: song.ron, instruments/kick.ron, patterns/pattern-1/kick.ron, .gitignore with ".nullwave/" */ }
#[test] fn empty_note_sequences_write_no_file() { assert!(!d.join("patterns/pattern-2").exists()); }
#[test] fn second_save_is_byte_identical() {}
#[test] fn deleted_instrument_and_pattern_files_are_removed_on_save() {}
#[test] fn existing_gitignore_is_left_alone() { /* custom content preserved */ }
#[test] fn sample_path_is_stored_absolute() { /* ron text contains "/Volumes/USB/kits/kick.wav" */ }
#[test] fn not_a_project_without_song_ron() { assert!(matches!(load_project(empty), Err(LoadError::NotAProject(_)))); }
// Review Focus 2:
#[test] fn conflict_markers_name_file_and_line() { /* write "<<<<<<< HEAD" into patterns/pattern-1/kick.ron line 3 */ assert!(matches!(err, LoadError::Parse{ path, line: 3, .. } if path.ends_with("patterns/pattern-1/kick.ron"))); }
#[test] fn failed_load_leaves_files_untouched() { /* bytes of every file identical before/after */ }
#[test] fn out_of_range_values_are_invalid() { /* pitch 200, velocity 0, length 0, pan 3.0 → LoadError::Invalid naming the file */ }
#[test] fn missing_instrument_file_is_io_error_naming_it() {}
#[test] fn orphan_file_is_warning() { assert_eq!(warnings, vec![LoadWarning::OrphanFile(..)]); }
```

- [ ] **Step 2: Run** `cargo test -p nw-core --test io_tests`. Expected: FAIL.
- [ ] **Step 3: Implement `io/files.rs`.** Validation on load: pitch ≤ 127, velocity 1..=127, length > 0, pan −1..=1, volume 0..=1, tempo 20..=999.
- [ ] **Step 4: Run** `cargo test -p nw-core`. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(core): RON project folder save/load`.

---

### Task 4: Crash recovery and outside-change detection

**Files:**
- Create: `crates/nw-core/src/io/{recovery.rs,watch.rs}`
- Test: `crates/nw-core/tests/recovery_watch_tests.rs`

**Interfaces:**
- Consumes: Task 3 `SaveReport`, `hash_bytes`.
- Produces:
  - `write_recovery(dir: &Path, &Project) -> io::Result<()>` writes `.nullwave/recovery.ron` atomically. `read_recovery(dir) -> Option<Project>` returns `None` if the file is absent or unparseable. `clear_recovery(dir) -> io::Result<()>`.
  - `external_changes(paths: &[PathBuf], last_saved: &HashMap<PathBuf, u64>, read: impl Fn(&Path) -> Option<Vec<u8>>) -> Vec<PathBuf>`. A path counts as external unless its current bytes hash to `last_saved[path]`. Paths under `.nullwave/` and non-`.ron` files (other than `.gitignore`) are never external. A path that's now missing counts as external if it was in `last_saved`.
  - `ProjectWatcher::new(dir: &Path, on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> notify::Result<ProjectWatcher>`. It's recursive and debounced so that events within 200 ms are delivered as one batch, and it stops when dropped.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn recovery_round_trip_and_clear() {}
#[test] fn corrupt_recovery_reads_as_none() {}
#[test] fn own_write_is_not_external() {}
#[test] fn changed_bytes_are_external() {}
#[test] fn deleted_saved_file_is_external() {}
#[test] fn nullwave_dir_is_ignored() {}
#[test] fn watcher_reports_hand_edit() { /* write a file, wait up to 2s on a channel */ }
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement** using `notify::recommended_watcher` and a debounce thread with `std::sync::mpsc`.
- [ ] **Step 4: Run** `cargo test -p nw-core`. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(core): crash recovery and outside-change detection`.

---

### Task 5: Sample loading

**Files:**
- Create: `crates/nw-engine/src/{lib.rs,sample.rs}`, `crates/nw-engine/tests/fixtures/` (generated in the test with `hound` as a dev-dependency: mono 44.1 kHz WAV, stereo 48 kHz WAV, and a `.wav` file filled with garbage bytes)
- Test: `crates/nw-engine/tests/sample_tests.rs`

**Interfaces:**
- Produces:
  - `SampleBuffer { sample_rate: u32, left: Vec<f32>, right: Vec<f32> }` (mono is duplicated to both channels) with `fn len(&self) -> usize`
  - `load_sample(path: &Path) -> Result<SampleBuffer, SampleError>` via `symphonia` with features `wav, aiff, flac, mp3, ogg/vorbis`
  - `enum SampleError { NotFound(PathBuf), Unsupported(PathBuf), Decode { path: PathBuf, message: String } }`, with `Display` and `Clone`
  - `SampleCache::new(handle: basedrop::Handle)`, `fn get(&mut self, path: &Path) -> Result<basedrop::Shared<SampleBuffer>, SampleError>` (loads on first request and caches both successes and failures), `fn status(&self, path) -> Option<Result<(), SampleError>>`, `fn invalidate(&mut self, path)`. It isn't used on the audio thread; the app calls `get` from a background thread.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn mono_wav_duplicates_channels() { assert_eq!(b.sample_rate, 44100); assert_eq!(b.left, b.right); assert_eq!(b.len(), 4410); }
#[test] fn stereo_keeps_channels() {}
// Review Focus 1:
#[test] fn missing_file_is_not_found() { assert!(matches!(load_sample(p), Err(SampleError::NotFound(_)))); }
#[test] fn garbage_bytes_are_decode_or_unsupported_not_panic() {}
#[test] fn text_file_is_unsupported() {}
#[test] fn cache_returns_same_buffer_and_caches_failure() { assert!(Shared::ptr_eq(&a, &b)); }
```

- [ ] **Step 2: Run** `cargo test -p nw-engine`. Expected: FAIL.
- [ ] **Step 3: Implement `sample.rs`.**
- [ ] **Step 4: Run** `cargo test -p nw-engine`. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(engine): sample loading and cache`.

---

### Task 6: Sampler voice

**Files:**
- Create: `crates/nw-engine/src/voice.rs`
- Test: `crates/nw-engine/tests/voice_tests.rs`

**Interfaces:**
- Consumes: `SampleBuffer` (Task 5).
- Produces:
  - `pan_gains(pan: f32) -> (f32, f32)`: constant power, `θ = (pan + 1) · π/4`, returning `(cos θ, sin θ)`
  - `playback_rate(pitch: u8, root: u8, sample_rate: u32, output_rate: u32) -> f64` = `2^((pitch − root)/12) · sample_rate/output_rate`
  - `VoiceStart { sample: Shared<SampleBuffer>, rate: f64, gain: f32, pan: f32, attack_samples: u32, release_samples: u32, duration_samples: Option<u64>, age: u64 }`. `gain` = `velocity/127 · volume`, and `duration_samples` is `None` for previews.
  - `Voice` (a `Default` value is inactive): `fn start(&mut self, VoiceStart)`, `fn release(&mut self)`, `fn render(&mut self, out_l: &mut [f32], out_r: &mut [f32])` (adds into the outputs with linear interpolation between samples), `fn is_active(&self) -> bool`, `fn age(&self) -> u64`
  - Envelope: linear ramp 0→1 over `attack_samples`, sustain at 1, linear ramp to 0 over `release_samples` starting from the current level. Release begins when `duration_samples` runs out or `release()` is called. The voice goes inactive when the release ends or the sample runs out. With `release_samples == 0` it stops immediately.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn center_pan_is_equal_power() { let (l, r) = pan_gains(0.0); assert!((l - r).abs() < 1e-6 && (l*l + r*r - 1.0).abs() < 1e-6); }
#[test] fn hard_left() { assert_eq!(pan_gains(-1.0).1.abs() < 1e-6, true); }
#[test] fn octave_up_doubles_rate() { assert!((playback_rate(72, 60, 48000, 48000) - 2.0).abs() < 1e-9); }
#[test] fn sample_rate_mismatch_corrects_pitch() { assert!((playback_rate(60, 60, 44100, 48000) - 0.91875).abs() < 1e-9); }
#[test] fn duration_triggers_release_then_silence() { /* DC sample of 1.0, duration 100, release 50: out[0..100] ≈ gain, out[100..150] ramps down, out[150..] == 0, !is_active */ }
#[test] fn attack_ramps_from_zero() {}
#[test] fn voice_ends_at_sample_end() {}
#[test] fn render_adds_into_output() {}
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement `voice.rs`.** It must not allocate in `render`.
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(engine): sampler voice`.

---

### Task 7: Snapshot, scheduler and offline engine core

**Files:**
- Create: `crates/nw-engine/src/{snapshot.rs,core.rs}`
- Test: `crates/nw-engine/tests/core_tests.rs` (sets `#[global_allocator] static A: assert_no_alloc::AllocDisabler`)

**Interfaces:**
- Consumes: Tasks 1, 5, 6.
- Produces:
  - `InstrumentSnapshot { sample: Option<Shared<SampleBuffer>>, root_note: u8, attack_ms: f32, release_ms: f32, volume: f32, pan: f32 }`
  - `NoteEvent { tick: Tick, instrument: usize, pitch: u8, velocity: u8, length: Tick }`
  - `Snapshot { tempo: f64, loop_length: Tick, instruments: Vec<InstrumentSnapshot>, events: Vec<NoteEvent> }` with events sorted by tick
  - `compile_snapshot(&Project, pattern: Option<&PatternId>, samples: &mut SampleCache) -> Snapshot`. Missing or failed samples give `sample: None`. With `None` for the pattern, the snapshot has no events. `loop_length` = `Project::pattern_length`.
  - `pub const MAX_VOICES: usize = 64;`
  - `enum Command { SetSnapshot(basedrop::Owned<Snapshot>), Play, Stop, Preview(Shared<SampleBuffer>), StopPreview }`
  - `enum Feedback { Playhead { tick: Tick, playing: bool }, Load(f32) }`
  - `EngineCore::new(output_rate: u32, initial: Owned<Snapshot>) -> EngineCore` (preallocates the voices). Methods:
    - `fn handle(&mut self, Command)`
    - `fn process(&mut self, out: &mut [f32])` (interleaved stereo; zeroes and then fills `out`)
    - `fn playhead(&self) -> Tick`
    - `fn is_playing(&self) -> bool`
    - `fn active_voices(&self) -> usize`
  - `run_block(core: &mut EngineCore, commands: &mut rtrb::Consumer<Command>, feedback: &mut rtrb::Producer<Feedback>, out: &mut [f32])`: drains commands, calls `process` and pushes one `Playhead` (dropped if the queue is full).
- Behavior:
  - `samples_per_tick = output_rate · 60 / (tempo · PPQ)`. The transport position is an `f64` tick.
  - `Play` starts from tick 0. `Stop` releases every voice, including previews, and resets to 0.
  - Events fire at their exact sample offset inside the block: render up to the event, then start the voice.
  - At `loop_length` the position wraps to 0 within the same block, and events at tick 0 fire again.
  - Note ends come only from `duration_samples` (length × samples-per-tick at note-on), never from note-off events. Editing or swapping the snapshot can therefore never leave a hung note.
  - `SetSnapshot` keeps the tick position (wrapped modulo the new `loop_length`) and the sounding voices. The old snapshot is dropped and deferred through basedrop.
  - When no voice is free, the voice with the lowest `age` is stolen.
  - `Preview` plays the buffer at rate `sample_rate/output_rate`, gain 0.78, center pan, no duration, and a 20 ms release when `StopPreview` arrives or a new preview starts.

- [ ] **Step 1: Write the failing tests.** Use a DC sample buffer of 1.0 so that output level shows which voices are sounding. Use 48 kHz at 120 BPM, which gives exactly 25 samples per tick at PPQ 960.

```rust
#[test] fn note_starts_at_exact_sample_offset() { /* event at tick 10 → first nonzero frame == 250 even across a 256-frame block boundary */ }
#[test] fn loop_wraps_and_retriggers() { /* loop_length 3840 = 96000 frames; event at 0 sounds again at frame 96000 */ }
#[test] fn playhead_wraps() {}
#[test] fn instrument_without_sample_is_silent_not_panic() { /* Review Focus 1 */ }
#[test] fn compile_marks_missing_sample_as_none() { /* path /nonexistent.wav */ }
// Review Focus 3:
#[test] fn removing_note_mid_play_does_not_hang() { /* start note len 960; swap to snapshot without it; after 960 ticks + release, output is silent and active_voices()==0 */ }
#[test] fn stop_releases_everything() { /* after Stop + release time: silent, active_voices()==0, playhead 0 */ }
#[test] fn tempo_change_keeps_position() {}
// Review Focus 4:
#[test] fn steals_oldest_when_pool_full() { /* 80 notes at tick 0 → active_voices()==64, no panic */ }
#[test] fn process_does_not_allocate() { assert_no_alloc::assert_no_alloc(|| { core.process(&mut buf); /* with 64 voices + snapshot swap via run_block */ }); }
#[test] fn velocity_and_volume_scale_gain() { /* vel 127, vol 0.5, center pan → ≈ 0.5·cos(π/4) */ }
#[test] fn preview_plays_and_stops() {}
```

- [ ] **Step 2: Run** `cargo test -p nw-engine --test core_tests`. Expected: FAIL.
- [ ] **Step 3: Implement `snapshot.rs` and `core.rs`.**
- [ ] **Step 4: Run** `cargo test -p nw-engine` in debug mode. Expected: PASS, with no aborts from `assert_no_alloc`.
- [ ] **Step 5: Commit** with the message `feat(engine): scheduler and offline engine core`.

---

### Task 8: Engine handle and cpal output

**Files:**
- Create: `crates/nw-engine/src/handle.rs`
- Test: `crates/nw-engine/tests/handle_tests.rs`

**Interfaces:**
- Consumes: Task 7.
- Produces:
  - `EngineHandle::start() -> Result<EngineHandle, EngineError>`. It opens the default output device, requests `BufferSize::Fixed(256)` and falls back to the device default, and uses `f32` stereo; if the device has more channels, the extras are written as zero.
  - Methods:
    - `fn send(&mut self, Command) -> Result<(), Command>` (command queue capacity 256)
    - `fn poll(&mut self) -> Vec<Feedback>` (feedback queue capacity 1024)
    - `fn collect_garbage(&mut self)`, which the UI calls every frame
    - `fn collector_handle(&self) -> basedrop::Handle`
    - fields `pub sample_rate: u32, pub buffer_size: Option<u32>`
  - `EngineError { NoDevice, Config(String), Stream(String) }`
  - `Load` feedback = time spent in `run_block` divided by the buffer duration, smoothed (`0.9·old + 0.1·new`), sent at most every 100 ms.
  - In debug builds the cpal callback body is wrapped in `assert_no_alloc::assert_no_alloc`.
- The collector must outlive the stream. Field order or an explicit `Drop` has to drop the stream first.

- [ ] **Step 1: Write the test** `#[test] #[ignore = "needs audio device"] fn starts_and_reports_playhead()`. It starts the handle, sends `Play`, sleeps 300 ms and asserts that a `Playhead { playing: true, tick > 0 }` was received.
- [ ] **Step 2: Implement `handle.rs`.**
- [ ] **Step 3: Run** `cargo test -p nw-engine -- --include-ignored` on the Mac. Expected: PASS. Then run `cargo test -p nw-engine`. Expected: PASS, with the device test reported as ignored.
- [ ] **Step 4: Commit** with the message `feat(engine): cpal output handle`.

---

### Task 9: Tabs, panes and workspace persistence

**Files:**
- Create: `crates/nw-workspace/src/{lib.rs,tabs.rs}`
- Test: `crates/nw-workspace/tests/tabs_tests.rs`

**Interfaces:**
- Consumes: `nw_core` ids, `Project`.
- Produces:
  - `enum TabKind { Pattern(PatternId), PianoRoll(PatternId, InstrumentId), Instrument(InstrumentId) }`
  - `Pane { tabs: Vec<TabKind>, active: Option<usize> }`
  - `enum SidebarView { Project, Sounds, Search }`, `enum PanelTab { Editor, Log }`
  - `WorkspaceState { panes: Vec<Pane>, focused_pane: usize, sidebar_visible: bool, sidebar_view: SidebarView, sidebar_width: f32, panel_visible: bool, panel_tab: PanelTab, panel_height: f32 }`. The `Default` has one empty pane, the sidebar visible at 240.0, and the panel visible at 180.0 on `Editor`.
  - Methods on `WorkspaceState`:
    - `fn open(&mut self, TabKind)`: if the tab is open in any pane, focus that pane and tab; otherwise append to the focused pane after its active tab and activate it
    - `fn close(&mut self, pane: usize, index: usize)`: activates the neighbor to the right, else the left; an empty second pane collapses
    - `fn close_active(&mut self)`
    - `fn activate_next(&mut self)`, `fn activate_prev(&mut self)`: wrap around
    - `fn active_tab(&self) -> Option<&TabKind>`
    - `fn split_right(&mut self)`: only when there's one pane with an active tab; creates a second pane containing a copy of that tab and focuses it; does nothing if already split
    - `fn retain_valid(&mut self, &Project)`: drops tabs that reference missing patterns or instruments
    - `fn active_pattern(&self) -> Option<PatternId>`: the pattern of the active `Pattern` or `PianoRoll` tab
  - `tab_title(&TabKind, &Project) -> String`: `"pattern 1"`, `"pattern 1 / Drums"`, `"Sampler: Drums"`
  - `save_workspace(dir: &Path, &WorkspaceState) -> io::Result<()>` writes `.nullwave/workspace.ron`. `load_workspace(dir, &Project) -> WorkspaceState` falls back to the default if the file is missing or corrupt, then calls `retain_valid`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn opening_open_tab_focuses_it() { ws.open(a); ws.open(b); ws.open(a.clone()); assert_eq!(ws.panes[0].tabs.len(), 2); assert_eq!(ws.active_tab(), Some(&a)); }
#[test] fn opening_tab_open_in_other_pane_focuses_that_pane() {}
#[test] fn new_tab_inserted_after_active() {}
#[test] fn close_activates_right_then_left() {}
#[test] fn next_prev_wrap() {}
#[test] fn split_only_on_request_and_only_once() { assert_eq!(WorkspaceState::default().panes.len(), 1); /* split twice → 2 panes */ }
#[test] fn closing_last_tab_of_second_pane_collapses() {}
#[test] fn titles() {}
// Review Focus 5:
#[test] fn retain_valid_drops_deleted_instrument_and_pattern_tabs() { /* Instrument(i), PianoRoll(p,i) gone; Pattern(p2) kept; active index stays valid */ }
#[test] fn load_workspace_skips_stale_tabs_and_survives_corrupt_file() {}
#[test] fn workspace_round_trip() {}
```

- [ ] **Step 2: Run** `cargo test -p nw-workspace`. Expected: FAIL.
- [ ] **Step 3: Implement `tabs.rs`.**
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(workspace): tabs, panes, persistence`.

---

### Task 10: Dirty tracking

**Files:**
- Create: `crates/nw-workspace/src/dirty.rs`
- Test: `crates/nw-workspace/tests/dirty_tests.rs`

**Interfaces:**
- Consumes: `DocPath` (Task 2), `TabKind` (Task 9).
- Produces: `DirtyTracker` with these methods:
  - `fn mark(&mut self, impl IntoIterator<Item = DocPath>)`
  - `fn clear(&mut self)`
  - `fn is_dirty(&self) -> bool`
  - `fn tab_is_dirty(&self, &TabKind) -> bool`: `Pattern(p)` is dirty if any `Notes(p, _)` is dirty; `PianoRoll(p, i)` checks `Notes(p, i)`; `Instrument(i)` checks `Instrument(i)`

- [ ] **Step 1: Write the failing tests.** Cover `pattern_tab_dirty_when_any_of_its_notes_dirty`, `piano_roll_only_its_own_notes`, `song_edit_dirties_project_not_tabs` and `clear_resets`.
- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement** with a `BTreeSet<DocPath>`. Derive `Ord` on `DocPath` in `nw-core` if it isn't derived already.
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(workspace): dirty tracking`.

---

### Task 11: Commands, keymap and palette ranking

**Files:**
- Create: `crates/nw-workspace/src/{commands.rs,palette.rs}`
- Test: `crates/nw-workspace/tests/commands_tests.rs`

**Interfaces:**
- Produces:
  - `CommandInfo { id: &'static str, title: &'static str }` and `pub const COMMANDS: &[CommandInfo]` with exactly these ids, each with a title:

    | id | default keys | context |
    |---|---|---|
    | `workspace.command_palette` | `cmd-shift-p` | — |
    | `workspace.quick_open` | `cmd-p` | — |
    | `workspace.toggle_sidebar` | `cmd-b` | — |
    | `workspace.toggle_panel` | `cmd-j` | — |
    | `workspace.close_tab` | `cmd-w` | — |
    | `workspace.next_tab` | `ctrl-tab` | — |
    | `workspace.prev_tab` | `ctrl-shift-tab` | — |
    | `workspace.split_right` | `cmd-\` | — |
    | `workspace.show_project` / `show_sounds` / `show_search` | `cmd-shift-e` / `cmd-shift-s` / `cmd-shift-f` | — |
    | `project.new` | — | — |
    | `project.open` | `cmd-o` | — |
    | `project.save` | `cmd-s` | — |
    | `project.new_pattern` | `cmd-n` | — |
    | `project.add_sample_folder` | — | — |
    | `pattern.rename` / `pattern.delete` / `instrument.delete` | — | — |
    | `edit.undo` / `edit.redo` | `cmd-z` / `cmd-shift-z` | — |
    | `transport.play_stop` | `space` | — |
    | `piano_roll.delete` | `backspace` | `PianoRoll` |
    | `piano_roll.select_all` | `cmd-a` | `PianoRoll` |
    | `piano_roll.duplicate` | `cmd-d` | `PianoRoll` |
    | `piano_roll.nudge_left/right/up/down` | `left/right/up/down` | `PianoRoll` |
    | `piano_roll.octave_up/down` | `shift-up/shift-down` | `PianoRoll` |
    | `app.quit` | `cmd-q` | — |

  - `Binding { keystrokes: String, command: String, context: Option<String> }` (serde)
  - `default_bindings() -> Vec<Binding>`
  - `merge_user_keymap(defaults: Vec<Binding>, user_ron: &str) -> Result<Vec<Binding>, KeymapError>`. The user file is a RON `Vec<Binding>`. A user entry replaces any default with the same `(keystrokes, context)`. `command: ""` unbinds. An unknown command id gives `KeymapError::UnknownCommand(String)`, and bad RON gives `KeymapError::Parse(String)`.
  - `PaletteItem { Command(&'static str), Pattern(PatternId), Instrument(InstrumentId), Sample(PathBuf) }`, `PaletteEntry { item: PaletteItem, label: String }`
  - `rank(query: &str, entries: &[PaletteEntry]) -> Vec<usize>`: indices of matching entries, best first, using `nucleo_matcher` (smart case, fuzzy). An empty query returns all entries in their original order.

- [ ] **Step 1: Write the failing tests:** `every_default_binding_names_a_known_command`, `no_duplicate_keystroke_in_same_context`, `user_override_replaces_default`, `empty_command_unbinds`, `unknown_command_errors`, `bad_ron_errors`, `rank_fuzzy_prefers_tighter_match` (`"tsb"` ranks "Toggle Sidebar" above "Toggle Panel" and returns no unrelated entries) and `empty_query_keeps_order`.
- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement** `commands.rs` and `palette.rs`.
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(workspace): commands, keymap, palette ranking`.

---

### Task 12: App settings and sample browser

**Files:**
- Create: `crates/nw-workspace/src/{settings.rs,browser.rs}`
- Test: `crates/nw-workspace/tests/settings_browser_tests.rs`

**Interfaces:**
- Produces:
  - `AppSettings { sample_folders: Vec<PathBuf>, last_project: Option<PathBuf> }` (serde, `Default`)
  - `settings_dir() -> PathBuf`: `dirs::config_dir()/Nullwave`, which is `~/Library/Application Support/Nullwave`. It holds `settings.ron` and the user `keymap.ron`.
  - `load_settings(path: &Path) -> AppSettings`: falls back to the default if the file is missing or corrupt. `save_settings(path, &AppSettings) -> io::Result<()>` writes atomically.
  - `pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "aif", "aiff", "flac", "mp3", "ogg"];` (case-insensitive)
  - `BrowserEntry { path: PathBuf, name: String, is_dir: bool }`
  - `list_dir(path: &Path) -> io::Result<Vec<BrowserEntry>>`: directories first, then audio files, each group sorted case-insensitively by name. Hidden entries (leading `.`) and non-audio files are skipped. A folder that isn't mounted returns `Err`, which the sidebar shows as "not connected".

- [ ] **Step 1: Write the failing tests:** `settings_round_trip`, `corrupt_settings_default`, `lists_dirs_then_audio_sorted_case_insensitive`, `skips_hidden_and_non_audio`, `uppercase_extension_counts` (`KICK.WAV`) and `missing_folder_is_err`.
- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement** `settings.rs` and `browser.rs`.
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(workspace): settings and sample browser`.

---

### Task 13: Piano roll geometry and note operations

**Files:**
- Create: `crates/nw-workspace/src/piano_roll.rs`
- Test: `crates/nw-workspace/tests/piano_roll_tests.rs`

**Interfaces:**
- Consumes: `Note`, `Tick`, `PPQ`.
- Produces:
  - `pub const DEFAULT_SNAP: Tick = 240;` (1/16 note)
  - `pub const EDGE_PX: f32 = 6.0;`
  - `PianoRollView { px_per_tick: f32, row_height: f32, top_pitch: u8, scroll_ticks: f32 }` with defaults `0.1, 14.0, 84, 0.0`. Methods: `fn tick_at(&self, x: f32) -> f32`, `fn pitch_at(&self, y: f32) -> Option<u8>`, `fn note_rect(&self, &Note) -> (f32, f32, f32, f32)` (x, y, w, h).
  - `snap_floor(tick: f32, snap: Tick) -> Tick`
  - `enum Hit { Body(usize), RightEdge(usize), Empty { tick: Tick, pitch: u8 } }` and `hit_test(notes: &[Note], &PianoRollView, x: f32, y: f32, snap: Tick) -> Hit`. The right edge is the last `min(EDGE_PX, w/3)` pixels. When notes overlap, the topmost (last-drawn, highest index) wins. The `Empty` tick is snapped.
  - `place_note(tick: Tick, pitch: u8, last_length: Tick) -> Note` (velocity `DEFAULT_VELOCITY`)
  - `moved(notes: &[Note], selected: &[usize], d_ticks: i64, d_pitch: i32) -> Vec<Note>`: returns the full new vector, with the *group* clamped so that no selected note goes below tick 0 or outside pitch 0..=127, and relative positions preserved
  - `resized(notes: &[Note], selected: &[usize], d_ticks: i64, snap: Tick) -> Vec<Note>`: the minimum length is `snap`
  - `duplicated(notes: &[Note], selected: &[usize]) -> (Vec<Note>, Vec<usize>)`: the copies are placed right after the selection's span, and the indices of the copies are returned
  - `with_velocity(notes: &[Note], selected: &[usize], velocity: u8) -> Vec<Note>`: clamped to 1..=127

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn hit_body_edge_empty() {}
#[test] fn empty_click_snaps_down() { /* x at tick 300 → Empty{tick:240,..} */ }
#[test] fn topmost_note_wins() {}
#[test] fn group_move_clamps_at_pitch_127_preserving_shape() { /* notes at 120 and 126, move +5 → 121 and 127 */ }
#[test] fn group_move_clamps_at_tick_zero() {}
#[test] fn resize_min_is_snap() {}
#[test] fn duplicate_places_after_span() { /* sel spans 0..960 → copies start at 960 */ }
#[test] fn velocity_clamped() { assert_eq!(with_velocity(&n, &[0], 0)[0].velocity, 1); }
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement `piano_roll.rs`.**
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(workspace): piano roll geometry and note ops`.

---

### Task 14: App shell (window, layout, state glue, keybindings, status bar)

GPUI 0.2.2 has sparse docs. Read `docs.rs/gpui/0.2.2`, the crate's `examples/` folder, and Zed's `workspace` / `ui` crates at the commit matching 0.2.2 for patterns: actions, `KeyBinding` with key contexts, `Render`, `Entity`, `FocusHandle`, background executor and `cx.spawn` timers. Keep GPUI types out of the library crates.

**Files:**
- Create: `crates/nullwave/src/{main.rs,app_state.rs,theme.rs}`, `crates/nullwave/src/ui/{mod.rs,components.rs,shell.rs,sidebar.rs,editor_area.rs,status_bar.rs,bottom_panel.rs}`

**Interfaces:**
- Consumes: everything from Tasks 1–13.
- Produces: `AppState`, a GPUI entity, with the fields `project_dir: Option<PathBuf>`, `project: Project`, `history: History`, `dirty: DirtyTracker`, `workspace: WorkspaceState`, `settings: AppSettings`, `engine: Option<EngineHandle>`, `samples: SampleCache`, `playhead: Tick`, `playing: bool`, `cpu_load: f32`, `log: Vec<String>` and `last_saved: HashMap<PathBuf, u64>`. It has these methods:
  - `fn apply(&mut self, edits: Vec<Edit>, cx)`: commits to history, marks dirty, recompiles and sends the snapshot, notifies
  - `fn undo(&mut self, cx)`, `fn redo(&mut self, cx)`
  - `fn open_tab(&mut self, TabKind, cx)`
  - `fn play_stop(&mut self, cx)`: compiles the snapshot for `workspace.active_pattern()`, or for the last played pattern if the focused tab isn't a pattern, then sends `Play`/`Stop`
  - `fn sample_status(&self, &InstrumentId) -> SampleStatus { NoSample, Ok, Missing(String) }`
  - `fn resync_engine(&mut self)`
- Behavior:
  - On launch, open `settings.last_project` if it still loads, restore `workspace.ron`, and start the engine.
  - If the engine fails, keep running with `engine: None`. The status bar then shows "No audio output" and the error goes to the Log.
  - A ~16 ms timer polls engine feedback, calls `collect_garbage` and updates the playhead/CPU.
  - Register every command from `COMMANDS` as a GPUI action and build keybindings from `merge_user_keymap(default_bindings(), keymap.ron)`. A keymap error is logged and the defaults are used.
  - In debug builds, set `#[global_allocator] static A: assert_no_alloc::AllocDisabler`.
- Layout, matching spec "layout A":
  - An activity bar 36 px wide with Project / Sounds / Search icons.
  - A resizable sidebar. The Project view has three sections: **project** (patterns list; a "+ new pattern" row), **presets** (a dimmed "available when the mixer arrives" line) and **sample folders** (each configured folder as a lazily expanded tree via `list_dir`, a "not connected" line on `Err`, and a "+ add folder" row that runs `project.add_sample_folder` with a native folder picker). Sounds shows only sample folders; Search filters patterns, instruments and sample names with `rank`.
  - An editor area: a tab bar per pane, where a dirty tab shows a dot instead of its close button, and the active editor below (placeholder views until Tasks 15–16).
  - A resizable, hideable bottom panel with the tabs *Editor* (empty placeholder) and *Log*.
  - A status bar. Left: ▶/■ button, playhead as `bar.beat.sixteenth` (1-based), active pattern name, tempo (click to edit the number, applied with `Edit::SetTempo`) and time signature. Right: sample rate in kHz, buffer size and CPU %.
  - Sidebar and panel sizes and visibility live in `WorkspaceState`, which is saved on every change (debounced 500 ms) and on quit.
- With no project open, the editor area shows two buttons, "New Project…" and "Open Project…" (wired in Task 17; stubbed to log for now).

- [ ] **Step 1: Implement the files above.** Keep each UI file focused on one region.
- [ ] **Step 2: Build:** run `cargo build -p nullwave`. Expected: success, with no warnings in `crates/nullwave`.
- [ ] **Step 3: Run and check manually:** create a test project with a tiny script or test fixture (one instrument with a real WAV, one pattern with 4 notes), set it as `last_project` in `settings.ron`, then run `cargo run -p nullwave`. Expected:
  - The layout matches spec layout A.
  - ⌘B toggles the sidebar and ⌘J toggles the panel.
  - Space plays and the playhead advances and loops.
  - The status bar shows the rate, buffer and CPU.
  - Quitting and relaunching restores the panel and sidebar sizes.
  - Renaming the WAV makes playback silent without a crash, and the Log shows the error.
- [ ] **Step 4: Commit** with the message `feat(app): workspace shell, state glue, status bar`.

---

### Task 15: Pattern tab (channel rack) and instrument tab

**Files:**
- Create: `crates/nullwave/src/ui/{pattern_tab.rs,instrument_tab.rs}`
- Modify: `crates/nullwave/src/ui/editor_area.rs`, `crates/nullwave/src/ui/sidebar.rs`

**Interfaces:**
- Consumes: `AppState` (Task 14), `naming`, `Edit`, `remove_instrument_edit`.
- Behavior:
  - **Pattern tab:**
    - Clicking a pattern in the sidebar opens `TabKind::Pattern`.
    - The tab has one row per instrument in project order. A row shows the name button and a mini preview, a 28 px strip that draws each note as a 3 px bar scaled to `pattern_length` across the pitch range used.
    - An empty row gets a dashed border and an "empty — double-click to add notes" hint.
    - Double-clicking a row opens `TabKind::PianoRoll(p, i)`.
    - Clicking the name opens `TabKind::Instrument(i)`.
    - A row whose sample status is `Missing` shows a "missing sample" marker with the error in a tooltip.
    - The last row is "+ add instrument", which opens a native file picker filtered to `AUDIO_EXTENSIONS`.
  - **Drag and drop:** dragging a sample from the sidebar onto "+ add instrument" creates a sampler named after the file stem (via `new_instrument`), as an `AddInstrument` edit. Dropping onto an existing row replaces that instrument's sample (`UpdateInstrument`).
  - **Preview:** clicking a sample file in the sidebar previews it (`Command::Preview` after loading on the background executor). Clicking another sample replaces the preview.
  - **Instrument tab** (`Sampler: <name>`):
    - Rows: sample path (with "Change…", and a "missing" state), root note (note name and number, a stepper), attack ms (0–2000), release ms (0–5000), volume (0–100%) and pan (L100–C–R100).
    - Each control commits one `UpdateInstrument` when the drag or edit ends.
    - A "Delete instrument" button, the same as `instrument.delete`, applies `remove_instrument_edit` and calls `workspace.retain_valid`.
  - **Patterns:**
    - `project.new_pattern` applies an `AddPattern` from `next_pattern`, opens its pattern tab and starts an inline rename in the sidebar.
    - `pattern.rename` renames inline in the sidebar (`RenamePattern`).
    - `pattern.delete` applies `RemovePattern` and calls `retain_valid`.
    - Every one of these is undoable.

- [ ] **Step 1: Implement.**
- [ ] **Step 2: Build:** run `cargo build -p nullwave`. Expected: success.
- [ ] **Step 3: Run and check manually.** Expected:
  - ⌘N creates "pattern 2" and opens its rack.
  - Dragging a WAV from a sample folder adds an instrument row.
  - Double-clicking it opens `pattern 2 / <name>`.
  - Double-clicking it again focuses the same tab instead of opening a second one.
  - Deleting an instrument closes its tabs, and ⌘Z brings back both the instrument and its notes.
  - Unplugging (or renaming) the sample folder shows "missing sample" on the row after reopening.
- [ ] **Step 4: Commit** with the message `feat(app): channel rack pattern tab and instrument tab`.

---

### Task 16: Piano roll and velocity lane

**Files:**
- Create: `crates/nullwave/src/ui/piano_roll.rs`
- Modify: `crates/nullwave/src/ui/bottom_panel.rs`, `crates/nullwave/src/ui/editor_area.rs`

**Interfaces:**
- Consumes: `nw_workspace::piano_roll` (Task 13) and `AppState::apply`.
- Behavior:
  - **Drawing:**
    - A keyboard strip 40 px wide on the left, with C notes labelled (`C4` = 60) and black keys darker.
    - Grid lines at the snap size, stronger at beats and strongest at bars.
    - The region past `pattern_length` is shaded.
    - The playhead line is drawn while this pattern plays.
    - Vertical scroll covers pitch, and the trackpad scrolls horizontally. ⌘+scroll zooms time.
  - **Mouse:**
    - Clicking empty space places a note with `place_note` (the last used length, initially `DEFAULT_SNAP · 4`) and previews its pitch through the instrument.
    - Dragging a body moves the selection with `moved` and dragging the right edge resizes with `resized`. Both show a live preview, held locally, and commit **one** `ReplaceNotes` on mouse-up.
    - Clicking selects, shift-click toggles, and dragging on empty space with ⌥ held draws a rubber band.
    - Right-click deletes.
  - **Keyboard** (context `PianoRoll`): delete, select all, duplicate, nudge by snap or semitone, and octave ±12. Each action is one undo step.
  - **Velocity lane:** when a piano roll is the active tab, the bottom panel's *Editor* tab shows a velocity lane with a bar per note aligned to the roll's horizontal scroll. Dragging a bar (or the bars of the selection) sets velocity with `with_velocity` and commits one edit on release.
  - **Hover:** a note shows pitch, start (`bar.beat.sixteenth`), length and velocity as a tooltip, since there's no inspector panel.
  - **Playback:** edits while playing are audible on the next loop pass, through the snapshot resync.

- [ ] **Step 1: Implement.**
- [ ] **Step 2: Build:** run `cargo build -p nullwave`. Expected: success.
- [ ] **Step 3: Run and check manually.** Expected:
  - Notes can be placed, moved, resized, deleted, duplicated and nudged.
  - ⌘Z undoes a whole drag in one step.
  - Velocity changes are audible.
  - Deleting a sounding note while playing leaves no stuck sound (Review Focus 3, in the real app).
  - Moving a note group past the top of the keyboard keeps its shape.
- [ ] **Step 4: Commit** with the message `feat(app): piano roll and velocity lane`.

---

### Task 17: Palette, prompts, save/open/new, outside changes, split

**Files:**
- Create: `crates/nullwave/src/ui/{palette.rs,modal.rs}`
- Modify: `crates/nullwave/src/app_state.rs`, `crates/nullwave/src/main.rs`, `crates/nullwave/src/ui/editor_area.rs`

**Interfaces:**
- Consumes: `rank`, `COMMANDS`, `save_project`, `load_project`, `write_recovery`, `read_recovery`, `clear_recovery`, `external_changes`, `ProjectWatcher`, `WorkspaceState::split_right`.
- Behavior:
  - **Palette:**
    - An overlay anchored top-center, 600 px wide. ⌘⇧P opens it in command mode (prefix `>` shown in the input; each command shows its title and keybinding). ⌘P opens it in quick-open mode over patterns, instruments and samples in the sample folders. In quick open, typing `>` switches to command mode.
    - ↑/↓ moves through the list, Enter runs the selected entry, and Esc closes the palette.
    - Running a pattern or instrument opens its tab. Running a sample previews it.
  - **Save:** `project.save` calls `save_project`, then sets `last_saved` from the report, clears the dirty set and calls `clear_recovery`. A save error shows in an in-window error prompt and leaves the project dirty.
  - **Recovery:** while the project is dirty, call `write_recovery` every 30 s.
  - **New project:** a native save panel picks a folder name. The app creates the folder with one instrument-less "pattern 1", saves it, opens it, and updates `settings.last_project`.
  - **Open project:** a native folder picker. `LoadError` shows in an in-window prompt with the file and line, and the current project stays open.
  - **Restoring recovery:** if `read_recovery` returns a project on open, a prompt reads "Restore unsaved changes from last session?" with *Restore* (loads it as the current project, everything marked dirty) and *Discard* (clears it).
  - **Quitting:** `app.quit` and window close while dirty show a prompt reading "Save changes before quitting?" with *Save* / *Don't Save* / *Cancel*. On a clean quit, call `clear_recovery`.
  - **Outside changes:** `ProjectWatcher` batches are filtered with `external_changes`. With no unsaved changes, the project reloads silently, then calls `retain_valid` and resyncs the engine. With unsaved changes, a prompt reads "Files changed on disk: <list>." with *Reload from disk* / *Keep mine* (Keep mine marks the whole project dirty so the next save overwrites the disk). A reload that fails to parse shows the `LoadError` and keeps the in-memory project.
  - **Split:** `workspace.split_right` renders two panes side by side with a draggable divider. Clicking a pane focuses it.
  - **Prompts:** all prompts are in-window modal overlays (`modal.rs`), never native alerts or separate windows. Enter triggers the primary button and Esc cancels.

- [ ] **Step 1: Implement.**
- [ ] **Step 2: Build:** run `cargo build -p nullwave`. Expected: success.
- [ ] **Step 3: Run and check manually.** Expected:
  - ⌘⇧P lists the commands with their shortcuts, and fuzzy typing works. ⌘P finds patterns and samples.
  - ⌘S clears the tab dots, and `git diff` after a one-note edit touches exactly one file.
  - Running `git checkout -- patterns/` in a terminal with no unsaved changes reloads the app. Doing it with unsaved changes shows the prompt.
  - Killing the app with `kill -9` after an edit and relaunching offers to restore.
  - Quitting with unsaved changes shows the in-window prompt.
  - Opening a project that has a conflict marker shows the file and line, and the open project stays.
  - ⌘\ splits, and the split happens only when asked.
- [ ] **Step 4: Commit** with the message `feat(app): palette, prompts, save/open/new, outside changes, split`.

---

### Task 17a: Sampler model and file format

The spec's "Sampler" section (revised 2026-11-04) defines every parameter, range and default used in Tasks 17a–17e. Read it first. Defaults must sound exactly like the sampler from Tasks 6–8.

**Files:**
- Modify: `crates/nw-core/src/model.rs`, `crates/nw-core/src/io/files.rs`, `crates/nullwave/src/ui/instrument_tab.rs` (only enough to compile against the new fields)
- Test: `crates/nw-core/tests/sampler_model_tests.rs`

**Interfaces:**
- Produces, in `nw-core::model` (all `Clone, Debug, PartialEq, Serialize, Deserialize`, every struct `#[serde(default)]`):
  - `Sampler { sample_path, root_note, region: Region, looping: Loop, one_shot: bool, processing: Processing, tune: Tune, volume_env: Envelope, filter: Filter, cutoff_env: ModEnvelope, pitch_env: ModEnvelope, pan_env: ModEnvelope, volume_lfo: Lfo, pan_lfo: Lfo, cutoff_lfo: Lfo, pitch_lfo: Lfo, velocity: VelocityMap, voice: VoiceSettings }`
  - `Region { start: f32, end: f32 }` (fractions, default `0.0, 1.0`)
  - `Loop { mode: LoopMode, start: f32, end: f32, crossfade_ms: f32 }`, `enum LoopMode { Off, Forward, PingPong }`
  - `Processing { reverse, normalize, invert_polarity, swap_stereo, declick: bool }` (declick defaults to `true`)
  - `Tune { coarse: i8, fine_cents: f32 }`
  - `Envelope { delay_ms, attack_ms, hold_ms, decay_ms, sustain, release_ms, attack_curve, decay_curve: f32 }`; volume default `0, 0, 0, 0, 1.0, 20.0, 0, 0`
  - `Filter { kind: FilterKind, cutoff_hz: f32, resonance: f32 }`, `enum FilterKind { Off, LowPass, HighPass, BandPass, Notch }`
  - `ModEnvelope { env: Envelope, amount: f32 }` (amount in octaves, semitones or pan units by target; default 0)
  - `Lfo { shape: LfoShape, rate: LfoRate, amount: f32, delay_ms: f32 }`, `enum LfoShape { Sine, Triangle, Saw, Square, Random }`, `enum LfoRate { Hz(f32), Sync { numerator: u16, denominator: u16 } }`
  - `VelocityMap { to_volume: f32, to_cutoff_octaves: f32 }` (default `1.0, 0.0`)
  - `VoiceSettings { polyphony: u8, mono: bool, glide_ms: f32, cut: u8, cut_by: u8 }` (default `64, false, 0, 0, 0`)
- Behavior:
  - **Legacy files:** an instrument file with top-level `attack_ms` / `release_ms` (the old shape) loads with those values in `volume_env`. New files are written in the new shape only.
  - **Validation on load:** every range in the spec, plus `region.start < region.end` and loop points inside the region. A violation is `LoadError::Invalid` naming the file, like the existing checks.

- [ ] **Step 1: Write the failing tests:** `default_sampler_matches_old_behaviour` (sustain 1, release 20, filter off, all amounts 0), `old_instrument_file_loads_attack_release_into_volume_env`, `new_shape_round_trips`, `missing_fields_take_defaults`, `region_start_after_end_is_invalid_naming_file`, `loop_outside_region_is_invalid`, `out_of_range_values_are_invalid` (one case per range).
- [ ] **Step 2: Run** `cargo test -p nw-core`. Expected: FAIL.
- [ ] **Step 3: Implement.** Update the instrument tab's slider fields to the new paths so the app still builds.
- [ ] **Step 4: Run** `cargo test --workspace` and `cargo build -p nullwave`. Expected: PASS, no warnings.
- [ ] **Step 5: Commit** with the message `feat(core): FL-style sampler model and file format`.

---

### Task 17b: Playback core (spec phase A)

**Files:**
- Create: `crates/nw-engine/src/process.rs`
- Modify: `crates/nw-engine/src/{voice.rs,snapshot.rs,core.rs,lib.rs}`
- Test: `crates/nw-engine/tests/process_tests.rs`, `crates/nw-engine/tests/playback_tests.rs`

**Interfaces:**
- Produces:
  - `process_sample(source: &SampleBuffer, settings: &ProcessSettings, output_rate: u32) -> ProcessedSample`, which runs off the audio thread. `ProcessSettings` holds `Processing`, `Region` and `Loop`. `ProcessedSample { buffer: SampleBuffer, start: usize, end: usize, loop_start: usize, loop_end: usize }` are frame indices into the processed buffer. Reverse, polarity, stereo swap, normalize (peak to 1.0 over the region), declick (2 ms fades at the region edges and, for loops, at the loop end) and the forward-loop crossfade are baked into `buffer`.
  - `ProcessedCache`, keyed by `(path, ProcessSettings)`, built on `SampleCache`. `compile_snapshot` marks the entries it uses and drops the rest, so memory follows the project.
  - `VoiceParams`: a `Copy` struct with everything a voice needs from the instrument and note. `VoiceStart` carries `Shared<ProcessedSample>` plus `VoiceParams`.
- Behavior:
  - Playback starts at `start` and stops at `end`.
  - **Loop modes:** forward jumps from `loop_end` to `loop_start`; ping-pong reverses direction at each loop edge.
  - **Held vs. one-shot:** held releases at the note's end. One-shot ignores the note length, except with a loop on, where the note's end always releases.
  - **Tune:** `playback_rate` takes `coarse` and `fine_cents`.
  - Everything in `Voice::render` stays allocation-free, and `process_does_not_allocate` from Task 7 must still pass.

- [ ] **Step 1: Write the failing tests.** Use a ramp sample (frame *i* has value *i*/N) so the output shows the read position:
  - `region_plays_only_between_start_and_end`
  - `reverse_plays_backwards`
  - `normalize_scales_peak_to_one`
  - `invert_and_swap_stereo`
  - `declick_fades_edges`
  - `forward_loop_repeats_until_note_end`
  - `ping_pong_reverses_at_edges`
  - `crossfade_removes_loop_discontinuity` (largest sample-to-sample jump at the wrap stays below a threshold)
  - `one_shot_ignores_note_length`
  - `one_shot_with_loop_still_ends_at_note_end`
  - `coarse_and_fine_tune_change_rate`
  - `cache_reuses_and_drops_unused`
- [ ] **Step 2: Run** `cargo test -p nw-engine`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p nw-engine` (debug, so `assert_no_alloc` is active). Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(engine): sampler region, loop, one-shot, processing, tuning`.

---

### Task 17c: Envelopes, filter and modulation (spec phase B)

**Files:**
- Create: `crates/nw-engine/src/{env.rs,filter.rs,lfo.rs}`
- Modify: `crates/nw-engine/src/{voice.rs,snapshot.rs,core.rs}`
- Test: `crates/nw-engine/tests/{env_tests.rs,filter_tests.rs,lfo_tests.rs,modulation_tests.rs}`

**Interfaces:**
- Produces:
  - `EnvState` (DAHDSR, stepping per sample, with curves): `start(&EnvParams)`, `release()`, `next() -> f32`, `is_done()`. `EnvParams` holds the times in samples.
  - `Svf`: a TPT state-variable filter. `set(kind, cutoff_hz, resonance, sample_rate)` and `process(l, r) -> (l, r)`.
  - `LfoState`: `start(&LfoParams, seed)` and `next() -> f32` in −1..=1. Random (sample-and-hold) uses a per-voice xorshift with no allocation. Tempo-synced rates are converted to Hz at note start from the snapshot tempo.
- Behavior:
  - **Volume:** the volume envelope replaces the linear attack/release.
  - **Control rate:** cutoff, pitch and pan modulation (envelope + LFO + velocity → cutoff) are computed every 16 frames, and filter coefficients update then too. Volume is per sample.
  - **Cutoff:** `base_hz × 2^(octaves)`, clamped to 20 Hz–min(20 kHz, 0.45 × output rate).
  - **Volume LFO:** scales gain by `1 − amount × (1 − lfo)/2`, so the amount is the depth of a dip below full volume.
  - **Velocity → volume:** `gain = volume × (1 − to_volume + to_volume × velocity/127)`.

- [ ] **Step 1: Write the failing tests:**
  - `dahdsr_stage_lengths` (exact sample counts for each stage)
  - `sustain_holds_until_release`
  - `release_from_mid_attack_starts_at_current_level`
  - `curve_zero_is_linear` and `curves_bend_monotonically`
  - `lowpass_attenuates_above_cutoff` (measure the RMS of a sine above and below cutoff)
  - `highpass_attenuates_below_cutoff`
  - `bandpass_and_notch_shapes`
  - `resonance_peaks_at_cutoff`
  - `filter_off_is_bit_exact_passthrough`
  - `lfo_shapes_and_rate` (zero crossings per second)
  - `synced_lfo_follows_tempo`
  - `random_lfo_holds_between_steps`
  - `lfo_delay`
  - `pitch_env_bends_rate`
  - `pan_lfo_moves_balance`
  - `velocity_to_cutoff_darkens_soft_notes`
  - `defaults_render_identically_to_task_7` (compare against the Task 7 expected outputs)
  - `process_does_not_allocate_with_all_modulation_on`
- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p nw-engine`. Expected: PASS. Then add an `#[ignore]` benchmark, `full_load_block_time`: 64 voices with the filter and all four LFOs on, rendering 512-frame blocks. Run it in release with `cargo test -p nw-engine --release -- --ignored full_load_block_time`. Expected: under 25 % of the block's real-time duration at 48 kHz.
- [ ] **Step 5: Commit** with the message `feat(engine): sampler envelopes, filter, LFOs, velocity mapping`.

---

### Task 17d: Voice behaviour (spec phase C)

**Files:**
- Modify: `crates/nw-engine/src/{core.rs,voice.rs,snapshot.rs}`
- Test: `crates/nw-engine/tests/voice_mgmt_tests.rs`

**Interfaces:**
- Produces: `Voice::fade_out(samples)`, a fast linear fade from the current level that ignores the release time. `Voice::glide_to(rate, glide_samples)` for legato. Voices remember their instrument index.
- Behavior:
  - **Fades:** every stolen, cut or over-limit voice fades over 5 ms (`FADE_MS`).
  - **Fade reserve:** the pool gains 16 reserve slots (`FADE_RESERVE`). When all `MAX_VOICES` are busy, the oldest voice is swapped (`voices.swap`, no allocation) into a free reserve slot to fade, and its slot takes the new note. With no free reserve slot it is cut hard, which is the old behaviour.
  - **Polyphony:** starting a note over the instrument's limit fades that instrument's oldest voice.
  - **Mono:** at most one non-fading voice per instrument. With `glide_ms > 0` and a voice sounding, the new note glides that voice's pitch (exponential in pitch, over `glide_ms`), keeps its envelope, and takes the new note's length. With `glide_ms == 0` the old voice fades and a new one starts.
  - **Cut groups:** a note on an instrument with `cut = G` (G > 0) fades every voice of instruments whose `cut_by == G`, including voices of the same instrument.
  - **Previews:** `PreviewNote` follows the same rules, so a preview chokes like a real note.

- [ ] **Step 1: Write the failing tests:**
  - `steal_fades_instead_of_cutting` (no sample-to-sample jump above a threshold)
  - `full_pool_and_full_reserve_still_never_panics`
  - `polyphony_limit_fades_oldest_of_that_instrument_only`
  - `mono_without_glide_retriggers`
  - `mono_glide_bends_pitch_and_keeps_envelope`
  - `open_hat_cut_by_closed_hat`
  - `cut_group_does_not_touch_other_groups`
  - `stop_still_releases_everything` (Review Focus 3)
  - `process_does_not_allocate_under_cuts_and_steals`
- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p nw-engine`. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(engine): polyphony limit, mono glide, cut groups, fade on steal`.

---

### Task 17e: Instrument tab (spec phase D)

**Files:**
- Create: `crates/nullwave/src/ui/{param.rs,waveform.rs,envelope_graph.rs}`
- Modify: `crates/nullwave/src/ui/instrument_tab.rs`, `crates/nullwave/src/app_state.rs`, `crates/nw-engine/src/sample.rs`
- Test: unit tests in `param.rs` and `waveform.rs`; `crates/nw-engine/tests/sample_tests.rs` for peaks

**Interfaces:**
- Consumes: the model from Task 17a and `AppState::preview_note`.
- Produces:
  - `nw_engine::sample::peaks(&SampleBuffer, buckets) -> Vec<(f32, f32)>` (min/max per bucket). The app computes peaks on the background executor and caches them per sample path.
  - `ParamSpec { label, get: fn(&Instrument) -> f32, set: fn(&mut Instrument, f32), min, max, scale: Linear | Log | Squared, format: fn(f32) -> String }`. One table describes every slider, and the Task 15 `Field` enum and slider code become generic over `ParamSpec`. A drag still shows its value live and commits one `UpdateInstrument` on release.
- Behavior:
  - **Sections:** Sample, Pitch, Envelopes, Filter, LFO, Velocity, Voice, each with a small uppercase header. Volume and pan stay at the top. The tab scrolls.
  - **Sample section:** the waveform, with start/end markers (dim outside the region) and loop markers when the loop is on. Each marker drags and commits on release.
    - Toggles: reverse, normalize, invert polarity, swap stereo, declick, one-shot.
    - Loop mode as a three-way segmented control, plus a crossfade slider.
    - The waveform shows the processed result, so reversing flips it.
  - **Envelopes section:** a segmented control picks volume / cutoff / pitch / pan. The graph shows the DAHDSR shape with draggable points (attack peak, decay/sustain, release end), and sliders sit beside it. Modulation envelopes also have an amount slider.
  - **LFO section:** a segmented control picks volume / pan / cutoff / pitch. Shape is a segmented control, rate has a "sync" toggle (free Hz or a note value), then amount and delay.
  - **Voice section:** polyphony stepper, mono toggle, glide slider (dimmed unless mono), and cut / cut-by steppers (0 shown as "none").
  - **Audition:** a ▶ button next to the tab title plays the root note through `preview_note`.
  - **Missing sample:** the waveform area shows the "missing sample" state from Task 15.

- [ ] **Step 1: Write the failing tests:** `peaks_min_max_per_bucket`, `peaks_of_empty_buffer`, `param_specs_round_trip_through_fraction` (for every spec, `value_at(fraction_of(v)) ≈ v` across its range), `param_specs_stay_in_range`, `marker_drag_keeps_start_before_end`.
- [ ] **Step 2: Run** the tests. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Build** with `cargo build -p nullwave` (no warnings), then run and check manually. Expected:
  - Every section is present.
  - Trimming the start in the waveform audibly skips the attack.
  - A looped pad sustains under a long note, and ping-pong is audible.
  - The filter envelope gives a pluck.
  - A tempo-synced LFO on cutoff wobbles in time.
  - Closed hat chokes open hat.
  - Mono glide slides between overlapping notes.
  - Every change is one ⌘Z step.
  - An old project from before Task 17a opens and sounds the same.
- [ ] **Step 5: Commit** with the message `feat(app): full sampler instrument tab`.

---

### Task 18: First-evening acceptance pass

**Files:**
- Create: `docs/acceptance/first-evening.md` (the checklist and its results)

- [ ] **Step 1: Run** `cargo test --workspace` and `cargo build --release -p nullwave`. Expected: all tests pass and the release build succeeds.
- [ ] **Step 2: Walk through the spec's success scenario** in the release build and record pass/fail for each line in `docs/acceptance/first-evening.md`:
  - New project.
  - Add a sample folder (a USB drive or any folder).
  - Preview samples.
  - Drag a kick, a snare and a hat into the rack.
  - Make 4 patterns with notes on 2–3 instruments each.
  - Play and loop each pattern, and change the tempo while playing.
  - Adjust volume, pan and release.
  - Shape a sound in the sampler: trim a sample, loop a pad, give a bass a filter-envelope pluck, and choke an open hat with a closed one.
  - Save, quit and relaunch: the project, the tabs and the panel sizes come back exactly as they were.
  - At no point does a floating window appear.
- [ ] **Step 3: Check the anti-goals:** count every window the OS shows during the walkthrough. Expected: one, plus the native file pickers only.
- [ ] **Step 4: Commit** with the message `docs: first-evening acceptance results`.
