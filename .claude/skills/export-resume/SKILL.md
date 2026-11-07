---
name: export-resume
description: TLA+ model of a resumable Export (claim a name, temp file, rename, resume after a quit, crash or closed lid, Stop). Use before and after changing export.rs claim/put, the run_items rows in library/runs.rs, export_photos' resume skip, or resume_export, and to read a broken rule from `scripts/formal`.
---

# Export resume, model-checked

`formal/ExportRun.tla` models one Export run with Argus killed at any step, any number of times up to a bound, and the person pressing Stop between Photos. TLC (the TLA+ model checker) tries every order of those steps and checks the rules below in every state. Tests in `engine/tests/resume.rs` kill at a few chosen points; the model kills at all of them.

Why TLA+: the question is "can a kill at some step leave the folder wrong", which is about the order of steps, not about one function's inputs. A model checker over a small model answers that exhaustively. Created with pstack's `/create-formal-verification-skill` (Greg, 2026-11-06).

## Run it

```sh
scripts/formal               # ~6 s: every rule holds, or is a known finding in formal/ExportRun.findings
scripts/formal --trace ExportRun-original OneCopy   # shortest path to a broken rule, file by file
scripts/formal --mutations   # ~3 s: each rule fails on the bug it guards (all must say DETECTED)
scripts/formal --whatif      # proposed fixes checked in the model (formal/ExportRun.whatif)
scripts/formal --stale       # covered code changed since the sync commit below?
```

`just check` runs `scripts/formal` (cached: it reruns only when `formal/` or the script changes) and `--stale`. It needs Java 11+ (`brew install openjdk` on the Mac; the cloud box has it); without Java it prints SKIPPED and passes. TLC 1.7.4 is downloaded once to `~/.cache/argus/` and checked by sha256. For a chart of the states, or to read a counterexample as a path in plain words, use pstack's `/statechart` skill on `formal/ExportRun.tla`.

## The rules

Configs: `ExportRun-jpeg.cfg` (one file per Photo: JPEG, HEIC, TIFF) and `ExportRun-original.cfg` (the RAW copy plus its `.xmp`). Both: 2 Photos, up to 2 kills, the person's own file already at Photo 1's name, names up to `-4`.

| Rule | In words | Harm if broken |
| --- | --- | --- |
| `ForeignKept` | A file the person already had at the export's name is never written over | Their file is lost |
| `NeverHalfWritten` | No name ever holds a half-written file | A broken JPEG that looks finished |
| `OneCopy` | Each Photo is in the folder once | DSC1.jpg and DSC1-2.jpg |
| `DoneIsWhole` | A Photo the run counts as done is whole on disk | A missing export reported as done |
| `AllExportedWhenDone` | A finished run exported every Photo | Silent gaps |
| `KeepsItsName` | A resumed Photo gets the name an uninterrupted run gives it | `-2` names after a crash |
| `NoTmpWhenDone`, `NoTmpWhenStopped` | No `.name.argus-tmp` left after the run ends or is stopped | Hidden junk, wasted disk |
| `NoEmptyNameWhenDone`, `NoEmptyNameWhenStopped` | No empty claimed name left behind | 0-byte files in the export folder |
| `NamesSuffice`, `TypeOK` | Sanity: the model's bounds are big enough, values well-typed | A modeling error |

## Findings, fixed 2026-11-06

`formal/ExportRun.findings` lists rules the current code breaks (none now). The model found two bugs, both reproduced in the real engine and fixed the same day; their old behaviour is kept as mutations in `formal/ExportRun.mutations`, and kill tests in `engine/tests/resume.rs` cover each.

- **F1, Original exports.** Killed after the RAW copy was renamed into place but before its `.xmp`: resume claimed a fresh name and exported the Photo twice (`-2`). Fix: `claim_again` reuses a *claimed* name whatever its files hold, since the run's row proves the name is the run's.
- **F2, every format.** Killed between `create_new` and `claim_run_item`: resume didn't know the empty file, left it, and wrote `-2`. Fix: `claim` records each name before trying it (`Library::intend_run_item`, `run_items.done = 2`). Resume reuses an *intended* name only while its files are missing or empty, and never counts a file at an intended name as done (it may be the person's own).

Residual risk the model leaves out: two Exports into the same folder at once, where one is killed after recording a name and before creating it, and the other claims that very name meanwhile. The resumed one then shares the name, and the later rename wins.

## When you change the covered code

1. `scripts/formal --stale` lists covered files changed since the sync commit. Read the diff against the action map in the header of `formal/ExportRun.tla`: each action is one indivisible step in the code (a syscall or one SQLite statement).
2. If the change alters a step, its order, or what resume decides, change the model the same way, then `scripts/formal`. Add a `--whatif` block first if you are trying a fix.
3. A rule that newly breaks: `scripts/formal --trace CFG RULE`. If the path is something the code can really do, it is a bug in the change; fix the code. If the path needs a step the code cannot take (two steps the code does atomically, a state it never writes), the model is wrong; fix the model, never weaken the rule.
4. A new rule needs a mutation in `formal/ExportRun.mutations` that breaks it on a config where it holds; `--mutations` must say DETECTED for all.
5. Once the model describes the code at HEAD, move `sync:` below to that commit.

## What it does not cover

Two Export runs side by side (see the residual risk above); disk full or other I/O errors inside `put` (its cleanup path); a kill inside `end_run`'s clearing loop (modeled as one step); filesystem durability of the rename itself; Imports (`import_folder`'s `resume_run`); the Queue's job claiming. Bounds: 2 Photos, 2 kills, 4 names; a pass covers those.

## Coverage record

model: ExportRun
sync: a395db30f6e0386949e645bd15d7e720ac532528

- covers: engine/src/export.rs
- covers: engine/src/library/runs.rs
- covers: engine/src/tools/exports.rs
- covers: engine/src/jobs/resume.rs

Functions: `export::{claim, claim_again, Slot, Reuse, put, tmp_of, finished, clear_unfinished, export_to}`, `Library::{intend_run_item, claim_run_item, finish_run_item, end_run, orphaned_runs}`, `tools::exports::export_photos` (the resume skip), `jobs::resume::{queue_orphaned_runs, resume_export}`.
