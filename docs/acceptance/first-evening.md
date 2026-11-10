# First-evening acceptance

The success scenario from the spec, walked through in the release build
(`cargo build --release -p nullwave`) on macOS.

- Date: 2026-11-09
- Build: `first-version` @ 35e5b9a
- `cargo test --workspace`: pass
- `cargo build --release -p nullwave`: pass

## Scenario

| # | Step | Result | Notes |
|---|------|--------|-------|
| 1 | New project | Pass | |
| 2 | Add a sample folder (USB drive or any folder) | Pass | |
| 3 | Preview samples | Pass | |
| 4 | Drag a kick, a snare and a hat into the rack | Pass | |
| 5 | Make 4 patterns with notes on 2–3 instruments each | Pass | |
| 6 | Play and loop each pattern; change tempo while playing | Pass | |
| 7 | Adjust volume, pan and release | Pass | |
| 8 | Sampler: trim a sample, loop a pad, filter-envelope pluck on a bass, closed hat chokes open hat | Pass | |
| 9 | Save, quit, relaunch: project, tabs and panel sizes return exactly | Pass | |
| 10 | No floating window appears at any point | Pass | |

## Anti-goals

| Check | Result | Notes |
|-------|--------|-------|
| Windows shown by the OS during the walkthrough: one, plus native file pickers only | Pass | |

## Verdict

All scenario steps and the anti-goal check passed, as reported by the user after the
walkthrough. No failures to follow up.
