# 2. SQLite library with JSON recipes inside it

Status: accepted (2026-10-17). Where originals live is amended by ADR 0011 (one portable folder; card imports copy into it).

## Context

Edits must be non-destructive and easy for an AI to read, write and diff. The app also has to query hundreds of photos quickly: cull state, scores, faces, and filters.

## Decision

There is one Library for all of Greg's photos (Greg, 2026-10-17: one big Library over one per shoot), stored as a single SQLite file. It references originals wherever they sit and keeps its own previews, so it works when a drive is unplugged. Imports are grouped as Shoots. Photos, cull state, ML scores and face boxes are ordinary tables. Each photo's Recipe is a versioned JSON document stored in a column, and every change appends a new version, which gives us undo and history for free. Originals are never written to.

Argus can write a Recipe out as a sidecar file next to the original (for backup, moving folders, or diffing in git), but the sidecar is a copy. The Library is the source of truth.

## Why

Sidecars as the source of truth would mean two copies that drift apart, and slow scans of folders on every query. SQLite gives one file, fast queries and transactions, while JSON keeps edits readable through the same tools the AI uses.

## Considered and rejected

Sidecar files only: portable, but slow to query and fragile when the app and the filesystem disagree.

Fully relational edit settings (a column per slider): fast, but every new tool needs a migration, and the AI can no longer read an edit as one document.
