# Formal verification

Tests check the cases someone thought to write. A model checker checks every case in a stated scope: every order of steps, every kill point, every interleaving of two processes, up to small bounds. Argus uses TLA+ models checked by TLC for the parts where a bug costs the most and tests reach the least: work that must survive a kill, and state several threads or processes share. Started with pstack's `/create-formal-verification-skill` (Greg, 2026-11-06: "fix, and routinely formally verify the app").

## The models

| Model | Promise | Skill | Found |
| --- | --- | --- | --- |
| `formal/ExportRun.tla` | A killed Export finishes each file once, under its own name, never over the person's file, never half written | `export-resume` | 2 bugs (Original exported twice; empty file and `-2`), fixed |
| `formal/Queue.tla` | No job runs twice at once, one queued job per dedupe key, a cancel sticks, across the app and `argus work` | `queue` | 2 bugs (a second process ran the app's job again; the same work queued twice), fixed |
| `formal/ImportRun.tla` | A killed Import with Copy copies each original once and leaves no stray copies | `import-resume` | 1 bug (copy renamed in before its row was saved, copied again as `-2`), fixed |

Every finding was reproduced by a test in `engine/tests/resume.rs` before it was fixed, and the old behaviour stays in the model's `.mutations` file.

## The routine

- `just check` runs `scripts/formal` and `scripts/formal --mutations` (cached: they rerun only when `formal/` or the script changes, ~11 s together) and `scripts/formal --stale`, which names any model whose covered code changed since its sync commit.
- **Changing covered code** (each model's skill lists its files): follow that skill. Mirror the change in the model, run `scripts/formal`, move the skill's `sync:` commit once the model describes HEAD. A `--stale` line in `just check` means someone skipped this.
- **A new stateful feature** (anything that must survive a quit, crash or sleep; anything two threads or processes share; a multi-step protocol over files and the database) gets a model in the same change, made with pstack's `/create-formal-verification-skill`. The bar: the model's actions map one-to-one to statements in the code, each rule is one plain sentence with its harm, each rule has a mutation that breaks it, and the model plus its configs check in a few seconds. Add a row above.
- **Fixing a bug in modeled code**: check the fix in the model first (`formal/SPEC.whatif`, `scripts/formal --whatif`), then write the failing test, then fix the code. Keep the old behaviour as a mutation.
- **A finding you can't fix now**: list it in `formal/SPEC.findings` (`CFG RULE FINDING`); `scripts/formal` checks it still breaks and says when it's fixed.

## Files

- `formal/SPEC.tla`: the model; its header maps each action to the code.
- `formal/SPEC-CFG.cfg`: one config per scope (constants, rules under INVARIANTS).
- `formal/SPEC.mutations`: plausible bugs, each naming the rule that must catch it.
- `formal/SPEC.whatif`: proposed fixes, each naming the findings it clears.
- `formal/SPEC.findings`: rules the code breaks today.
- `scripts/formal`: the runner (TLC 1.7.4, downloaded once to `~/.cache/argus`, needs Java 11+; `brew install openjdk` on the Mac). `--trace CFG RULE` prints the shortest counterexample. pstack's `/statechart` draws a model's states as a chart.

## Ideas for the next models

Undo and Redo of batches (`library/undo.rs`: undo all = start, redo all = end, which `just fuzz` samples); Backup to a place that disconnects mid-copy (`library/backup.rs`); the Cull Round marks.
