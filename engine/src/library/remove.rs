//! Remove from Library, like Lightroom's Remove: Photos (or whole Shoots) leave the Library
//! but nothing is lost. Every row that belongs to them (Verdicts, Ratings, Recipe Versions,
//! Stacks, Suggestions, Facts, queued jobs...) moves into `trash.db` next to `library.db`,
//! so `restore_removed` puts them back exactly, ids included, and Undo works across restarts.
//!
//! Originals are never touched unless the person asks: with `delete_originals` they move to
//! the Trash (renamed if a file there has the name), and a restore moves them back while
//! they're still there. Previews stay until a removal is emptied, by hand or 30 days on.
//!
//! The rows to move are found from the schema's foreign keys, so a table added later is
//! covered without a change here. The trash's tables mirror the Library's (plus a `removal`
//! column) and grow a column when the Library's table did.

use super::{db_err, resolve_path, time, Library, PhotoId, ShootId};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const TRASH_FILE: &str = "trash.db";
/// Removals older than this are emptied when the next one happens.
const KEEP: Duration = Duration::from_secs(30 * 24 * 3600);

#[derive(Debug, Clone, Serialize)]
pub struct RemoveReport {
    /// Pass to `restore_removed` to undo.
    pub removal: i64,
    pub label: String,
    pub photos: usize,
    pub shoots: usize,
    /// Original files moved to the Trash (a RAW+JPEG pair counts two).
    pub originals_trashed: usize,
    /// Files that couldn't be moved; their Photos are removed all the same.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub removal: i64,
    pub label: String,
    pub photos: usize,
    pub shoots: usize,
    /// Originals moved back from the Trash.
    pub originals_back: usize,
    /// Originals no longer in the Trash: their Photos come back as missing (relink finds them).
    pub errors: Vec<String>,
}

/// One removal that can still be restored.
#[derive(Debug, Clone, Serialize)]
pub struct Removal {
    pub removal: i64,
    pub label: String,
    pub at: String,
    pub photos: usize,
    pub shoots: usize,
    pub originals_trashed: usize,
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn id_list(ids: &BTreeSet<i64>) -> String {
    ids.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn columns(db: &Connection, schema: &str, table: &str) -> Result<Vec<String>> {
    let mut stmt = db
        .prepare(&format!("PRAGMA {schema}.table_info({})", quote(table)))
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(db_err)?;
    rows.collect::<std::result::Result<_, _>>().map_err(db_err)
}

fn tables(db: &Connection, schema: &str) -> Result<Vec<String>> {
    let mut stmt = db
        .prepare(&format!(
            "SELECT name FROM {schema}.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        ))
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(db_err)?;
    rows.collect::<std::result::Result<_, _>>().map_err(db_err)
}

/// A foreign key: (column, parent table, parent column).
type ForeignKey = (String, String, String);

/// Each foreign key of `table`.
fn foreign_keys(db: &Connection, table: &str) -> Result<Vec<ForeignKey>> {
    let mut stmt = db
        .prepare(&format!("PRAGMA main.foreign_key_list({})", quote(table)))
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            let to: Option<String> = r.get(4)?;
            Ok((
                r.get::<_, String>(3)?,
                r.get::<_, String>(2)?,
                to.unwrap_or_else(|| "rowid".into()),
            ))
        })
        .map_err(db_err)?;
    rows.collect::<std::result::Result<_, _>>().map_err(db_err)
}

/// Where the Trash is: `ARGUS_TRASH` (tests), else the Mac's `~/.Trash`, else the
/// freedesktop Trash on Linux.
fn trash_dir() -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("ARGUS_TRASH") {
        return Ok(PathBuf::from(d));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| Error("no HOME, so no Trash".into()))?;
    let home = PathBuf::from(home);
    Ok(if cfg!(target_os = "macos") {
        home.join(".Trash")
    } else {
        home.join(".local/share/Trash/files")
    })
}

/// Move a file, copying across disks; never removes the source before the copy is whole.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::File::open(to)?.sync_all()?;
    std::fs::remove_file(from)
}

