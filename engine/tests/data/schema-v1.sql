-- The Library schema as v1 shipped it (2026-10-17), for upgrade tests. Never edit.

CREATE TABLE shoots (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  source      TEXT NOT NULL,          -- folder or card the Import read from
  imported_at TEXT NOT NULL
);

CREATE TABLE photos (
  id          INTEGER PRIMARY KEY,
  shoot_id    INTEGER NOT NULL REFERENCES shoots(id),
  path        TEXT NOT NULL,          -- original, read-only, may be offline
  file_name   TEXT NOT NULL,
  hash        TEXT NOT NULL UNIQUE,   -- blake3 of the original's bytes (dedupe)
  bytes       INTEGER NOT NULL,
  kind        TEXT NOT NULL,          -- 'raw' | 'jpeg'
  captured_at TEXT NOT NULL,          -- ISO 8601 local time, EXIF or file mtime
  width       INTEGER NOT NULL,       -- upright, full size
  height      INTEGER NOT NULL,
  camera      TEXT NOT NULL,
  preview     TEXT NOT NULL,          -- path relative to the Library folder
  imported_at TEXT NOT NULL
);
CREATE INDEX photos_by_shoot ON photos(shoot_id, captured_at);

-- A batch groups changes made together (one AI cull, one Look apply) for batch undo.
CREATE TABLE batches (
  id      INTEGER PRIMARY KEY,
  author  TEXT NOT NULL,              -- 'person' | 'ai'
  label   TEXT NOT NULL,
  at      TEXT NOT NULL
);

-- Current Verdict per Photo. No row means Unflagged by nobody.
CREATE TABLE verdicts (
  photo_id INTEGER PRIMARY KEY REFERENCES photos(id),
  verdict  TEXT NOT NULL,             -- 'pick' | 'reject' | 'unflagged'
  author   TEXT NOT NULL,
  reason   TEXT,
  batch_id INTEGER REFERENCES batches(id),
  at       TEXT NOT NULL
);

-- Append-only history of every Verdict change, for undo and batch undo.
CREATE TABLE verdict_log (
  id       INTEGER PRIMARY KEY,
  photo_id INTEGER NOT NULL REFERENCES photos(id),
  verdict  TEXT NOT NULL,
  author   TEXT NOT NULL,
  reason   TEXT,
  batch_id INTEGER REFERENCES batches(id),
  at       TEXT NOT NULL
);

-- A person changing something the AI decided. Kept from day one to learn taste later.
CREATE TABLE overrides (
  id          INTEGER PRIMARY KEY,
  photo_id    INTEGER NOT NULL REFERENCES photos(id),
  field       TEXT NOT NULL,          -- 'verdict' or a Recipe path like 'exposure'
  ai_value    TEXT NOT NULL,
  person_value TEXT NOT NULL,
  at          TEXT NOT NULL
);

-- Recipe Versions: append-only, the highest version is current.
CREATE TABLE recipe_versions (
  photo_id INTEGER NOT NULL REFERENCES photos(id),
  version  INTEGER NOT NULL,
  recipe   TEXT NOT NULL,             -- JSON
  author   TEXT NOT NULL,
  batch_id INTEGER REFERENCES batches(id),
  at       TEXT NOT NULL,
  PRIMARY KEY (photo_id, version)
);

-- Shoot-wide structures (Shoot context). Filled in by later milestones.
CREATE TABLE stacks (
  id       INTEGER PRIMARY KEY,
  shoot_id INTEGER NOT NULL REFERENCES shoots(id)
);
CREATE TABLE stack_members (
  stack_id INTEGER NOT NULL REFERENCES stacks(id),
  photo_id INTEGER NOT NULL REFERENCES photos(id),
  rank     INTEGER NOT NULL,
  PRIMARY KEY (stack_id, photo_id)
);
CREATE TABLE embeddings (
  photo_id INTEGER NOT NULL REFERENCES photos(id),
  model    TEXT NOT NULL,
  vector   BLOB NOT NULL,             -- little-endian f32
  PRIMARY KEY (photo_id, model)
);
-- A Shoot's Looks, append-only: each row is one version of the Look named in its JSON
-- `id`; the newest row for an id is current. Undo of a batch deletes that batch's rows.
CREATE TABLE looks (
  id       INTEGER PRIMARY KEY,
  shoot_id INTEGER NOT NULL REFERENCES shoots(id),
  look     TEXT NOT NULL,             -- JSON {id, name, adjust}
  at       TEXT NOT NULL
);
