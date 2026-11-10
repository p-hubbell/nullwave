# Timelines Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the channel-rack pattern with section timelines (lanes of numbered, linked clips), add a song timeline holding placed patterns, loose clips and audio clips, and keep old projects opening and sounding the same.

**Architecture:** `nw-core` gets a `Timeline` (clips + placements) used by both `Pattern` and a new `Song`, clip-level edits, a v2 file layout with legacy migration, and a pure `flatten` that turns timelines into note lists. `nw-engine` compiles those lists (plus audio lanes as one-shot samplers) into the existing flat event snapshot, gaining a play-once mode and seeking. `nw-workspace` gets the pure timeline geometry and hit testing; the app gets one timeline component used by the pattern tab and the song tab, with the existing piano roll embedded underneath.

**Tech Stack:** Rust 2024 workspace; GPUI 0.2.2; cpal/rtrb/basedrop/`assert_no_alloc`; serde + RON.

**Spec:** `docs/superpowers/specs/2026-11-09-nullwave-timelines-design.md` (builds on `docs/superpowers/specs/2026-11-02-nullwave-design.md`).

## Global Constraints

- Audio thread: no allocation, locks, I/O or logging; talk to the UI only through the existing queues; snapshots are built off-thread.
- One window; nothing floats; opening an open thing focuses its tab; the workspace restores exactly.
- Explicit save (⌘S), dirty dots per tab, crash recovery and outside-change detection keep working for every new file.
- Every edit is exactly one undo step (`History::commit` with one group).
- Every action is a named command in the palette.
- Project files stay human-readable RON; one file per clip's notes.
- New patterns are named "Pattern N"; nothing names sections for the user.
- Defaults must render old projects exactly as before (anti-goal 4).
- Each task: tests first where possible, `cargo test --workspace` and `cargo clippy --workspace --all-targets` clean (the four pre-existing warnings excepted), then the user's manual checks for UI tasks, then commit with the given message ending in `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. An existing `.nullwave/workspace.ron` containing `PianoRoll(..)` tabs must not reset the whole workspace; those tabs become their pattern's tab (test in Task 6).
2. Deleting an instrument whose clips sit in several patterns and in the song, then undoing, restores every clip and placement (test in Task 2).
3. Dragging a clip's left edge past the clip's original beginning, or past its end, must not underflow or produce a zero-length clip (test in Task 1).
4. Pressing Space in an empty song plays one silent bar and stops; it never divides by zero or loops forever (test in Task 5).
5. A song that references a missing clip file fails to load with that file named; a missing audio file loads, plays silent and is flagged (tests in Tasks 3 and 5).

---

### Task 1: Clip and timeline model

**Files:**
- Modify: `crates/nw-core/src/model.rs`
- Create: `crates/nw-core/src/clip.rs` (clip maths), register in `crates/nw-core/src/lib.rs`
- Test: `crates/nw-core/tests/clip_tests.rs`, update `crates/nw-core/tests/model_tests.rs`

