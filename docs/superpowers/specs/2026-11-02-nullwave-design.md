# Nullwave — Design (first version)

Date: 2026-11-02
Status: draft for review
Revised 2026-11-04: the sampler grows from a minimal one-shot player to an FL-style sampler (see "Sampler"). The user asked to pack in as much of FL's Sampler channel as possible, all of it before the first-evening acceptance pass.
Background: `references/initial-concept.md` (the original concept outline) and `references/Screenshot 2026-11-02 at 8.31.17 PM.png` (rough wireframe). This document narrows and in places overrides the concept outline; where they disagree, this document wins.

## Intent and why

Nullwave is a music production app (DAW) shaped like a code editor — specifically like VS Code and Zed, the editors the user lives in.

The user produces in FL Studio. The music is fine; working in FL is not. FL feels unorganized and overwhelming, and the worst moment is when the playlist, piano roll, mixer and plugin windows are all open and overlapping. IDEs feel the opposite: every view has a fixed home, nothing floats, and you only see what you chose to see.

"IDE-shaped DAW" is the identity, not just a means to tidiness. Someone who uses VS Code should open Nullwave and recognise it immediately: file tree on the left, tabs across the top, a panel at the bottom, a status bar, a command palette, keyboard shortcuts for everything, and a project that is just a folder of files.

The user is the first and primary user. This is a personal tool; it does not need to compete with FL on features, it needs to feel better to sit in.

## Success for the first version

The user opens Nullwave for an evening instead of FL and, by the end, has created as many patterns as they want — each with notes for several instruments — and heard them play. The work is saved and reopens exactly as left.

## Goals

- A calm, organised workspace that looks and behaves like VS Code (layout below).
- FL's mental model of instruments and patterns, kept intact (model below).
- Make notes in a piano roll and hear them, with low enough latency to feel responsive.
- Everything reachable by keyboard and by mouse; every action is a named command in the palette.
- Projects saved as a folder of human-readable text files.

## Non-goals (first version)

