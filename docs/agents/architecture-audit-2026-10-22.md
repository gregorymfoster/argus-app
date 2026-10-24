# Architecture health check, 2026-10-22

Greg asked whether Argus still follows Matt Pocock's principles (deep modules, small interfaces, tests at the seam, one shared language) now that many threads land features and benches in parallel. Audited main at da84e78.

## Verdict

Mostly yes, and the load-bearing parts are healthy. The pressure shows in two god files and one piece of logic that lives in both Rust and TypeScript.

What holds up well. Every caller (MCP, the in-app Agent, the app, `argus serve`) goes through one tool layer with one entry point, `Tools::call`, and `commands::dispatch` adds only the person's own commands on top. The UI's only door to the engine is `prototype/src/data/backend.ts`. The bench Python drives the engine through the `argus` CLI (`score.py`, `edit_head.py features` via `argus edit-fit`) instead of re-implementing the pipeline, so benches measure the shipped code. Specialist models sit behind the Analyzer seam (ADR 0013). The glossary has 60+ terms and the code uses them.

## Issues, ranked

1. **`engine/src/tools.rs` is a 3,600-line god file.** 79 tool specs (about 750 lines of JSON Schema in `specs()`) and their bodies share one file, so nearly every feature thread edits it and rebases through it. The interface is right (deep, one `call`); the file layout is not. Fix: split into `engine/src/tools/` by area (library, cull, edit, masks and spots, presets, export), each file holding its specs beside its bodies, `Tools::call` unchanged. Pure move, no behavior change.
2. **`engine/src/cull.rs` (2,200 lines) also owns the model Providers.** The Ollama client (streaming, stall watchdog, model choice), the Claude API client, `claude -p`, and API-key storage live in the cull module, so `naming`, `technical`, `usage` and `tools` import `cull::Provider` to talk to a model. That is a leaky boundary: the cull is one user of Providers, not their home. Fix: move them to `engine/src/providers/` and keep `cull.rs` for the Contact sheet, chunking and applying results.
3. **Presets are applied in two languages.** `prototype/src/domain/presets.ts` re-implements `engine/src/presets.rs` (Amounts, absolute keys, curves, undo by `recipe.presets`) and the UI uses it even on a live Library, so hover previews are instant. Nothing checks the two agree, and the prototype has no unit-test runner, so TS domain logic is checked only through screenshot and e2e scripts. Fix: a parity check that applies every built-in Preset at a few Amounts in both and diffs the Recipes.
4. **A few tools are tested only below the seam.** `stack_bursts`, `rerate_shoot` and `rank_presets` have unit tests on their modules but no call through `Tools` in `engine/tests/`. Low risk; add when touching them.
5. **Doc drift (fixed in this change).** Two ADRs were numbered 0010 (naming and the background Queue); the Queue one is now 0016 and its four references point there. Exemplar was a concept in code (`exemplars.rs`) but missing from `GLOSSARY.md`; added.

Worth watching: the Commands section of `CLAUDE.md` has grown to feature documentation that every session reads. Moving per-feature detail into `docs/` would keep the file short, as it asks of itself. Done 2026-10-22: the detail is now `docs/commands.md`.

## Sweep results (same day)

Greg then asked for a full clean-code sweep. Five agents split it by area and everything landed on main between a1c7aad and this note.

Fixed: issue 3 has a parity check now (`cd prototype && npm run preset-parity`, 513 of 513 cases agree) and issue 4 has seam tests. The bench shares `bench/common.py` instead of five copies of the CLI wrapper and four readers of the `facts` table. The engine lost a second calendar in `cull.rs`, two slug functions, a copied smoothstep and two functions only tests called, and `masks::smoothstep` no longer returns NaN on a zero-width edge. The prototype lost dead exports, a tabs component and 19 unused CSS classes, with `npm run dead-code` (knip) to keep it that way. CLAUDE.md went from 3,672 to 2,010 words, with the per-command detail moved verbatim to `docs/commands.md`, and docs/mcp.md now lists all 79 tools.

Build: warm `just check` with goldens went from 43 s to 34 s (test binaries run side by side through `scripts/test-parallel`, a lighter denoise unit test). `cargo test -p argus-engine` was rebuilding about 90 crates because `-p` resolves features differently; every doc and skill now runs tests from the repo root. Goldens had been skipping silently in projects sessions because the container resumes a snapshot taken before `/mnt/project-files` was mounted; `just check` now fetches the sample RAWs itself (`scripts/cloud-ensure`).

Still open, ranked:
1. Issues 1 and 2 (split `tools.rs`, move Providers out of `cull.rs`): done 2026-10-22 as pure moves, to `engine/src/tools/` (one file per area) and `engine/src/providers/`.
2. Python copies of engine rules that can drift silently: `bench/taste_head.py:42-70` (the `awkward` and `taste::score` rules), `bench/crs.py:21` (`TEMP_PER_MIRED`, the XMP mapping), and `edit_head.py` folds that differ from `splits.fold`.
3. The prototype has six ETA formatters and three byte formatters, each with different text. Merging them changes visible copy, so it needs a design call.
4. App-shell Tauri calls (`restart_to_update`, window and menu events) skip `backend.ts`; a small `data/shell.ts` would give them one seam.
5. The glossary says to avoid "stars" but the CLI uses `--min-stars` and `stars`.