**Interfaces:**
- Produces (in `model.rs`):
  - `pub type ClipNumber = u32;`
  - `pub enum Container { Pattern(PatternId), Song }` (Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)
  - `pub struct ClipKey { pub instrument: InstrumentId, pub number: ClipNumber }` (same derives)
  - `pub struct ClipRef { pub container: Container, pub key: ClipKey }` (same derives)
  - `pub struct Clip { pub key: ClipKey, pub start: Tick, pub length: Tick, pub notes: Vec<Note> }` — notes in clip-local ticks, sorted by `(start, pitch)`; the audible window is `[start, start + length)`.
  - `pub struct Placement { pub clip: ClipKey, pub at: Tick }` — `at` is where the window's first tick sounds.
  - `pub struct Timeline { pub clips: Vec<Clip>, pub placements: Vec<Placement> }` (Default) with `clip(&ClipKey) -> Option<&Clip>`, `clip_mut`, `uses(&ClipKey) -> usize`, `next_number(&InstrumentId) -> ClipNumber` (highest used + 1, first is 1), `end() -> Tick` (max `at + length`, 0 if empty).
  - `Pattern { id, name, timeline: Timeline }` (the `notes` map is gone).
  - `pub struct PatternPlacement { pub pattern: PatternId, pub at: Tick }`
  - `pub struct AudioLaneId(pub String)` (transparent, like `InstrumentId`); `pub struct AudioLane { pub id: AudioLaneId, pub path: PathBuf, pub volume: f32 }`; `pub struct AudioPlacement { pub lane: AudioLaneId, pub at: Tick }`
  - `pub struct Song { pub timeline: Timeline, pub patterns: Vec<PatternPlacement>, pub audio_lanes: Vec<AudioLane>, pub audio: Vec<AudioPlacement> }` (Default)
  - `Project { tempo, time_signature, instruments, patterns, song: Song }`
  - `Project::timeline(&Container) -> Option<&Timeline>`, `timeline_mut`, `pattern_length(&PatternId) -> Tick` (timeline end rounded up to a bar, minimum one bar).
  - `pub enum PlayTarget { Pattern(PatternId), Song }`
- Produces (in `clip.rs`):
  - `Clip::new(key: ClipKey, length: Tick) -> Clip` (start 0, no notes)
  - `Clip::audible(&self) -> impl Iterator<Item = Note> + '_` — notes whose start lies in the window, re-timed to window-relative ticks, length cut at the window end. Notes starting before the window are silent.
  - `resize_end(clip: &Clip, length: Tick) -> Clip` — length clamped to at least 1.
  - `trim_start(clip: &Clip, delta: i64) -> (Clip, i64)` — moves the window's start by `delta` keeping notes fixed in time; returns the clip and the delta actually applied (every placement of the clip moves by it). `delta` is clamped so length stays ≥ 1. If the new start would be negative, all notes shift right by the shortfall and `start` becomes 0.

- [ ] **Step 1: Write failing tests in `clip_tests.rs`**
  - `audible_cuts_notes_at_window_end`: clip start 0, length 960, note at 480 length 960 → one note, start 480, length 480.
  - `audible_skips_notes_before_window`: start 480, notes at 0 and 600 → one note at 120.
  - `trim_start_keeps_notes_in_time`: start 0, length 3840, note at 960; `trim_start(+960)` → start 960, length 2880, note still at 960, applied 960.
  - `trim_start_past_beginning_rebases` (Review Focus 3): start 0, note at 0; `trim_start(-480)` → start 0, length + 480, note at 480, applied −480.
  - `trim_start_never_empties`: length 960, `trim_start(5000)` → length 1, applied 959. `resize_end(_, 0)` → length 1.
  - `next_number_and_uses`: timeline with bell 1 placed twice, bell 3 once → `uses(bell 1) == 2`, `next_number(bell) == 4`, `next_number(kick) == 1`, `end()` correct.
  - In `model_tests.rs`, replace the old `pattern_length` tests: a pattern with one 1-bar clip placed at bar 3 → length 3 bars; empty → 1 bar.
- [ ] **Step 2:** `cargo test -p nw-core --test clip_tests` → FAIL (types missing).
- [ ] **Step 3: Implement the types and functions above** and remove `Pattern::notes`. This breaks `edit.rs` and `io/`; Tasks 2 and 3 rewrite them, so Tasks 1–3 are done on one working tree and committed together. Downstream crates stay broken until Tasks 5, 6 and 8.
- [ ] **Step 4:** `cargo test -p nw-core --test clip_tests --test model_tests` → PASS.
- [ ] **Step 5:** commit with Task 3.

### Task 2: Edits and arrangement operations

**Files:**
- Modify: `crates/nw-core/src/edit.rs`, `crates/nw-core/src/naming.rs`
- Create: `crates/nw-core/src/arrange.rs` (pure ops returning edit groups)
- Test: rewrite `crates/nw-core/tests/edit_tests.rs`; create `crates/nw-core/tests/arrange_tests.rs`

