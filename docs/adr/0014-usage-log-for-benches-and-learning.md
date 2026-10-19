# 0014. An append-only usage log in the Library feeds benches and learning

Status: accepted (2026-10-18). Greg asked for Argus to record his real use as training and benchmark data: the before, the AI's guess, and what he exported.

## Context

The benches in `fixtures/bench/` come from Greg's old Lightroom catalog. They are noisy and frozen, and they never see what Argus's own AI proposed. Everything Argus needs to do better is already passing through the Library while he works: the Recipe a Photo starts with, each Suggestion, Agent edit and Auto, which take he accepts or dismisses, his tweaks, his flags and stars, and the Recipe he finally exports. But the Library keeps only current state for most of it. Suggestions are replaced when new takes arrive, Undo appends new Versions and puts Verdicts back, and nothing records an export at all.

## Decision

The Library gets one append-only table, `usage_events(photo_id, kind, data JSON, at)` (schema v8). Writes at the Library's edges append to it: `write_recipe`, `set_verdict`, `set_rating`, `replace_suggestions`, accepting and dismissing a Suggestion, the Agent's request, and `export_photos`. A Photo's first event is preceded by a `start` that holds the Recipe it had before anything changed. Rows are never updated. Undo, new edits and fresh Suggestions add rows, and only `clear_usage` deletes.

Recording is on by default and lives under Settings › General ("Record my edits to improve Argus"), with counts, Export and Clear. The log stays inside the Library folder (so it moves, backs up and restores with it) and is never uploaded. Logging is best-effort: a failed log write prints a warning and never fails the edit it describes.

Each AI take carries its Provider, model, the job's kind and arguments (Compute level, count, reference), the Argus version and seconds taken. The Argus version stands in for the prompt and loop version, so a change to the editing loop that should be told apart in the data ships with a version bump.

`engine::usage` folds a Photo's events into a History (start, takes with their fate, the kept take, the final Recipe, the sliders moved since the kept take, the AI's first Verdict and stars against the person's last) and exports the log as bench cases in `fixtures/bench`'s own layout. `bench/score.py --bench <folder>` runs every existing bench on it, and `--logged` scores the takes Greg actually saw.

## Consequences

Benches can run on Greg's real Shoots without a new scorer, and a later "learn my edits" feature has clean pairs of (start, AI take, kept, final) to learn from. The format is documented in `docs/usage-log.md`; new event kinds may be added, but existing fields keep their meaning.

The kept take is inferred, not recorded: of the takes that were applied, the one nearest the exported Recipe wins, if it is nearer than the start. That survives Undo and switching between takes, at the cost of naming a take Greg may have only passed through.

The log grows by a few hundred bytes per event, with the Recipe JSON being most of it. A heavy year is a few tens of megabytes. If that matters, a later step can drop `recipe` events between exports.

Removing a Photo keeps its events (no foreign key), so the log outlives the Library's own rows. Clear is the only way to delete them.
