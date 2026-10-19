# 9. Keeping edits safe: durable commits, ordered upgrades, backups inside the Library

Status: accepted (2026-10-18)

## Context

Every Verdict, Recipe Version and Look lives in one SQLite file (ADR 0002). Losing it means losing a day of culling and editing, which is the one thing a photo app must never do. Several processes write it at once: the app, `argus serve`, the MCP server and each tool call open their own connection. The app can be killed, the Mac can lose power, and every release may change the schema. Greg also wants the Library portable: he picks its folder and may move it to another disk.

## Decision

Every connection uses WAL with `synchronous = FULL`, `foreign_keys` on and a 10 s busy timeout. FULL costs one extra fsync per commit, which is nothing next to rendering a RAW, and it makes a commit durable rather than only crash-consistent. The busy timeout makes a second writer wait instead of failing with "database is locked". Writes that read before writing (the next Recipe Version number, a batch undo, flags, Stacks) run in `BEGIN IMMEDIATE` transactions, so two processes can't both claim a number and a crash leaves a batch undo either whole or not started.

Schema changes are an ordered list of steps in `engine/src/library/migrate.rs`. Each step is Rust (so a step can rewrite paths, not only run SQL) and runs in its own transaction that also bumps `user_version`, re-reading the version under the write lock so two processes opening an old Library never both apply a step. A new Library gets `schema.sql` (always the latest) in one go, and a test checks that an upgraded Library ends up with exactly the same tables and columns as a new one. A Library from a newer Argus is refused, untouched.

Backups are whole copies of `library.db`, made with `VACUUM INTO` into a temporary name and then renamed, in the Library's own `backups/` folder so they move with it. The app and `argus serve` make one a day at start, keeping the newest 14. Before any schema upgrade a tagged copy (`_pre-vN`) is made, and before a restore another (`_before-restore`); tagged copies are never pruned. An integrity check (`PRAGMA quick_check`) runs before every backup, so a damaged database never pushes good backups out. `argus restore <name>` puts one back through SQLite's backup API, under its locks, so an open app sees the restored data rather than a torn file. Originals are never copied (they are never written), and previews aren't either (they can be rebuilt).

## Considered and rejected

Backups in a separate folder such as `~/Library/Application Support`: safer against losing the Library's disk, but they wouldn't travel with a portable Library and would point at the wrong photos after a move. Time Machine already covers the whole disk.

Copying `library.db` with the file system: a copy taken during a write can be torn, and the WAL file holds recent commits that a plain copy misses.

A migration framework crate: our steps are a few lines each and a list is easier to read than a dependency.

## Consequences

- Backups sit on the same disk as the Library, so they guard against bugs, bad batches and crashes, not a dead drive. Time Machine or a synced folder covers that.
- The kill test (`engine/tests/library_safety.rs`) SIGKILLs a writer at random points and checks every write it reported is there and nothing is torn. It covers process crashes; power loss rests on `synchronous = FULL` and SQLite's own guarantees.
- Import still holds one long transaction for a whole Shoot, so a crash mid-import leaves no half Shoot, but flags wait for the import to finish writing.