**Interfaces:**
- Consumes: Task 1 types.
- Produces — `DocPath` becomes: `Song` (song.ron), `Instrument(InstrumentId)`, `PatternLayout(PatternId)` (placements of a pattern), `Arrangement` (song placements, audio lanes), `Clip(ClipRef)`.
- Produces — `Edit` variants (note-level variants are removed; the piano roll edits whole clips):
  - `ReplaceClip { container: Container, before: Option<Clip>, after: Option<Clip> }` (add / change / remove; keyed by `ClipKey`)
  - `ReplacePlacements { container: Container, before: Vec<Placement>, after: Vec<Placement> }` (removal by equality, like notes today; kept sorted by `(at, clip)`)
  - `ReplacePatternPlacements { before: Vec<PatternPlacement>, after: Vec<PatternPlacement> }`
  - `ReplaceAudioPlacements { before: Vec<AudioPlacement>, after: Vec<AudioPlacement> }`
  - `AddAudioLane { lane: AudioLane, index: usize }`, `RemoveAudioLane { lane, index }`, `UpdateAudioLane { before: AudioLane, after: AudioLane }`
  - `RemoveInstrument`/`RestoreInstrument` carry `removed: Vec<(Container, Vec<Clip>, Vec<Placement>)>` instead of `notes`.
  - `AddInstrument`, `UpdateInstrument`, `AddPattern { pattern, index }`, `RemovePattern`, `RenamePattern`, `SetTempo` unchanged in meaning.
  - `touched()`: clip edits → `Clip(ref)`; placement edits → `PatternLayout(p)` or `Arrangement`; pattern placements, audio placements and audio lanes → `Arrangement`; Add/RemovePattern → `Song`, `PatternLayout`, and each clip.
- Produces — `arrange.rs` (each returns the edits for one undo step):
  - `new_clip(p: &Project, c: &Container, instrument: &InstrumentId, at: Tick, length: Tick) -> (Vec<Edit>, ClipKey)`
  - `move_placements(p, c, moves: &[(Placement, Tick)]) -> Vec<Edit>`
  - `delete_placements(p, c, targets: &[Placement]) -> Vec<Edit>` — also removes any clip left with no placement.
  - `duplicate_placements(p, c, targets: &[Placement]) -> (Vec<Edit>, Vec<Placement>)` — copies land right after the selection's span, same clip numbers.
  - `make_unique(p, c, target: &Placement) -> (Vec<Edit>, ClipKey)` — copies the clip under `next_number`, repoints that one placement, removes the old clip if now unused.
  - `resize_clip(p, r: &ClipRef, length: Tick) -> Vec<Edit>`; `trim_clip_start(p, r, delta: i64) -> Vec<Edit>` (moves every placement of the clip by the applied delta; placements that would go below 0 clamp the delta).
  - `edit_clip_notes(p, r, notes: Vec<Note>) -> Vec<Edit>` (used by the piano roll).
  - `duplicate_pattern(p, id: &PatternId) -> (Vec<Edit>, PatternId)` — new id/name from `naming::next_pattern`, appended at the end, independent clip copies.
  - `place_pattern(p, id, at) -> Vec<Edit>`, `move_pattern_placements(p, moves: &[(PatternPlacement, Tick)])`, `delete_pattern_placements(p, &[PatternPlacement])`, `duplicate_pattern_placements(p, &[PatternPlacement]) -> (Vec<Edit>, Vec<PatternPlacement>)`
  - `delete_pattern(p, id) -> Vec<Edit>` — also removes its song placements.
  - `add_audio(p, path: &Path, at: Tick) -> Vec<Edit>` — reuses the lane whose `path` matches, else adds a lane (id from the file stem via `naming`, volume 0.78).
  - `move_audio`, `delete_audio` (removes a lane left with no placements), `set_audio_volume(p, lane, volume)`.
  - `remove_instrument_edit(p, id) -> Option<Edit>` captures clips and placements in every pattern and the song.

