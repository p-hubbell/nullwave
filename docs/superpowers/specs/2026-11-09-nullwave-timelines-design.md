# Nullwave — Timelines: patterns as sections, and the song

Date: 2026-11-09
Status: draft for review
Builds on: `docs/superpowers/specs/2026-11-02-nullwave-design.md` (the first-version spec). Where the two disagree, this document wins. In particular it replaces the first spec's "Instruments, patterns and notes (FL's model)" and "Channel rack" sections, and moves "Song arrangement timeline" and "audio clips" off its non-goals list.

## Intent and why

In FL the user builds a good hook first, with every instrument playing at once in one pattern. Then they get stuck. They keep tweaking the loop and can't grow it into a whole beat: an 8-bar pattern gives them no way to see or build what comes before and after it.

Nullwave should make it natural to grow a hook into a full beat, one section at a time. The way the user works:

1. Build the hook as a pattern.
2. Copy that pattern and rework the copy into another section, such as a verse.
3. Lay the sections out in the song, in any order, along with anything else the song needs: a producer tag at the start, a sound that drops in now and then.

To support that, a pattern stops being a short loop with one note list per instrument. It becomes a **timeline of one section**: one lane per instrument, with clips placed along it at any length. The song is **the same kind of timeline for the whole track**, holding patterns, loose clips and audio.

## Success

The user takes a hook they built and, in one evening, grows it into a full beat: several sections made by copying and reworking, laid out in the song with a producer tag at the start, and heard from start to finish. Nothing they changed affected something they didn't mean to change.

## The three levels

- **Clip.** A run of notes for one instrument, with a length. A clip is shown as a block with a number, such as guitar 1 or bell 2.
- **Pattern.** A section of the song. It is a timeline with one lane per instrument, and clips placed along it.
- **Song.** The whole track. It is a timeline with the same instrument lanes, plus audio lanes. It holds placed patterns, loose clips and audio clips.

## What the user decided

### Clips

- Inside one pattern, **blocks with the same number are the same clip**. If guitar 1 appears twice in a pattern and you edit one, both change.
- **A new number is a variant.** Bell 2 is a different clip from bell 1.
- Clip numbers count per instrument within one pattern (and separately within the song, for loose clips). Pattern 1's bell 1 and pattern 2's bell 1 are different clips.

### Patterns

