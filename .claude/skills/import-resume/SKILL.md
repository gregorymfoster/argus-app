---
name: import-resume
description: TLA+ model of an Import that copies originals into the Library, killed at any step and resumed (copy through a temp file, rename, Photo rows committed every 500 ms). Use before and after changing library/import.rs copy_verified, its commit loop, or the import resume path, and to read a broken rule from `scripts/formal`.
---

# Import resume, model-checked

`formal/ImportRun.tla` models an Import with Copy on (originals into `originals/<Shoot>/`), killed up to twice and resumed the way `resume_run` does: same Shoot, same copy folder, temporary files cleared, every file of the card walked again, files whose hash has a committed Photo row skipped. Photo rows go into an open transaction committed every `FLUSH_EVERY` (500 ms), so a kill loses the uncommitted ones while their copies stay. Created 2026-11-06 under the routine in `docs/formal.md`.

## Run it

```sh
scripts/formal                                         # every model; ~3 s
scripts/formal --trace ImportRun-copy OneCopyEach      # shortest path to a broken rule
scripts/formal --mutations                             # every rule catches its bug
scripts/formal --stale                                 # covered code changed since the sync commit
```

## The rules

Config `ImportRun-copy`: 2 files, names up to `-3`, up to 2 kills.

| Rule | In words | Harm if broken |
| --- | --- | --- |
| `OneCopyEach` | Each original is copied into the Library once | DSC1.ARW and DSC1-2.ARW, twice the disk |
| `RowHasCopy` | A saved Photo points at a copy that is there | A Photo with no original |
| `AllImportedWhenDone` | A finished Import brought in every file | Silent gaps |
| `NoStrayCopyWhenDone` | No copy the Library doesn't know is left | Orphan originals eating disk |
| `NoTmpWhenDone` | No `.argus-tmp` left | Hidden junk |

## Finding, fixed 2026-11-06

A kill after a copy was renamed into place but before its row was committed (a window of up to 500 ms per batch) left the copy with no row; the resumed Import copied it again as `-2` and the first copy stayed, unreferenced. Reproduced by `resume::an_import_killed_after_a_copy_but_before_its_row_was_saved_does_not_copy_it_twice`. Fix: `copy_verified` reuses a file already under the name (or its `-N` names) with the same size and content hash. The old behaviour is a mutation in `formal/ImportRun.mutations`.

## When you change the covered code

Same steps as the `export-resume` skill: `--stale`, mirror the change in the model (one action = one syscall or one statement; a transaction commit is its own step), `scripts/formal`, a mutation for each new rule, then move `sync:`.

## What it does not cover

Imports without Copy (rows only); the card going away mid-Import (the `card` pause); two Imports into one Shoot at once; RAW+JPEG pairs (the JPEG travels with its RAW the same way); the same content twice on one card (deduped by hash in `seen`); previews.

## Coverage record

model: ImportRun
sync: a395db30f6e0386949e645bd15d7e720ac532528

- covers: engine/src/library/import.rs
- covers: engine/src/tools/library.rs

Functions: `library::import::{copy_verified, clear_tmp, originals_dir}`, `Library::import` (the commit loop and resume), `tools::library` `import_folder` with `resume_run`.