- [ ] **Step 1: Write failing tests**
  - `edit_tests.rs`: for every variant, `apply` then `inverse().apply` restores the project (`assert_eq!`), and `touched()` names the expected `DocPath`s.
  - `arrange_tests.rs`:
    - `delete_last_placement_removes_clip` and undo brings both back (apply edits, then inverses in reverse).
    - `make_unique_repoints_one_placement`: bell 1 placed twice → after, one placement is bell 2 with equal notes, bell 1 still used once.
    - `duplicate_pattern_is_independent`: duplicate, then `edit_clip_notes` on the copy → original pattern's clip unchanged; copy is named "Pattern N" with the next N.
    - `trim_moves_every_placement`: clip placed at 0 and 3840, trim +960 → placements at 960 and 4800.
    - `trim_clamps_at_zero`: clip with start 0 placed at 0, `trim_clip_start(−960)` → no edits (no placement can move left of 0); placed at 1920 instead → placement at 960, clip rebased per `trim_start`.
    - `remove_instrument_restores_everywhere` (Review Focus 2): instrument with clips in two patterns and the song; remove then inverse → identical project.
    - `delete_pattern_removes_song_placements` and its inverse restores both.
    - `add_audio_reuses_lane`: same path twice → one lane, two placements.
- [ ] **Step 2:** `cargo test -p nw-core --test edit_tests --test arrange_tests` → FAIL.
- [ ] **Step 3: Implement** the variants and ops. Edits in one group are applied in order and undone in reverse, so ops emit placements-removal before clip-removal and clip-add before placement-add.
- [ ] **Step 4:** run the same tests → PASS.
- [ ] **Step 5:** commit together with Task 3.

### Task 3: File format v2 and migration

**Files:**
- Modify: `crates/nw-core/src/io/files.rs`, `crates/nw-core/src/io/recovery.rs` (only if it names `DocPath::Notes`)
- Test: `crates/nw-core/tests/io_tests.rs`, `crates/nw-core/tests/recovery_watch_tests.rs`; fixture `crates/nw-core/tests/fixtures/v1-project/` (copy of a project saved by the first version: song.ron, one instrument, two patterns with notes)

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces — layout (relative paths from `DocPath::relative_path`):
  - `song.ron`: `format: 2`, tempo, time signature, instrument ids, pattern `(id, name)` list.
  - `patterns/<pattern>/pattern.ron`: `placements: [(clip: (instrument, number), at)]`.
  - `patterns/<pattern>/clips/<instrument>-<number>.ron`: `(start, length, notes)`.
  - `song/arrangement.ron`: pattern placements, clip placements, audio lanes, audio placements.
  - `song/clips/<instrument>-<number>.ron`.
  - Clips are discovered from placements; a placement whose clip file is missing is `LoadError::Invalid` naming the clip file.
- Migration: a `song.ron` without `format` (serde default 0) is v1. Each `patterns/<p>/<instrument>.ron` becomes clip `(instrument, 1)` with start 0, length = the v1 pattern length (last note end rounded up to a bar, minimum one bar, computed across all instruments of that pattern), placed at 0. The song starts empty. Saving writes v2 and `remove_stale` (extended to `song/`) deletes the v1 files.
- Validation adds: clip `length ≥ 1`; audio lane volume in `0..=1`; placements referencing unknown instruments, patterns or audio lanes are `Invalid`.

- [ ] **Step 1: Write failing tests**
  - `v2_round_trip`: a project with two patterns (one with a repeated clip and a trimmed clip), song placements of both patterns, a loose clip and an audio lane → save, load, `assert_eq!`.
  - `v1_fixture_migrates`: load the fixture → each pattern has clip 1 per instrument at 0, `pattern_length` equals the v1 length, the notes are identical; save then list files → no `patterns/<p>/<instrument>.ron` left, `format: 2` in song.ron.
  - `v1_sounds_the_same` (anti-goal 4): `flatten::pattern_notes` (Task 4) of the migrated pattern equals the v1 notes sorted — add this assertion in Task 4.
  - `missing_clip_file_is_named` (Review Focus 5): delete one clip file → `LoadError::Invalid` whose path is that file.
  - `zero_length_clip_rejected`.
  - Existing recovery and outside-change tests updated to the new files and passing.