- Third-party plugins (VST3/AU/CLAP). Omnisphere etc. come in a later phase.
- Song arrangement timeline (comes right after the first version).
- Mixer, buses, effects, built-in synth. (The sampler's own filter and modulation are part of the instrument, not effects.)
- MIDI keyboard input and recording notes (the user wants this in the future, not in the first version).
- Audio recording, audio clips, automation.
- WAV export, `.mid` export.
- Git integration inside the app (the project folder just stays git-friendly).
- Windows and Linux.

## Anti-goals (failure even if it technically works)

- **Window juggling.** Having to open and close windows over and over, like FL. Concretely:
  - Nothing ever opens as a floating window — not instruments, not settings, not dialogs that could be a tab or panel instead.
  - Opening something that is already open focuses its existing tab; it never creates a duplicate.
  - The workspace (open tabs, active tab, sidebar/panel visibility and sizes) is restored exactly on reopen.
  - Nothing appears, splits or rearranges on its own.
- **Clutter.** The default screen shows one editor and the panels the user chose. Density is a cost, not a feature.

## Constraints (decided by the user)

- **Rust stack from the concept outline, kept**: GPUI (standalone) for UI, `cpal` for audio output, `rtrb` for UI↔engine messaging, `symphonia` for decoding samples, `rubato` for resampling, `assert_no_alloc` / `basedrop` for real-time safety, `serde` for project files. `midir`, `fundsp`, `nih-plug`, `clack` are later-phase.
- **Built-in instruments only at first.** No plugin hosting in the first version.
- Real-time rules from the concept outline apply unchanged: the audio thread never allocates, locks, does I/O or logs; it talks to the UI only through preallocated lock-free queues; graph changes are built off-thread and swapped in.

## What the user decided

### Workspace layout ("layout A", VS Code-style)

One window. From left to right, top to bottom:

- **Activity bar** (far left, narrow): icons that switch what the sidebar shows — Project, Sounds, Search; Version Control later.
- **Sidebar**: the project tree and sample folders, with the sections from the wireframe:
  - **project** — patterns (and later arrangements, etc.)
  - **presets** — saved setups such as master bus / drum bus / melody bus. *Visible as a section, but empty/inert until the mixer exists.*
  - **sample folders** — the user's USB drive (drum kits, loops, one-shots) and any other folders they add.
- **Editor area**: tabbed. **One editor visible at a time by default.** Splitting is possible but only when the user explicitly asks (command or drag) — never automatically. The user is wary that splits are overwhelming.
- **Bottom panel**: hideable, with tabs. Its first tab follows the active editor (for a piano roll: velocity lane). Other tabs: Log (MIDI and Mixer later).
- **Status bar**: left — play / stop (record arrives with MIDI input), playhead position, active pattern, tempo, time signature. Right — sample rate, buffer size, CPU load (git branch later).
- **Command palette**: overlay at the top, opened with ⌘⇧P (and ⌘K-style quick open), fuzzy search over commands, patterns, instruments and files.
- **Right-side panel: an open slot.** The layout should allow a right dock, but nothing goes in it in the first version. The user is open to one if a good use comes up (a candidate: controls for the selected instrument while the piano roll stays open).

### Instruments, patterns and notes (FL's model)

- A project has a list of **instruments** (like FL's channel rack). In the first version every instrument is a sampler.
- A project has any number of **patterns**.
- Each pattern holds **at most one note sequence per instrument**. Instruments with no notes in a pattern are simply empty for that pattern.
- **Playing a pattern** plays every instrument's notes in that pattern together, looping, at the project tempo (like FL's pattern mode).
- Each instrument has a basic **volume and pan**, so patterns are listenable before a mixer exists.
- Drums are written as notes in the piano roll. An FL-style step sequencer is a possible later editor.

### Channel rack ("option B": a pattern opens as its own tab)

- Clicking a pattern in the sidebar opens a **pattern tab** that looks like FL's channel rack: one row per instrument, each row showing a small preview of that instrument's notes in this pattern; empty rows are visibly empty (dashed).
- **Double-clicking a row opens that instrument's piano roll** for this pattern as another tab, titled `pattern 1 / Drums`.
- An **"+ add instrument"** row at the bottom.
- Tabs follow the anti-goal rule: opening an already-open pattern or piano roll focuses it.

### Piano roll

- Notes on a pitch × time grid with a keyboard strip on the left; add, move, resize, delete notes with mouse and keyboard.
- Velocity editing in the bottom panel.
- Snap to grid.

### Sampler (the one built-in instrument)

Modelled on FL Studio's Sampler channel, with as much of its feature set as fits the first version. The user's words: "pack in as many of FL sampler's features as we can."

**How we match FL.** Behaviour comes from Image-Line's public FL Studio manual (Sampler channel settings) and from black-box comparison: rendering the same notes in the user's own FL install and in Nullwave, then comparing by ear or with a spectrum view. We do **not** decompile or disassemble FL Studio: its licence forbids it, and code shaped by its binary would be a copyright liability for a tool we may ship. Where the manual and our ears disagree, our ears win.

One audio file per sampler instrument, played from the piano roll, pitched across the keys relative to a root note. Everything below is per instrument.

**A. Sample and playback**
- **Region:** start and end points, stored as fractions (0–1) of the file's length so they survive the file being re-exported at another length; start < end.
- **Loop:** off, forward or ping-pong, with loop start and end inside the region and a crossfade (0–500 ms) at the forward loop point so it doesn't click.
- **One-shot vs. held:** held (default, today's behaviour) ends the sound at the note's end and then releases; one-shot plays the whole region regardless of note length. With a loop on, the note's end always ends the sound.
- **Sample processing** (FL's "precomputed effects"): reverse, normalize, invert polarity, swap stereo, and declick (short fades at region and loop edges, on by default). These are computed off the audio thread when the sample or setting changes, never per note.
- **Pitch:** root note (0–127), coarse tune (±24 semitones) and fine tune (±100 cents).

**B. Envelopes, filter and modulation**
- **Volume envelope:** delay, attack, hold, decay, sustain and release (DAHDSR). Times 0–5000 ms (delay and hold 0–2000 ms), sustain 0–100 %, plus a curve (tension) for the attack and for decay/release. The default (all 0 ms, sustain 100 %, release 20 ms) sounds exactly like today's sampler.
- **Filter:** off, low-pass, high-pass, band-pass or notch (state-variable, 12 dB/oct), with cutoff 20 Hz–20 kHz on a log scale and resonance 0–100 %. Off by default.
- **Modulation envelopes:** DAHDSR envelopes for filter cutoff (±8 octaves), pitch (±24 semitones) and pan (±100 %). Each has an amount that defaults to 0, meaning off.
- **LFOs** for volume, pan, cutoff and pitch: shape (sine, triangle, saw, square, random/sample-and-hold), rate free (0.01–50 Hz) or tempo-synced (1/64 note to 8 bars), amount (0 = off; volume and pan 0–100 %, cutoff ±8 octaves, pitch ±24 semitones), and delay (0–5000 ms). Each LFO restarts with every note.
- **Velocity:** velocity → volume (0–100 %, default 100 % as today) and velocity → cutoff (±8 octaves, default 0).

**C. Voices**
- **Polyphony limit** (1–64 per instrument, default 64). Over the limit, that instrument's oldest voice fades out.
- **Mono** with **glide (portamento)** 0–2000 ms: one voice per instrument. With glide above 0, a new note while one sounds slides pitch without restarting the envelope (legato). With glide 0, it restarts the voice.
- **Cut groups**, FL-style: `cut` is the group this instrument's notes cut, and `cut by` is the group that cuts this instrument (e.g. an open hi-hat cut by a closed one). Groups 1–16; 0 means none.
- Any voice that is stolen, cut or limited fades over a few milliseconds rather than stopping dead, so it never clicks.

**Real-time and timing rules for all of the above**
- The audio thread never allocates. Every voice preallocates its filter, envelope, LFO and glide state, and processed samples arrive through the snapshot like the samples do today.
- Parameter edits commit when a control is released (one undo step) and apply to notes that start afterwards. Sounding voices keep the values they started with. Changing them live is on the roadmap.
- Old project files load unchanged: missing fields take the defaults above, and the existing `attack_ms` / `release_ms` become the volume envelope's attack and release. New values are range-checked on load like the existing ones.

**D. Instrument tab**
- Organised into sections: **Sample**, **Pitch**, **Envelopes**, **Filter**, **LFO**, **Velocity**, **Voice**.
- **Sample** shows the waveform with draggable start, end and loop markers, plus the processing toggles, loop mode, crossfade and one-shot. Waveform peaks are computed off the UI thread and cached.
- **Envelopes** has one graph per envelope (volume, cutoff, pitch, pan) with draggable points, and numeric sliders beside it.
- Every control commits one `UpdateInstrument` when the drag or edit ends, as today. A small ▶ button auditions the instrument at its root note.
- It is still a tab, never a floating window.

### Sample browsing

- Sample folders (including the USB drive) appear in the sidebar.
- *(My guess, accepted in playback:)* clicking a sample previews it; dragging a sample onto a pattern-tab row or onto "+ add instrument" loads it into a sampler.
- **Samples are referenced where they live, not copied — matching what FL Studio does** (the user asked for FL's behaviour). The instrument file stores the sample's full path, e.g. on the USB drive. If a sample is missing when the project opens (drive unplugged, other machine), the instrument loads silent and its channel-rack row shows a clear "missing sample" marker; reconnecting the drive and reopening restores it. A "collect samples into project" command (FL's equivalent is exporting a zipped project) is on the roadmap.

## Roadmap after the first version (roughly in order)

1. Song arrangement timeline (place patterns on a timeline).
2. Mixer with buses — this is where the sidebar **presets** section (master bus, drum bus, melody bus) comes alive.
3. Built-in synth and effects (gain, EQ, delay) via `nih-plug` / `fundsp`.
4. WAV export (offline render).
5. Third-party plugins: CLAP via `clack` first, then VST3/AU (needed for Omnisphere).
6. MIDI keyboard: play notes live and record them into the piano roll (`midir`).
7. Sampler extras: time-stretch (tempo-independent pitch), arpeggiator, granular mode, keep-on-disk streaming for very long samples, multi-sample key zones, and parameter changes that affect already-sounding voices.
8. Later: a "collect samples into project" command (like FL's zipped project export), audio recording, automation, in-app git panel, `.mid` export, step sequencer, extension API.

## Calls I made (the user can override)

- **macOS first.** That's what the user is on. Code shouldn't preclude Windows/Linux, but they aren't tested or shipped.
- **Single window**, no multi-window support in the first version.
- **No inspector panel** by default; note details show in the bottom panel / on hover.
- **Sampler only** in the first version; the synth moves to the roadmap. The user said an FL-style sampler is "good enough", and later asked for as much of FL's Sampler feature set as fits (see "Sampler").
- **Project format: RON**, one file per instrument and per pattern-instrument note sequence, so git diffs stay small. Suggested shape (the builder may refine):
  ```
  my-song/
    song.ron                  # tempo, time signature, instrument order, pattern order
    instruments/
      drums.ron               # sampler settings, sample reference, volume, pan
      synth-2.ron
    patterns/
      pattern-1/
        drums.ron             # notes for this instrument in this pattern
        synth-2.ron
    .nullwave/                # workspace state (open tabs, panel sizes) — git-ignored
    .gitignore
  ```
- **Pattern length is FL-style**: a pattern's loop length follows its last note across all instruments, rounded up to a whole bar, minimum one bar. No per-pattern length setting in the first version. Space plays the pattern in the focused tab.
- **Saving is explicit, like VS Code**: ⌘S saves; unsaved-change dots on tabs; a prompt on quit with unsaved changes. A pattern open in two tabs is one document. Unsaved work is also written to `.nullwave/` periodically for crash recovery. Undo/redo covers all edits within a session and is not kept across restarts.
- **Outside file changes are handled like VS Code**: the app watches the project folder; if a file changes on disk (hand edit, git checkout) and there are no unsaved changes, it reloads silently; if there are, it asks which to keep.
- **Instrument settings open as an instrument tab** (e.g. `Sampler: Drums`), opened by clicking the instrument name on a channel-rack row — consistent with "built-in instruments open as tabs, never floating windows" from the conversation. The tab holds the sampler sections described under "Sampler", plus volume and pan. The right dock stays empty.
- **Default keybindings follow VS Code conventions** (⌘⇧P palette, ⌘P quick open, ⌘W close tab, ⌘B toggle sidebar, ⌘J toggle bottom panel), with Space for play/stop. All rebindable via a keymap file. Shortcuts are context-aware (same key may act differently in the piano roll vs the pattern tab).

## Left to the builder

- GPUI version to pin, and how to structure the component layer around it (concept outline: keep UI code behind your own components; use Zed's source as reference).
- Audio graph: custom vs Firewheel. The first version's graph is small (N samplers, each voice with its own filter and modulation → pan/volume → output), so a simple custom graph is reasonable.
- Exact piano roll interactions beyond the basics above, zoom behaviour, grid sizes.
- Visual styling, within "calm, VS Code/Zed-like, dark by default".
- Test strategy and real-time stress testing approach.
