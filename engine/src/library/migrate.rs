//! Schema versions. A new Library gets `schema.sql` (always the latest) in one step; an
//! older one runs each step after its version, in order, each in its own transaction, so
//! a crash mid-upgrade leaves the Library at the last whole version. Before upgrading an
//! existing Library, a copy of its database goes into `backups/` (ADR 0009).
//!
//! To change the schema: append a step to [`STEPS`] and make the same change in
//! `schema.sql`. `tests/library_safety.rs` checks that an upgraded Library ends up with
//! exactly the tables and columns of a new one.

use super::{backup, db_err, store_path, PhotoId};
use crate::{Error, Result};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use std::path::Path;

/// One upgrade, run inside a write transaction. It gets the Library folder for steps that
/// need more than SQL (rewriting paths, say).
type Step = fn(&Transaction, &Path) -> Result<()>;

/// Every schema change, oldest first: `STEPS[i]` takes a Library from v`i+1` to v`i+2`.
const STEPS: &[Step] = &[
    v2_look_batches,
    v3_raw_jpeg_pairs,
    v4_settings_relative_paths,
    v5_ratings_best_photos,
    v6_queue_suggestions,
    v7_facts,
    v8_usage_log,
    v9_exposure,
];

/// The schema version this Argus writes.
pub const LATEST: i64 = STEPS.len() as i64 + 1;

fn v2_look_batches(tx: &Transaction, _root: &Path) -> Result<()> {
    sql(
        tx,
        "ALTER TABLE looks ADD COLUMN batch_id INTEGER REFERENCES batches(id);",
    )
}

fn v3_raw_jpeg_pairs(tx: &Transaction, _root: &Path) -> Result<()> {
    sql(tx, "ALTER TABLE photos ADD COLUMN jpeg TEXT;")
}

/// A `settings` table, and originals inside the Library stored relative to it, so the
/// Library folder can move.
fn v4_settings_relative_paths(tx: &Transaction, root: &Path) -> Result<()> {
    sql(
        tx,
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    let rows: Vec<(PhotoId, String, Option<String>)> = {
        let mut q = tx
            .prepare("SELECT id, path, jpeg FROM photos")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(db_err)?
    };
    for (id, path, jpeg) in rows {
        let jpeg = jpeg.map(|j| store_path(root, Path::new(&j)));
        tx.execute(
            "UPDATE photos SET path = ?1, jpeg = ?2 WHERE id = ?3",
            params![store_path(root, Path::new(&path)), jpeg, id],
        )
        .map_err(db_err)?;
    }
    Ok(())
}

/// Star Ratings (with their log, for undo) and a Shoot's Best Photos.
fn v5_ratings_best_photos(tx: &Transaction, _root: &Path) -> Result<()> {
    sql(
        tx,
        "CREATE TABLE ratings (
           photo_id INTEGER PRIMARY KEY REFERENCES photos(id),
           stars    INTEGER NOT NULL,
           author   TEXT NOT NULL,
           batch_id INTEGER REFERENCES batches(id),
           at       TEXT NOT NULL
         );
         CREATE TABLE rating_log (
           id       INTEGER PRIMARY KEY,
           photo_id INTEGER NOT NULL REFERENCES photos(id),
           stars    INTEGER NOT NULL,
           author   TEXT NOT NULL,
           batch_id INTEGER REFERENCES batches(id),
           at       TEXT NOT NULL
         );
         CREATE TABLE best_photos (
           photo_id INTEGER PRIMARY KEY REFERENCES photos(id),
           shoot_id INTEGER NOT NULL REFERENCES shoots(id),
           rank     INTEGER NOT NULL
         );",
    )
}

/// The background Queue and Suggestions (ADR 0010): the end of `schema.sql`.
fn v6_queue_suggestions(tx: &Transaction, _root: &Path) -> Result<()> {
    let schema = include_str!("schema.sql");
    sql(
        tx,
        &schema[schema
            .find("-- The background queue")
            .expect("queue in schema")
            ..schema.find("-- Facts: what").expect("facts in schema")],
    )
}

fn v7_facts(tx: &Transaction, _root: &Path) -> Result<()> {
    let schema = include_str!("schema.sql");
    sql(
        tx,
        &schema[schema.find("-- Facts: what").expect("facts in schema")
            ..schema.find("-- Usage log").expect("usage log in schema")],
    )
}

fn v8_usage_log(tx: &Transaction, _root: &Path) -> Result<()> {
    let schema = include_str!("schema.sql");
    sql(
        tx,
        &schema[schema.find("-- Usage log").expect("usage log in schema")..],
    )
}

/// Shutter, aperture, ISO, focal length and lens on each Photo, read from the start of
/// every original that is online (a few hundred KB each; an offline one stays blank).
fn v9_exposure(tx: &Transaction, root: &Path) -> Result<()> {
    sql(
        tx,
        "ALTER TABLE photos ADD COLUMN shutter REAL;
         ALTER TABLE photos ADD COLUMN aperture REAL;
         ALTER TABLE photos ADD COLUMN iso INTEGER;
         ALTER TABLE photos ADD COLUMN focal REAL;
         ALTER TABLE photos ADD COLUMN lens TEXT;",
    )?;
    let rows: Vec<(PhotoId, String)> = {
        let mut q = tx.prepare("SELECT id, path FROM photos").map_err(db_err)?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(db_err)?
    };
    for (id, path) in rows {
        let e = super::scan::read_exif_prefix(&super::resolve_path(root, &path)).exposure;
        tx.execute(
            "UPDATE photos SET shutter = ?1, aperture = ?2, iso = ?3, focal = ?4, lens = ?5
             WHERE id = ?6",
            params![e.shutter, e.aperture, e.iso, e.focal, e.lens, id],
        )
        .map_err(db_err)?;
    }
    Ok(())
}

fn sql(tx: &Transaction, sql: &str) -> Result<()> {
    tx.execute_batch(sql).map_err(db_err)
}

pub fn user_version(db: &Connection) -> Result<i64> {
    db.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_err)
}

/// Bring the Library's database to [`LATEST`]. Safe to race: each step re-reads the
/// version under the write lock, so two processes opening an old Library never both
/// apply a step.
pub fn run(db: &mut Connection, root: &Path) -> Result<()> {
    let found = user_version(db)?;
    if found > LATEST {
        return Err(Error(format!(
            "this Library was written by a newer Argus (schema v{found}; this one knows v{LATEST}). Update Argus to open it."
        )));
    }
    if found == LATEST {
        return Ok(());
    }
    if found > 0 {
        backup::snapshot(db, root, &format!("pre-v{LATEST}"))?;
    }
    loop {
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let v = user_version(&tx)?;
        let next = match v {
            0 => {
                sql(&tx, include_str!("schema.sql"))?;
                LATEST
            }
            v if v < LATEST => {
                STEPS[(v - 1) as usize](&tx, root)
                    .map_err(|e| Error(format!("upgrading the Library to v{}: {e}", v + 1)))?;
                v + 1
            }
            _ => return Ok(()),
        };
        tx.pragma_update(None, "user_version", next)
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
    }
}