- [ ] **Step 2:** `cargo test -p nw-core` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `cargo test -p nw-core` → PASS.
- [ ] **Step 5: Commit** Tasks 1–3 together: `feat(core): clip timelines, song model and v2 project files`.

### Task 4: Flattening timelines into notes

**Files:**
- Create: `crates/nw-core/src/flatten.rs`
- Test: `crates/nw-core/tests/flatten_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct FlatNote { pub tick: Tick, pub instrument: InstrumentId, pub pitch: u8, pub velocity: u8, pub length: Tick }`
  - `pub fn pattern_notes(p: &Project, id: &PatternId) -> Vec<FlatNote>` — every placement's `Clip::audible()` offset by `at`.
  - `pub fn song_notes(p: &Project) -> Vec<FlatNote>` — loose song clips plus each placed pattern's notes offset by its `at`.
  - Both sorted by `(tick, instrument, pitch)`.
  - `pub fn song_length(p: &Project, audio_ticks: &dyn Fn(&AudioLaneId) -> Tick) -> Tick` — the end of the last loose clip, placed pattern (`at + pattern_length`) or audio placement, rounded up to a bar, minimum one bar.

- [ ] **Step 1: Failing tests:** repeated clip fires twice; a note running past its clip's end is cut there; pattern placed twice in the song fires twice with the right offsets; overlapping pattern and loose clip on the same instrument both fire; empty song → no notes, `song_length` one bar; audio-only song → length covers the audio; plus the `v1_sounds_the_same` assertion from Task 3.
- [ ] **Step 2:** run → FAIL. **Step 3:** implement. **Step 4:** run → PASS.
- [ ] **Step 5: Commit** `feat(core): flatten patterns and the song into notes`.

### Task 5: Engine: play targets, audio lanes, play-once and seeking

**Files:**
- Modify: `crates/nw-engine/src/snapshot.rs`, `crates/nw-engine/src/core.rs`, `crates/nw-engine/src/voice.rs`
- Test: `crates/nw-engine/tests/playback_tests.rs`, `crates/nw-engine/tests/core_tests.rs`; fix the remaining engine tests to the new model

**Interfaces:**
- Consumes: `PlayTarget`, `flatten::{pattern_notes, song_notes, song_length}`.
- Produces:
  - `compile_snapshot(project: &Project, target: Option<&PlayTarget>, samples: &mut SampleCache) -> Snapshot`
  - `Snapshot { tempo, length: Tick, looping: bool, instruments, events }` — `length` replaces `loop_length`; patterns loop, the song does not.
  - Instruments are the project's instruments followed by one per audio lane, built from `Sampler { sample_path: Some(lane.path), one_shot: true, root_note: 60, ..Sampler::default() }` with `volume = lane.volume`, pan 0. An audio placement becomes a `NoteEvent { pitch: 60, velocity: 127, length: <file length in ticks> }`. A missing file gives a silent instrument (as missing samples do today).
  - `Command::Seek(Tick)` — sets the position (and cursor) without changing play state.
  - `Command::Play` no longer resets the position to 0; the app seeks first.
  - On reaching `length` with `looping == false`: release all voices, stop, keep the position at `length`, and report `playing: false`.
  - `VoiceStart` gains `pub offset: f64` (source frames into the region). On `Play` (and on `Seek` while playing), audio events whose span covers the position start a voice at the matching offset, so audio is heard from mid-file. Note events are not chased.

- [ ] **Step 1: Failing tests**
  - `song_plays_once_and_stops`: song with one pattern placed at bar 2 → events fire at the right frames, transport stops at the song end, position does not wrap.
  - `empty_song_stops_after_one_bar` (Review Focus 4).
  - `seek_then_play_starts_there`: seek to bar 3 of a 4-bar pattern → first event fired is the one at bar 3; after the end it loops to bar 1.
  - `audio_lane_plays_file_at_root_pitch`: rendered output matches the sample's frames (scaled by volume and centre pan) — reuse the tuning test's comparison.
  - `audio_chases_mid_file`: seek into the middle of an audio placement, play → first output frames match the sample from that offset.
  - `missing_audio_is_silent` (Review Focus 5).
  - `process_does_not_allocate` still passes with a song snapshot including audio.