- A pattern tab shows the pattern as a timeline: one lane per instrument, numbered clips placed along it. Sections are as long as they need to be; nothing caps them at 8 bars.
- **Clicking a clip opens its notes in a piano roll under the timeline, inside the same tab** (the user's choice, "A"). The whole section stays in view while the user edits.
- **The channel rack grid goes away.** The pattern tab replaces it.
- **New patterns get plain names**: Pattern 1, Pattern 2 and so on. They become "Hook" or "Verse" only if the user renames them. Nothing assumes an order of sections.
- **Copying a pattern makes an independent copy.** The copy gets its own clips: they start out identical and keep the same numbers, but changing a bass note in the copy leaves the original alone.
- The lanes are the project's instruments, **shared by the whole song**. The guitar in one pattern is the same sampler as the guitar in another.

### The song

- The song tab is a timeline with **one lane per instrument, just like the pattern tab**, only longer.
- The user places **patterns anywhere along it, in any order**.
- **A placed pattern drops its clips into each instrument's lane, wrapped in an outline** that shows which pattern they came from. There is no separate strip of pattern blocks.
- **A pattern is the same thing wherever it is placed.** Editing a placed pattern, in the song or in its own tab, changes every place it appears.
- The song can also hold **loose MIDI clips** (notes for one instrument that aren't part of any pattern, like a sound that drops in now and then) and **audio clips** (like the user's producer tag).
- **Audio isn't an instrument, so an audio clip gets its own audio lane.**

### Accepted in playback (my guesses, confirmed by the user)

- **Making clips:** clicking an empty spot in a lane creates a new clip for that instrument with the next free number, and opens it in the piano roll.
- **Moving and sizing:** dragging a clip moves it; dragging its edge changes its length.
- **Repeating:** ⌥-drag or ⌘D duplicates a clip as *the same clip* (same number). A "Make unique" command turns one copy into a new variant with the next free number.
- **Pattern length** is the end of its last clip, rounded up to a whole bar, minimum one bar. A section grows by placing clips further along; there's no length to set.
- **Playing a pattern:** Space loops the whole pattern, and the piano roll's playhead follows inside the selected clip.
- **Sidebar:** lists patterns by name and the instruments. Adding an instrument adds a lane to every pattern and the song, empty until clips are placed.
- **Duplicate pattern** is one command. It makes the independent copy and opens it.
- **Editing in the song:** clicking a clip inside a pattern's outline opens the piano roll under the song timeline, and changes go into the pattern. Dragging the outline moves the whole placed pattern. A single clip can't be pulled out of a placed pattern in the song; that's done in the pattern itself.
- **Full editing inside an outline (decided after review).** Everything you can do to clips in the pattern tab, you can do inside the pattern's outline in the song, and every edit goes into the pattern, so all of its placements change together. Clicking an empty spot inside the outline adds a new clip to the pattern. Dragging, resizing, ⌥-drag/⌘D, Make unique and delete all change the pattern. A clip dragged inside an outline stays inside it: it moves within the pattern and can't leave the outline. If a drag would put it before the pattern's start, it stops at the start; dragging past the end makes the pattern longer.
- **Loose clips:** clicking an empty spot in a song lane creates a loose clip for that instrument. Loose clips are numbered within the song the same way, so repeats of song bell 3 stay linked.
- **Audio clips:** dragging a file from the sample browser into the song gives it an audio lane. It plays at its own speed, not tuned or stretched. In this first pass it can only be moved and have its volume set.
- **Overlaps:** if two things overlap on the same lane, both play.
- **Playing the song:** Space in the song tab plays from the playhead. The song's length runs to the end of the last thing placed.
- **Old projects:** each existing pattern turns into a pattern with clip 1 at bar 1 for every instrument that had notes. The song starts empty.

## Goals

- Pattern tabs as section timelines, with the clip piano roll underneath.
- A song tab that places patterns, loose clips and audio clips on instrument and audio lanes.
- Linked clips (same number) and linked patterns (every placement) that always look linked.
- One command to copy a section and start reworking it.
- Older projects open and sound the same.

## Non-goals (this round)

- Audio clips inside patterns. Audio lives in the song only.
- Trimming, fading, tuning, stretching or reversing audio clips. Move and volume only.
- Pulling a single clip out of a placed pattern in the song, or turning a placed pattern into loose clips.
- Automation, tempo or time-signature changes over the song, markers.
- A song loop range. (Space plays from the playhead to the end.)
- WAV export. It is the next roadmap item and fits naturally after this.
- Mixer, buses, effects, plugins, MIDI input — unchanged from the first spec's roadmap.

## Anti-goals (failure even if it technically works)

The user agreed to these:

1. **Surprise edits.** Changing a note changes something the user didn't expect, such as the hook when they meant to change only the verse. Numbers and outlines must always make it obvious what is linked to what.
2. **Back to tab juggling.** Bouncing between the song tab and pattern tabs just to tweak things. Both tabs allow editing in place, with the piano roll underneath.
3. **Growing a section feels like chores.** Turning the hook into a verse starts with one command that leaves the user inside the new copy, not with re-placing clips by hand.
4. **Losing what already works.** Today's patterns, sounds and saved projects open and sound the same after the change.

The first spec's anti-goals still apply: no floating windows, opening something already open focuses its tab, the workspace is restored exactly, nothing splits or rearranges on its own.

## Constraints

- Everything from the first spec stands: the Rust stack, GPUI, one window, macOS first, real-time rules on the audio thread, explicit saving, crash recovery, outside-change detection, every action a named command, every edit one undo step.
- The project stays a folder of human-readable RON files that diff well in git: one file per clip's notes, so changing a clip touches one small file.

## Calls I made (the user can override)

- **Clips have their own length.** Dragging a clip's edge changes the clip, so every repeat of it changes length too. Notes past the clip's end are kept but don't play, and the piano roll shows them dimmed. A new clip starts one bar long.
- **Notes stop at the clip's end.** A note that starts inside a clip but runs past its end is cut at the end (its release still rings out, as when any note ends). What plays is what the block shows, and a repeat of the clip never collides with a held note from the previous one.
- **Both clip edges can be dragged.** The right edge changes the length. The left edge trims the start like FL: the notes stay where they are in time, and notes before the new start are hidden and silent. That means a clip has a start offset as well as a length, both shared by every repeat of the clip. The piano roll shows the whole clip with the hidden parts dimmed.
- **Linked things say so.** Each clip block shows its number. While a clip is open in the piano roll, every other block of the same clip is highlighted, and the piano roll's header names it in full with a count, e.g. `Pattern 1 › bell 2 · used 3×`. When the clip belongs to a pattern placed in the song, the header says so: `Pattern 1 › bell 2 · 3× in pattern, pattern placed 2× in song`. This is my answer to anti-goal 1.
- **Unused clips disappear.** Deleting the last block of a clip deletes the clip. Undo brings it back.
- **Make unique takes the next free number** for that instrument in that pattern (or in the song).
- **Duplicate pattern** names the copy with the next free "Pattern N", puts it at the end of the sidebar list, and opens it in a new tab. It does not place it in the song.
- **Pattern tab and song tab share one timeline editor.** The song version adds pattern outlines and audio lanes. Lanes follow the project's instrument order; audio lanes come after the instrument lanes, in the order they were added.
- **One song per project**, listed at the top of the sidebar's project section as "Song". It opens as a tab like anything else.
- **Placing patterns in the song:** drag a pattern from the sidebar onto the song timeline. ⌘D on a selected placed pattern repeats it right after itself.
- **Loose clips win over outlines.** Loose clips are drawn on top of pattern outlines, and a click on a loose clip goes to the loose clip. To make a new loose clip where a pattern is placed, draw it outside the outline and drag it in.
- **The outline handle.** Because clicks inside an outline edit the pattern, the outline is moved by its label tab at the top-left (the pattern name), not by clicking anywhere inside it.
- **The outline** is drawn around the pattern's whole length (start to its rounded-up end) across all instrument lanes, labelled with the pattern name at its top-left. Instrument lanes a pattern doesn't use stay empty inside the outline.
- **Audio lane:** one lane per audio clip file, named after the file, with a volume control on the lane header. Dropping the same file again reuses its lane. Missing audio files behave like missing samples today: the clip stays, plays silent, and shows a "missing" marker.
- **Snapping:** placements snap to the bar by default; holding ⌘ while dragging disables snapping; the snap size is a menu on the timeline, the same as the piano roll's.
- **Playing:** Space plays the tab that has focus: a pattern tab loops its pattern, the song tab plays once from the playhead to the end and stops. Clicking the ruler moves the playhead. The status bar shows "Song" or the pattern's name as what's playing.
- **The piano roll tab type goes away.** A clip's notes are always edited in the piano roll under the pattern or song timeline. The velocity lane stays in the bottom panel and follows the piano roll that's open.
- **The piano roll under the timeline** shows only the open clip, starting at the clip's first beat, with the clip's length marked. Its height is adjustable with a drag handle and remembered with the workspace. Closing it (Escape or a close button) gives the timeline the full height.
- **Adding instruments:** an "+ add instrument" row sits under the last instrument lane in both tabs. Dragging a sample onto it makes a new instrument, and dragging a sample onto an instrument's lane header replaces its sample, the same as on channel-rack rows today. Clicking a lane header's name opens the instrument tab. The missing-sample marker moves to the lane header.
- **Old projects:** each old pattern's per-instrument notes become clip 1 for that instrument in that pattern, placed at bar 1, with the clip length equal to the old pattern's length. The old files are read, and saving writes the new layout and removes the old files (save already cleans up files it no longer owns). Projects saved by this version can't be opened by the first version; that's acceptable.

## Left to the builder

- The exact file layout, within the constraints above. A reasonable shape:
  ```
  my-song/
    song.ron                     # tempo, time signature, instrument order, pattern order, audio lanes
    instruments/drums.ron
    patterns/pattern-1/
      pattern.ron                # name, clip lengths, and placements: (instrument, clip number, start)
      clips/drums-1.ron          # notes of drums clip 1
    song/
      arrangement.ron            # placed patterns, loose-clip placements, audio placements
      clips/bell-3.ron           # notes of loose song clips
  ```
- How the engine plays the new structure. The engine today plays a flat, sorted list of note events; flattening placements into that list off the audio thread looks sufficient. Audio clips need a voice that plays a file at its own speed from a given tick; a sampler voice at the file's root pitch with no envelope beyond a declick would do. The song needs a "play once and stop at the end" mode next to the pattern's loop.
- How the old in-memory types change (`Pattern`, tabs, the `Edit` variants, recovery and outside-change handling for the new files).
- Clip, outline and lane visuals, zoom, scrolling and selection details (multi-select, marquee), within "calm, VS Code/Zed-like, dark".
- Exact keyboard shortcuts beyond the ones named, following the first spec's conventions.
