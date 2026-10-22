---
name: verify
description: Prove an Argus change works without Greg. Maps each feature to the narrowest check that drives it for real, with how long it takes and where it runs. Use before calling any change done.
---

# Verify a change

ADR 0005: every change is checkable by Claude without a human. Pick the narrowest row below, run it while iterating, then `just land` once (it runs `just check`). Look at any PNG a check writes before saying a visual change worked. Long runs go through `scripts/progress`; check them with `just status`. Run cargo from the repo root without `-p argus-engine` (the unit tests are `cargo test --profile fast --lib <name>`): the engine alone resolves fewer dependency features than the workspace, so `-p` rebuilds ~90 crates, 4-5 min cold.

## Pick the check

| Changed | Narrowest proof | Time (warm) | Runs |
| --- | --- | --- | --- |
| Engine logic, a tool, a command | `cargo test --profile fast --test engine <module>::<name>` (modules in `engine/tests/`) | 5-20 s | both |
| Pipeline, colour, a slider's look | `just render <raw> --set k=v`, then open `out/<stem>.jpg`; goldens via `--test engine golden::` | 2-10 s | both |
| Deliberate pipeline change | `bless-goldens` skill | 1 min | both |
| A Mask | `argus tool view_mask '{...}' --library L --out m.jpg`, `--test engine masks::` | 10 s | both |
| Library schema, writes, undo | `--test engine library_safety::`, `undo_looks::` | 20 s | both |
| AI cull or Agent plumbing | `live.mjs` (mock model) or `argus agent ... --provider claude-code` | 3-5 min | cloud |
| AI quality | `just bench --no-model`, `bench/score.py --only edit --case ID` | 1-10 min | cloud, Mac at night |
| A screen, a key, a flow | `just live <lib> --only <steps>` (real engine; steps: grid, flags, zoom, cancel, cull, auto, export, agent, sliders, suggest, masks, look, rename, remove, views, keywords, search; `STEPS` in `prototype/scripts/live.mjs`); all steps once before landing | 10-25 s a step, 80 s all | both |
| View › Zoom, pinch, Edit › Undo routing | `just live <lib> --only zoom,flags`: dispatches `argus:zoom` / `argus:edit-menu`, the same path the Tauri menu takes, and synthetic pinch (ctrl+wheel, WebKit gesture events) | 20 s | both |
| How any screen looks (unintended visual change) | `just shots [name]`: dummy-data screens diffed against blessed baselines, diffs in `prototype/out/shots/diff/`; `just shots-bless [names]` after an intended change | 3 min all | cloud |
| The native menu bar itself | `just mac-menus`: launches Argus.app on a throwaway Library, checks menus and shortcuts, clicks Edit › Undo/Redo, Zoom, Settings | 1 min | Mac thread (Accessibility granted once) |
| Bars, labels, window widths | `just layout`; Import: `just import-check` | 1-2 min | both |
| Design tokens, stray styles | `npm run design-lint` (in `just check`) | 2 s | both |
| Random-input crashes | `just fuzz <lib> [seed] [steps]` | 2-5 min | both |
| Idle cost, memory | `just perf` | 5 min | cloud |
| Speed of decode, import, edits | `just speed` | 2-4 min | both |
| The packaged app binary | `just app-e2e-quick` while iterating, `just app-e2e` once before landing UI | 2 / 6 min | cloud (Xvfb) |
| On Greg's real Mac and WebKit | `just mac-e2e` (background priority) | ~10 min | Mac thread only |

## Still needs Greg's eyes

Only what no script reaches: the macOS folder picker, window chrome and traffic lights, first-launch Gatekeeper, and whether a screen looks good (the shot diff only says it changed). Batch these into one screen-control request that says why (CLAUDE.md).

## When a feature has no row

Add the check first (a test at the seam, a `live.mjs` step, or a window event that mirrors a native-only input as `argus:zoom` does), then add its row here. A feature with no scripted proof is a gap to close, not a reason to ask Greg.