- [ ] **Step 2:** `cargo test -p nw-engine` → FAIL. **Step 3:** implement. **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(engine): song playback, audio lanes, seeking`.

### Task 6: Workspace state for timelines

**Files:**
- Modify: `crates/nw-workspace/src/tabs.rs`, `crates/nw-workspace/src/dirty.rs`, `crates/nw-workspace/src/commands.rs`
- Test: `crates/nw-workspace/tests/tabs_tests.rs`, `dirty_tests.rs`, `commands_tests.rs`

**Interfaces:**
- Produces:
  - `TabKind { Pattern(PatternId), Song, Instrument(InstrumentId), PianoRoll(PatternId, InstrumentId) }` — `PianoRoll` stays only so old workspace files parse; `retain_valid` turns it into `Pattern(p)` (deduplicated, keeping the active one active).
  - `pub struct TimelineTabState { pub tab: TabKind, pub open_clip: Option<ClipKey>, pub roll_height: f32, pub px_per_beat: f32, pub scroll_ticks: f32 }`; `WorkspaceState` gains `#[serde(default)] pub timelines: Vec<TimelineTabState>` with `timeline(&TabKind) -> TimelineTabState` (defaults: no clip, `roll_height` 240, `px_per_beat` 24, scroll 0) and `set_timeline(TimelineTabState)`.
  - `WorkspaceState::play_target() -> Option<PlayTarget>` replaces `active_pattern()`.
  - `tab_title`: `Song` → "Song".
  - `pub fn clip_header(p: &Project, r: &ClipRef) -> String` — `Pattern 1 › bell 2 · used 3×`; when the pattern is placed in the song, `Pattern 1 › bell 2 · 3× in pattern, pattern placed 2× in song`; loose song clips `Song › bell 3 · used 2×`.
  - Dirty: `Pattern(p)` is dirty on `PatternLayout(p)` or any `Clip` in `Container::Pattern(p)`; `Song` on `Arrangement` or any song clip.
  - Commands (id → title, binding, context): `project.open_song` "Open Song"; `pattern.duplicate` "Duplicate Pattern"; `timeline.duplicate` "Duplicate" (cmd-d, `Timeline`); `timeline.delete` "Delete" (backspace, `Timeline`); `timeline.make_unique` "Make Unique"; `timeline.close_roll` "Close Piano Roll" (escape, `Timeline`).

- [ ] **Step 1: Failing tests:** `legacy_piano_roll_tabs_become_pattern_tabs` (Review Focus 1: parse a workspace string containing `PianoRoll(("pattern-1"), ("drums"))` and `Pattern(("pattern-1"))` → one `Pattern` tab, sidebar sizes kept); `timeline_state_round_trips`; `clip_header` cases above; dirty cases; the new commands exist and bindings resolve in context.
- [ ] **Step 2:** → FAIL. **Step 3:** implement. **Step 4:** `cargo test -p nw-workspace` → PASS.
- [ ] **Step 5: Commit** `feat(workspace): song tab, timeline view state, clip headers`.

### Task 7: Timeline geometry and hit testing

**Files:**
- Create: `crates/nw-workspace/src/timeline.rs`
- Test: `crates/nw-workspace/tests/timeline_tests.rs`