/// A free name for `file` in `dir`: `DSC08005.ARW`, then `DSC08005-2.ARW`...
fn free_name(dir: &Path, file: &Path) -> PathBuf {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            dir.join(&name)
        } else {
            dir.join(format!("{stem}-{n}{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

impl Library {
    fn attach_trash(&self) -> Result<()> {
        let path = self.root.join(TRASH_FILE);
        self.db
            .execute("ATTACH DATABASE ?1 AS trash", [path.to_string_lossy()])
            .map_err(db_err)?;
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS trash.removals (
                   id INTEGER PRIMARY KEY, label TEXT NOT NULL, at TEXT NOT NULL,
                   photos INTEGER NOT NULL, shoots INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS trash.files (
                   removal INTEGER NOT NULL, original TEXT NOT NULL, trashed TEXT NOT NULL);",
            )
            .map_err(db_err)
    }

    /// Run `f` with `trash.db` attached, detaching after whatever happened.
    fn with_trash<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.attach_trash()?;
        let out = f(self);
        let _ = self.db.execute_batch("DETACH DATABASE trash");
        out
    }

    /// Remove Photos and whole Shoots from the Library (see the module docs). An empty
    /// Shoot left behind by removing its last Photos stays, like Lightroom's folders.
    pub fn remove(
        &mut self,
        photos: &[PhotoId],
        shoots: &[ShootId],
        delete_originals: bool,
    ) -> Result<RemoveReport> {
        if photos.is_empty() && shoots.is_empty() {
            return Err(Error(
                "nothing to remove: pass photo_ids or shoot_ids".into(),
            ));
        }
        let report = self.with_trash(|lib| lib.remove_rows(photos, shoots, delete_originals))?;
        // Old removals go once a new one lands; a failure here never undoes the removal.
        let cutoff = time::from_system(SystemTime::now() - KEEP);
        if let Ok(old) = self.removed().map(|rs| {
            rs.into_iter()
                .filter(|r| r.at < cutoff)
                .map(|r| r.removal)
                .collect::<Vec<_>>()
        }) {
            for r in old {
                let _ = self.empty_removed(Some(r));
            }
        }
        Ok(report)
    }

    fn remove_rows(
        &mut self,
        photos: &[PhotoId],
        shoots: &[ShootId],
        delete_originals: bool,
    ) -> Result<RemoveReport> {
        // Everything doomed, by table, as rowids: the seeds, then whatever points at them.
        let mut doomed: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
        let present = |db: &Connection, table: &str, ids: &[i64]| -> Result<BTreeSet<i64>> {
            let set: BTreeSet<i64> = ids.iter().copied().collect();
            let mut stmt = db
                .prepare(&format!(
                    "SELECT rowid FROM main.{table} WHERE rowid IN ({})",
                    id_list(&set)
                ))
                .map_err(db_err)?;
            let rows = stmt.query_map([], |r| r.get(0)).map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>().map_err(db_err)
        };
        let shoot_set = present(&self.db, "shoots", shoots)?;
        let photo_set = present(&self.db, "photos", photos)?;
        if shoot_set.len() < shoots.len() || photo_set.len() < photos.len() {
            return Err(Error(
                "some of those Photos or Shoots aren't in the Library".into(),
            ));
        }
        doomed.insert("shoots".into(), shoot_set.clone());
        doomed.insert("photos".into(), photo_set);
        // Queued work for them would only fail later; it comes back on restore.
        let mut queued = BTreeSet::new();
        {
            let mut stmt = self
                .db
                .prepare(
                    "SELECT id, json_extract(args, '$.photo_id'), json_extract(args, '$.shoot_id')
                     FROM main.jobs WHERE status = 'queued'",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (id, p, s) = row.map_err(db_err)?;
                if p.is_some_and(|p| photos.contains(&p))
                    || s.is_some_and(|s| shoot_set.contains(&s))
                {
                    queued.insert(id);
                }
            }
        }
        // Photos of removed Shoots are found below through photos.shoot_id.
        doomed.insert("jobs".into(), queued);

        let all = tables(&self.db, "main")?;
        let fks: Vec<(String, Vec<ForeignKey>)> = all
            .iter()
            .map(|t| Ok((t.clone(), foreign_keys(&self.db, t)?)))
            .collect::<Result<_>>()?;
        loop {
            let mut grew = false;
            for (table, keys) in &fks {
                for (col, parent, pcol) in keys {
                    let Some(parents) = doomed.get(parent).filter(|s| !s.is_empty()) else {
                        continue;
                    };
                    let sql = format!(
                        "SELECT rowid FROM main.{} WHERE {} IN (SELECT {} FROM main.{} WHERE rowid IN ({}))",
                        quote(table),
                        quote(col),
                        if pcol == "rowid" { "rowid".to_string() } else { quote(pcol) },
                        quote(parent),
                        id_list(parents)
                    );
                    let mut stmt = self.db.prepare(&sql).map_err(db_err)?;
                    let found: Vec<i64> = stmt
                        .query_map([], |r| r.get(0))
                        .map_err(db_err)?
                        .collect::<std::result::Result<_, _>>()
                        .map_err(db_err)?;
                    let set = doomed.entry(table.clone()).or_default();
                    for id in found {
                        grew |= set.insert(id);
                    }
                }
            }
            if !grew {
                break;
            }
        }

        // The originals, read before their rows go.
        let photo_ids = doomed["photos"].clone();
        let mut originals: Vec<String> = Vec::new();
        if delete_originals {
            let mut stmt = self
                .db
                .prepare(&format!(
                    "SELECT path, jpeg FROM main.photos WHERE id IN ({})",
                    id_list(&photo_ids)
                ))
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })
                .map_err(db_err)?;
            for row in rows {
                let (path, jpeg) = row.map_err(db_err)?;
                originals.push(path);
                originals.extend(jpeg);
            }
        }

        let n_photos = photo_ids.len();
        let n_shoots = shoot_set.len();
        let label = match (n_shoots, n_photos) {
            (0, 1) => "Removed 1 Photo".to_string(),
            (0, n) => format!("Removed {n} Photos"),
            (1, n) => {
                let name: String = self
                    .db
                    .query_row(
                        "SELECT name FROM main.shoots WHERE id = ?1",
                        [shoot_set.iter().next().unwrap()],
                        |r| r.get(0),
                    )
                    .map_err(db_err)?;
                format!(
                    "Removed {name} ({n} Photo{})",
                    if n == 1 { "" } else { "s" }
                )
            }
            (s, n) => format!("Removed {s} Shoots ({n} Photos)"),
        };

        let tx = self.db.transaction().map_err(db_err)?;
        // Parents and children go in one go; the keys are checked at commit.
        tx.execute_batch("PRAGMA defer_foreign_keys = ON")
            .map_err(db_err)?;
        tx.execute(
            "INSERT INTO trash.removals (label, at, photos, shoots) VALUES (?1, ?2, ?3, ?4)",
            params![label, time::now(), n_photos as i64, n_shoots as i64],
        )
        .map_err(db_err)?;
        let removal = tx.last_insert_rowid();
        for (table, ids) in &doomed {
            if ids.is_empty() {
                continue;
            }
            let mirror = quote(&format!("t_{table}"));
            tx.execute_batch(&format!(
                "CREATE TABLE IF NOT EXISTS trash.{mirror} AS SELECT 0 AS removal, * FROM main.{} WHERE 0",
                quote(table)
            ))
            .map_err(db_err)?;
            let cols = columns(&tx, "main", table)?;
            let have = columns(&tx, "trash", &format!("t_{table}"))?;
            for c in cols.iter().filter(|c| !have.contains(c)) {
                tx.execute_batch(&format!(
                    "ALTER TABLE trash.{mirror} ADD COLUMN {}",
                    quote(c)
                ))
                .map_err(db_err)?;
            }
            let list = cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
            tx.execute(
                &format!(
                    "INSERT INTO trash.{mirror} (removal, {list}) SELECT ?1, {list} FROM main.{} WHERE rowid IN ({})",
                    quote(table),
                    id_list(ids)
                ),
                [removal],
            )
            .map_err(db_err)?;
            tx.execute(
                &format!(
                    "DELETE FROM main.{} WHERE rowid IN ({})",
                    quote(table),
                    id_list(ids)
                ),
                [],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;

        // Only now, with the rows safely in the trash, do any originals move.
        let mut trashed = 0;
        let mut errors = Vec::new();
        if !originals.is_empty() {
            let dir = trash_dir()?;
            std::fs::create_dir_all(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
            for stored in originals {
                let from = resolve_path(&self.root, &stored);
                if !from.exists() {
                    errors.push(format!("{}: not found", from.display()));
                    continue;
                }
                let to = free_name(&dir, &from);
                match move_file(&from, &to) {
                    Ok(()) => {
                        trashed += 1;
                        self.db
                            .execute(
                                "INSERT INTO trash.files (removal, original, trashed) VALUES (?1, ?2, ?3)",
                                params![removal, stored, to.to_string_lossy()],
                            )
                            .map_err(db_err)?;
                    }
                    Err(e) => errors.push(format!("{}: {e}", from.display())),
                }
            }
        }
        Ok(RemoveReport {
            removal,
            label,
            photos: n_photos,
            shoots: n_shoots,
            originals_trashed: trashed,
            errors,
        })
    }

    /// Removals that can still be restored, newest first.
    pub fn removed(&mut self) -> Result<Vec<Removal>> {
        self.with_trash(|lib| {
            let mut stmt = lib
                .db
                .prepare(
                    "SELECT r.id, r.label, r.at, r.photos, r.shoots,
                       (SELECT COUNT(*) FROM trash.files f WHERE f.removal = r.id)
                     FROM trash.removals r ORDER BY r.id DESC",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Removal {
                        removal: r.get(0)?,
                        label: r.get(1)?,
                        at: r.get(2)?,
                        photos: r.get::<_, i64>(3)? as usize,
                        shoots: r.get::<_, i64>(4)? as usize,
                        originals_trashed: r.get::<_, i64>(5)? as usize,
                    })
                })
                .map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>().map_err(db_err)
        })
    }

    /// Put a removal back (the newest when `removal` is None): its rows, ids and all, and
    /// any originals it moved to the Trash that are still there.
    pub fn restore_removed(&mut self, removal: Option<i64>) -> Result<RestoreReport> {
        self.with_trash(|lib| {
            let (removal, label, photos, shoots): (i64, String, i64, i64) = lib
                .db
                .query_row(
                    "SELECT id, label, photos, shoots FROM trash.removals
                     WHERE (?1 IS NULL OR id = ?1) ORDER BY id DESC LIMIT 1",
                    [removal],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(db_err)?
                .ok_or_else(|| Error("nothing to restore".into()))?;
            // A Photo imported again since can't come back twice.
            if lib.db.query_row("SELECT 1 FROM trash.sqlite_master WHERE name = 't_photos'", [], |_| Ok(())).optional().map_err(db_err)?.is_some() {
                let again: i64 = lib
                    .db
                    .query_row(
                        "SELECT COUNT(*) FROM trash.t_photos t JOIN main.photos p ON p.hash = t.hash OR p.id = t.id WHERE t.removal = ?1",
                        [removal],
                        |r| r.get(0),
                    )
                    .map_err(db_err)?;
                if again > 0 {
                    return Err(Error(format!("{again} of these Photos are in the Library again (imported since), so this can't be restored")));
                }
            }

            // Originals first, so the Photos come back pointing at real files.
            let files: Vec<(String, String)> = {
                let mut stmt = lib.db.prepare("SELECT original, trashed FROM trash.files WHERE removal = ?1").map_err(db_err)?;
                let rows = stmt.query_map([removal], |r| Ok((r.get(0)?, r.get(1)?))).map_err(db_err)?;
                rows.collect::<std::result::Result<_, _>>().map_err(db_err)?
            };
            let mut back = 0;
            let mut errors = Vec::new();
            for (stored, trashed) in files {
                let to = resolve_path(&lib.root, &stored);
                let from = PathBuf::from(&trashed);
                if to.exists() {
                    continue;
                }
                if let Some(dir) = to.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match move_file(&from, &to) {
                    Ok(()) => back += 1,
                    Err(e) => errors.push(format!("{trashed}: {e}")),
                }
            }

            let mirrors: Vec<String> = tables(&lib.db, "trash")?.into_iter().filter(|t| t.starts_with("t_")).collect();
            let tx = lib.db.transaction().map_err(db_err)?;
            tx.execute_batch("PRAGMA defer_foreign_keys = ON").map_err(db_err)?;
            for mirror in &mirrors {
                let table = &mirror[2..];
                let main = columns(&tx, "main", table)?;
                if main.is_empty() {
                    continue; // a table the Library no longer has
                }
                let have = columns(&tx, "trash", mirror)?;
                let list = main.iter().filter(|c| have.contains(c)).map(|c| quote(c)).collect::<Vec<_>>().join(", ");
                tx.execute(
                    &format!(
                        "INSERT INTO main.{} ({list}) SELECT {list} FROM trash.{} WHERE removal = ?1",
                        quote(table),
                        quote(mirror)
                    ),
                    [removal],
                )
                .map_err(db_err)?;
                tx.execute(&format!("DELETE FROM trash.{} WHERE removal = ?1", quote(mirror)), [removal])
                    .map_err(db_err)?;
            }
            tx.execute("DELETE FROM trash.files WHERE removal = ?1", [removal]).map_err(db_err)?;
            tx.execute("DELETE FROM trash.removals WHERE id = ?1", [removal]).map_err(db_err)?;
            tx.commit().map_err(|e| {
                Error(match e {
                    rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
                        "this needs a Shoot that was removed after it: restore that first".to_string()
                    }
                    e => format!("library: {e}"),
                })
            })?;
            Ok(RestoreReport { removal, label, photos: photos as usize, shoots: shoots as usize, originals_back: back, errors })
        })
    }

    /// Forget removals for good (all when `removal` is None): their rows and previews go.
    /// Originals are never deleted here; ones moved to the Trash stay there.
    pub fn empty_removed(&mut self, removal: Option<i64>) -> Result<usize> {
        self.with_trash(|lib| {
            let ids: Vec<i64> = {
                let mut stmt = lib.db.prepare("SELECT id FROM trash.removals WHERE (?1 IS NULL OR id = ?1)").map_err(db_err)?;
                let rows = stmt.query_map([removal], |r| r.get(0)).map_err(db_err)?;
                rows.collect::<std::result::Result<_, _>>().map_err(db_err)?
            };
            let mirrors: Vec<String> = tables(&lib.db, "trash")?.into_iter().filter(|t| t.starts_with("t_")).collect();
            for id in &ids {
                // Previews and Fact files, unless a Photo in the Library uses the same one
                // (the same original imported again).
                let mut files: Vec<String> = Vec::new();
                if mirrors.iter().any(|m| m == "t_photos") {
                    let mut stmt = lib
                        .db
                        .prepare("SELECT preview, hash FROM trash.t_photos t WHERE removal = ?1 AND NOT EXISTS (SELECT 1 FROM main.photos p WHERE p.hash = t.hash)")
                        .map_err(db_err)?;
                    let rows = stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(db_err)?;
                    for row in rows {
                        let (preview, hash) = row.map_err(db_err)?;
                        // Argus's own preview and the camera's, whichever exist.
                        files.push(preview);
                        files.push(super::import::preview_rel(&hash));
                        files.push(super::import::camera_preview_rel(&hash));
                    }
                }
                if mirrors.iter().any(|m| m == "t_facts") && columns(&lib.db, "trash", "t_facts")?.iter().any(|c| c == "blob_file") {
                    let mut stmt = lib
                        .db
                        .prepare("SELECT blob_file FROM trash.t_facts WHERE removal = ?1 AND blob_file IS NOT NULL")
                        .map_err(db_err)?;
                    for f in stmt.query_map([id], |r| r.get::<_, String>(0)).map_err(db_err)? {
                        files.push(f.map_err(db_err)?);
                    }
                }
                let tx = lib.db.transaction().map_err(db_err)?;
                for mirror in &mirrors {
                    tx.execute(&format!("DELETE FROM trash.{} WHERE removal = ?1", quote(mirror)), [id]).map_err(db_err)?;
                }
                tx.execute("DELETE FROM trash.files WHERE removal = ?1", [id]).map_err(db_err)?;
                tx.execute("DELETE FROM trash.removals WHERE id = ?1", [id]).map_err(db_err)?;
                tx.commit().map_err(db_err)?;
                for f in files {
                    let _ = std::fs::remove_file(lib.root.join(f));
                }
            }
            Ok(ids.len())
        })
    }
}
