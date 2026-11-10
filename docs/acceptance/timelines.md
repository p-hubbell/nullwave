# Timelines acceptance

The growing-a-hook scenario from `docs/superpowers/specs/2026-11-09-nullwave-timelines-design.md`,
walked through in the release build (`cargo build --release -p nullwave`) on macOS.

- Date: 2026-11-09
- Build: `timelines` @ 82a5300
- `cargo test --workspace`: pass (315 tests)
- `cargo build --release -p nullwave`: pass

## Scenario

| # | Step | Result | Notes |
|---|------|--------|-------|
| 1 | Take a hook pattern | Pass | |
| 2 | Duplicate Pattern and rework the copy into another section | Pass | |
| 3 | Repeat for a third section | Pass | |
| 4 | Lay the sections out in the song, not hook first | Pass | |
| 5 | Add a producer tag at the start and a loose clip that drops in now and then | Pass | |
| 6 | Edit a section from inside the song | Pass | |
| 7 | Play the whole beat from start to finish | Pass | |
| 8 | Save, quit, relaunch: song, tabs and roll heights return | Pass | |

## Anti-goals

| Check | Result | Notes |
|-------|--------|-------|
| No surprise edits: numbers, outlines and the roll header make links obvious | Pass | |
| No tab juggling: editing works in place in the song and pattern tabs | Pass | |
| Growing a section is one command that leaves you in the copy | Pass | |
| An old project opens and sounds the same | Pass | |

## Verdict

All scenario steps and anti-goal checks passed, as reported by the user after the
walkthrough. No failures to follow up.