**Interfaces:**
- Produces:
  - `pub enum Lane { Instrument(InstrumentId), Audio(AudioLaneId), AddInstrument }`; `pub fn lanes(p: &Project, c: &Container) -> Vec<Lane>` (instruments in project order; audio lanes only for `Song`; `AddInstrument` last).
  - `pub enum Owner { Direct, Pattern(PatternPlacement) }`
  - `pub enum BlockKind { Clip { placement: Placement, owner: Owner }, Audio(AudioPlacement) }`
  - `pub struct Block { pub kind: BlockKind, pub lane: usize, pub start: Tick, pub end: Tick, pub label: String }` — label is the clip number, or the file name for audio.
  - `pub fn blocks(p, c, audio_ticks: &dyn Fn(&AudioLaneId) -> Tick) -> Vec<Block>` — in the song, a placed pattern's clips appear with `Owner::Pattern` at `placement.at + clip.at`.
  - `pub struct Outline { pub placement: PatternPlacement, pub start: Tick, pub end: Tick, pub name: String }`; `pub fn outlines(p) -> Vec<Outline>`.
  - `pub struct TimelineView { pub px_per_tick: f32, pub scroll: f32, pub lane_height: f32, pub header_width: f32, pub ruler_height: f32 }` with `x_of(tick) -> f32`, `tick_at(x) -> f32`, `lane_at(y) -> Option<usize>`.
  - `pub enum Edge { Left, Right, Body }` (edges are the outer 6 px of a block, never more than a third of it)
  - `pub enum TimelineHit { Ruler(Tick), Header(usize), Block { index: usize, edge: Edge }, OutlineLabel(PatternPlacement), Empty { lane: usize, tick: Tick, inside: Option<PatternPlacement> }, Nothing }`
  - `pub fn hit_test(view, lanes, blocks, outlines, x, y) -> TimelineHit` — order: ruler, headers, `Direct` blocks, outline labels (an 18 px tab at the outline's top-left), pattern blocks, empty space (reporting the outline it falls inside).
  - `pub fn to_pattern_local(outline_at: Tick, tick: Tick) -> Option<Tick>` — `None` before the outline start (drags clamp to the start).
  - Snapping reuses `piano_roll::snap_floor`; default snap is one bar; ⌘ disables it (the app passes `snap = 1`).

- [ ] **Step 1: Failing tests:** lane order with and without audio; song blocks of a twice-placed pattern have the right starts and `Owner`; a loose clip overlapping an outline wins the hit; the outline label is hit only in its tab; empty space inside an outline reports it; edges on a tiny block stay ≤ a third; `tick_at(x_of(t)) == t` across zoom levels; clamping before an outline start.
- [ ] **Step 2:** → FAIL. **Step 3:** implement. **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(workspace): timeline geometry and hit testing`.

### Task 8: Pattern tab as a timeline, with the piano roll underneath

**Files:**
- Create: `crates/nullwave/src/ui/timeline.rs` (canvas painting + mouse handling for one `Container`)
- Delete: `crates/nullwave/src/ui/pattern_tab.rs` (channel rack)
- Modify: `crates/nullwave/src/ui/editor_area.rs`, `crates/nullwave/src/ui/piano_roll.rs` (`RollKey` becomes `ClipRef`; shows the whole clip with the hidden parts outside `[start, start+length)` dimmed; edits go through `arrange::edit_clip_notes`), `crates/nullwave/src/ui/shell.rs`, `crates/nullwave/src/ui/bottom_panel.rs` (velocity lane follows the open clip), `crates/nullwave/src/ui/sidebar.rs` (drop targets move to lane headers and the add row), `crates/nullwave/src/app_state.rs` (`resync_engine` uses `play_target()`), `crates/nullwave/src/actions.rs`

**Interfaces:**
- Consumes: Tasks 2, 6, 7. Every mouse gesture ends in exactly one `AppState::apply(edits)` built from an `arrange` op; drags preview locally and commit on mouse-up, like the instrument tab's drags.
- Behaviour (pattern tab):
  - Click empty lane space → `new_clip` (one bar at the snapped tick), open it in the roll.
  - Click a block → select it and open its clip; other placements of that clip get a highlight border; the roll header shows `clip_header`.
  - Drag body → move (multi-select with ⇧-click); drag right edge → `resize_clip`; drag left edge → `trim_clip_start`; ⌥-drag → duplicate as the same clip; ⌘D, Backspace, Make Unique commands.
  - A snap menu on the timeline, like the piano roll's; bar by default; holding ⌘ while dragging disables snapping.
  - The roll sits below the timeline with a drag handle for its height (`roll_height`, saved); Escape or its close button hides it.
  - Lane header: name (click opens the instrument tab), missing-sample marker, sample drop target. "+ add instrument" row accepts sample drops.
  - Space loops the pattern; the roll's playhead shows inside the open clip at the placement that contains the playhead.
- [ ] **Step 1:** Unit-test any pure helper you add here (e.g. marquee/selection maths) in the module's `#[cfg(test)]`.
- [ ] **Step 2:** `cargo test --workspace` and clippy clean; `cargo run -p nullwave`.
- [ ] **Step 3: User's manual checks:** an old project opens with each pattern as clip-1 lanes and sounds the same; make a clip, draw notes in the roll under it; ⌥-drag it — editing one changes both; Make Unique → number 2, now independent; resize and left-trim behave as the spec says; delete the last copy then ⌘Z; the roll height persists across relaunch; every gesture is one ⌘Z step.
- [ ] **Step 4: Commit** `feat(app): pattern timeline with embedded piano roll`.

### Task 9: Song tab

**Files:**
- Modify: `crates/nullwave/src/ui/timeline.rs` (outlines, audio lanes), `crates/nullwave/src/ui/sidebar.rs` ("Song" entry at the top of the project section; patterns draggable), `crates/nullwave/src/ui/status_bar.rs` (shows "Song" or the pattern name), `crates/nullwave/src/app_state.rs` (song playhead kept in the app: Space in the song seeks to it then plays; clicking the ruler moves it; `duplicate_pattern` command opens the copy), `crates/nullwave/src/actions.rs`, `crates/nullwave/src/ui/palette.rs`

**Interfaces:**
- Consumes: Tasks 2, 5, 6, 7.
- Behaviour (song tab):
  - Drag a pattern from the sidebar onto the timeline → `place_pattern` at the snapped tick. Its clips appear in the lanes with an outline and a name tab; drag the tab to move it; ⌘D/Backspace act on selected placements.
  - Inside an outline everything from Task 8 works and edits the pattern (convert with `to_pattern_local`; a drag before the outline start stops at it).
  - Empty space outside outlines → `new_clip` in `Container::Song` (loose clip).
  - Drop a sample from the browser onto the timeline area → `add_audio` at the snapped tick. Audio lane header has a volume slider (`set_audio_volume`, committed on release) and a missing marker.
  - Space plays once from the song playhead; the playhead line is drawn across all lanes.
  - "Duplicate Pattern" (palette, and on the sidebar's pattern row wherever Rename and Delete live today) makes the copy and opens its tab.
- [ ] **Step 1:** `cargo test --workspace`, clippy clean, run the app.
- [ ] **Step 2: User's manual checks:** duplicate a pattern and rework it without the original changing; place both in the song in any order; edit a placed pattern in the song and see every placement and the pattern tab change; add a clip inside an outline; add a loose clip; drop a producer tag at bar 1 and hear it, also when starting mid-file; play stops at the end; save, quit, relaunch → song, tabs and roll heights come back.
- [ ] **Step 3: Commit** `feat(app): song timeline with patterns, loose clips and audio`.

### Task 10: Acceptance pass

**Files:**
- Create: `docs/acceptance/timelines.md`

- [ ] **Step 1:** `cargo test --workspace` and `cargo build --release -p nullwave` → pass.
- [ ] **Step 2: User walks through in the release build**, recording pass/fail per line: take a hook pattern; Duplicate Pattern and rework the copy into another section; repeat for a third; lay the sections out in the song in a non-hook-first order; add a producer tag at the start and a loose clip that drops in now and then; edit a section from inside the song; play the whole beat start to finish; check each of the four anti-goals (no surprise edits, no tab juggling, growing a section is one command, an old project opens and sounds the same); save, quit, relaunch.
- [ ] **Step 3: Commit** `docs: timelines acceptance results`.
