-- The Library schema, latest version (see migrate.rs for the history). docs/spec.md (Library), ADR 0002.
-- This is always the latest schema. For any change, also append a step in library/migrate.rs.

CREATE TABLE shoots (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  source      TEXT NOT NULL,          -- folder or card the Import read from
  imported_at TEXT NOT NULL
);

-- jpeg: the camera's JPEG of a RAW+JPEG frame, read-only. shutter, aperture, iso, focal,
-- lens: EXIF exposure settings (seconds, f-number, ISO, mm as marked, lens model), NULL
-- when not recorded. No comments beside these columns: SQLite's DROP COLUMN (used by the
-- upgrade tests) mangles them.
CREATE TABLE photos (
  id          INTEGER PRIMARY KEY,
  shoot_id    INTEGER NOT NULL REFERENCES shoots(id),
  path        TEXT NOT NULL,          -- original, read-only, may be offline: relative to the
                                      -- Library folder when copied into it, else absolute
  file_name   TEXT NOT NULL,
  hash        TEXT NOT NULL UNIQUE,   -- blake3 of the original's bytes (dedupe)
  bytes       INTEGER NOT NULL,
  kind        TEXT NOT NULL,          -- 'raw' | 'jpeg'
  captured_at TEXT NOT NULL,          -- ISO 8601 local time, EXIF or file mtime
  width       INTEGER NOT NULL,       -- upright, full size
  height      INTEGER NOT NULL,
  camera      TEXT NOT NULL,
  preview     TEXT NOT NULL,          -- path relative to the Library folder
  imported_at TEXT NOT NULL,
  jpeg        TEXT,
  shutter     REAL,
  aperture    REAL,
  iso         INTEGER,
  focal       REAL,
  lens        TEXT
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
  at       TEXT NOT NULL,
  batch_id INTEGER REFERENCES batches(id)
);
-- Per-Library settings (preview size and quality), JSON values by key.
CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- Current star Rating per Photo (Lightroom's 0-5 stars). No row means 0 stars by nobody.
CREATE TABLE ratings (
  photo_id INTEGER PRIMARY KEY REFERENCES photos(id),
  stars    INTEGER NOT NULL,          -- 0..5
  author   TEXT NOT NULL,
  batch_id INTEGER REFERENCES batches(id),
  at       TEXT NOT NULL
);
-- Append-only history of every Rating change, for undo and batch undo.
CREATE TABLE rating_log (
  id       INTEGER PRIMARY KEY,
  photo_id INTEGER NOT NULL REFERENCES photos(id),
  stars    INTEGER NOT NULL,
  author   TEXT NOT NULL,
  batch_id INTEGER REFERENCES batches(id),
  at       TEXT NOT NULL
);
-- The AI's Best Photos of a Shoot, 1 = best. Replaced by every cull, like Stacks.
CREATE TABLE best_photos (
  photo_id INTEGER PRIMARY KEY REFERENCES photos(id),
  shoot_id INTEGER NOT NULL REFERENCES shoots(id),
  rank     INTEGER NOT NULL
);

-- The background queue: work the AI does while the person is away (Suggestions, culls).
-- Highest priority first, then oldest. A job left `running` by a quit is queued again.
CREATE TABLE jobs (
  id          INTEGER PRIMARY KEY,
  kind        TEXT NOT NULL,          -- 'suggest' | 'suggest_shoot' | 'match_hero' | 'cull_shoot'
  args        TEXT NOT NULL,          -- JSON
  priority    INTEGER NOT NULL DEFAULT 0,
  dedupe_key  TEXT,                   -- one queued or running job per key
  label       TEXT NOT NULL,          -- for the person: "Suggestions for DSC08005"
  status      TEXT NOT NULL,          -- 'queued' | 'running' | 'done' | 'failed' | 'cancelled'
  done        INTEGER NOT NULL DEFAULT 0,
  total       INTEGER NOT NULL DEFAULT 0,
  result      TEXT,                   -- JSON
  error       TEXT,
  created_at  TEXT NOT NULL,
  started_at  TEXT,
  finished_at TEXT
);
CREATE INDEX jobs_by_status ON jobs(status, priority DESC, id);

-- Suggestions: whole Recipes the AI proposes for a Photo. They never change the Photo
-- until the person accepts one, which saves it as a new Recipe Version.
CREATE TABLE suggestions (
  id       INTEGER PRIMARY KEY,
  photo_id INTEGER NOT NULL REFERENCES photos(id),
  job_id   INTEGER REFERENCES jobs(id),
  reference_id INTEGER REFERENCES photos(id), -- matched to this Photo's edit ("apply to the rest")
  rank     INTEGER NOT NULL,          -- 1 = the AI's favourite
  title    TEXT NOT NULL,             -- "Bright and natural"
  note     TEXT NOT NULL,             -- why, in a sentence
  recipe   TEXT NOT NULL,             -- JSON, a whole Recipe
  preview  TEXT,                      -- path relative to the Library folder
  status   TEXT NOT NULL,             -- 'open' | 'accepted' | 'dismissed'
  at       TEXT NOT NULL
);
CREATE INDEX suggestions_by_photo ON suggestions(photo_id, status);

-- Facts: what the specialist models (Analyzers, ADR 0013) found in a Photo. A cache: a
-- Fact is stale when its Analyzer's version or the preview it was made from changed, and
-- clear_cache may drop them all. Large blobs live in cache/facts/ instead of here.
CREATE TABLE facts (
  photo_id    INTEGER NOT NULL REFERENCES photos(id),
  analyzer    TEXT NOT NULL,          -- 'signals' | 'faces' | 'eyes' ...
  version     INTEGER NOT NULL,       -- the Analyzer's version that made it
  input_hash  TEXT NOT NULL,          -- blake3 of the preview it was made from
  value       TEXT NOT NULL,          -- JSON
  blob        BLOB,                   -- planes or vectors, when small
  blob_file   TEXT,                   -- else this file, relative to the Library folder
  computed_at TEXT NOT NULL,
  PRIMARY KEY (photo_id, analyzer)
);

-- Usage log: what happened to each Photo, for benches and learning the person's edits
-- (ADR 0014, docs/usage-log.md). Append-only: Undo, new edits and Suggestion refreshes
-- never change a row; only clear_usage (Settings) deletes. No foreign key, so a Photo's
-- history survives Remove from Library.
CREATE TABLE usage_events (
  id       INTEGER PRIMARY KEY,
  photo_id INTEGER NOT NULL,
  kind     TEXT NOT NULL,             -- start | recipe | take | take_accepted | take_dismissed
                                      -- | agent | verdict | stars | export
  data     TEXT NOT NULL,             -- JSON, by kind (docs/usage-log.md)
  at       TEXT NOT NULL
);
CREATE INDEX usage_by_photo ON usage_events(photo_id, id);

-- Keywords and captions (docs/naming.md): hierarchical Keywords on each Photo, stored as
-- the full path ('Places|Portugal|Lisbon'), and one caption (IPTC Description). The AI's
-- stay 'suggested' until the person keeps them; a dismissed one stays as a row so the AI
-- doesn't offer it again. Written to XMP on export.
CREATE TABLE photo_keywords (
  photo_id INTEGER NOT NULL REFERENCES photos(id),
  keyword  TEXT NOT NULL,
  author   TEXT NOT NULL,             -- 'person' | 'ai'
  state    TEXT NOT NULL,             -- 'kept' | 'suggested' | 'dismissed'
  at       TEXT NOT NULL,
  PRIMARY KEY (photo_id, keyword)
);
CREATE INDEX keywords_by_name ON photo_keywords(keyword);
CREATE TABLE captions (
  photo_id INTEGER PRIMARY KEY REFERENCES photos(id),
  text     TEXT NOT NULL,
  author   TEXT NOT NULL,             -- 'person' | 'ai'
  state    TEXT NOT NULL,             -- 'kept' | 'suggested'
  at       TEXT NOT NULL
);

-- Albums: saved searches, like Lightroom's smart collections. `query` is JSON
-- ({"text": "autumn friends"}); the Photos are found again each time, so an Album keeps
-- up with Imports and new Keywords.
CREATE TABLE albums (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  query      TEXT NOT NULL,
  created_at TEXT NOT NULL
);

-- People (the People view): faces the person named. `face` is the face's number in the
-- Photo's `faces` Fact, left to right. A face with no person is one the person hid (not
-- someone to name); `face_not` remembers "not this person" so it isn't asked again. Naming
-- a face also keeps the Keyword `People|<name>` on its Photo. The AI never names anyone.
CREATE TABLE people (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
  created_at TEXT NOT NULL
);
CREATE TABLE face_names (
  photo_id  INTEGER NOT NULL REFERENCES photos(id),
  face      INTEGER NOT NULL,
  person_id INTEGER REFERENCES people(id),   -- NULL: hidden
  at        TEXT NOT NULL,
  PRIMARY KEY (photo_id, face)
);
CREATE INDEX face_names_by_person ON face_names(person_id);
CREATE TABLE face_not (
  photo_id  INTEGER NOT NULL REFERENCES photos(id),
  face      INTEGER NOT NULL,
  person_id INTEGER NOT NULL REFERENCES people(id),
  PRIMARY KEY (photo_id, face, person_id)
);
