//! Resumable runs: an Export or Import recorded as it goes, so closing the lid, a quit or a
//! crash part way never loses it (Greg, 2026-11-02). The next start finds the run its
//! process left behind ([`Library::orphaned_runs`]) and the Queue finishes it from the Photo
//! it reached (`resume_run`, [`crate::jobs`]).
//!
//! An Export claims each file's name before it writes ([`Library::claim_run_item`]), and
//! writes through a temporary file renamed into place, so a resumed Export finds each file
//! either whole under its name or not there yet, and writes it once.

use super::{db_err, time, Library, PhotoId};
use crate::Result;
use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

pub type RunId = i64;

/// One run, as the app's Work mode shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub id: RunId,
    /// `export` or `import`.
    pub kind: String,
    pub label: String,
    pub args: Value,
    /// `running`, `paused`, `done` or `stopped`.
    pub status: String,
    /// Why it is paused: `quit` (Argus quit or crashed), `card` (its card is not connected).
    pub note: Option<String>,
    pub pid: Option<i64>,
    pub total: i64,
    /// Photos finished (an Export's), from its items.
    pub done: i64,
    pub started_at: String,
    pub updated_at: String,
}

const COLS: &str = "id, kind, label, args, status, note, pid, total, started_at, updated_at,
    (SELECT COUNT(*) FROM run_items i WHERE i.run_id = runs.id AND i.done = 1)";

fn run(r: &Row) -> rusqlite::Result<Run> {
    let args: String = r.get(3)?;
    Ok(Run {
        id: r.get(0)?,
        kind: r.get(1)?,
        label: r.get(2)?,
        args: serde_json::from_str(&args).unwrap_or(Value::Null),
        status: r.get(4)?,
        note: r.get(5)?,
        pid: r.get(6)?,
        total: r.get(7)?,
        started_at: r.get(8)?,
        updated_at: r.get(9)?,
        done: r.get(10)?,
    })
}

/// Whether process `pid` is still alive on this Mac.
pub fn alive(pid: i64) -> bool {
    if pid == std::process::id() as i64 {
        return true;
    }
    if pid <= 0 || pid > i32::MAX as i64 {
        return false;
    }
    // Signal 0 checks without sending anything; EPERM means it lives under another user.
    let r = unsafe { libc::kill(pid as i32, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

impl Library {
    /// Record a new run, driven by this process.
    pub fn start_run(&self, kind: &str, label: &str, args: &Value, total: i64) -> Result<RunId> {
        let now = time::now();
        self.db
            .execute(
                "INSERT INTO runs (kind, label, args, status, pid, total, started_at, updated_at)
                 VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, ?6)",
                params![
                    kind,
                    label,
                    args.to_string(),
                    std::process::id() as i64,
                    total,
                    now
                ],
            )
            .map_err(db_err)?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn run(&self, id: RunId) -> Result<Run> {
        self.db
            .query_row(&format!("SELECT {COLS} FROM runs WHERE id = ?1"), [id], run)
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| crate::Error(format!("no run {id}")))
    }

    /// Runs not finished (running or paused), oldest first.
    pub fn unfinished_runs(&self) -> Result<Vec<Run>> {
        let mut q = self
            .db
            .prepare(&format!(
                "SELECT {COLS} FROM runs WHERE status IN ('running', 'paused') ORDER BY id"
            ))
            .map_err(db_err)?;
        let rows = q.query_map([], run).map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Runs left `running` by a process that is gone: Argus quit or crashed mid-way.
    pub fn orphaned_runs(&self) -> Result<Vec<Run>> {
        Ok(self
            .unfinished_runs()?
            .into_iter()
            .filter(|r| r.status == "running" && !r.pid.is_some_and(alive))
            .collect())
    }

    /// Set a run's status (`running` takes it for this process), with why when paused.
    pub fn set_run_status(&self, id: RunId, status: &str, note: Option<&str>) -> Result<()> {
        self.db
            .execute(
                "UPDATE runs SET status = ?2, note = ?3, updated_at = ?4,
                    pid = CASE WHEN ?2 = 'running' THEN ?5 ELSE pid END WHERE id = ?1",
                params![id, status, note, time::now(), std::process::id() as i64],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    /// Replace what resumes a run (an Import's Shoot, once it has one).
    pub fn set_run_args(&self, id: RunId, args: &Value) -> Result<()> {
        self.db
            .execute(
                "UPDATE runs SET args = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, args.to_string(), time::now()],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    /// A Photo of a run: the file it claimed, and whether that file is finished.
    pub fn run_item(&self, id: RunId, photo: PhotoId) -> Result<Option<(Option<PathBuf>, bool)>> {
        self.db
            .query_row(
                "SELECT path, done FROM run_items WHERE run_id = ?1 AND photo_id = ?2",
                params![id, photo],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?.map(PathBuf::from),
                        r.get::<_, i64>(1)? == 1,
                    ))
                },
            )
            .optional()
            .map_err(db_err)
    }

    /// Every Photo of a run: (Photo, file, done).
    pub fn run_items(&self, id: RunId) -> Result<Vec<(PhotoId, Option<PathBuf>, bool)>> {
        let mut q = self
            .db
            .prepare("SELECT photo_id, path, done FROM run_items WHERE run_id = ?1 ORDER BY rowid")
            .map_err(db_err)?;
        let rows = q
            .query_map([id], |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<String>>(1)?.map(PathBuf::from),
                    r.get::<_, i64>(2)? == 1,
                ))
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Record the file a run's Photo is about to be written to, before writing it.
    pub fn claim_run_item(&self, id: RunId, photo: PhotoId, path: &std::path::Path) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO run_items (run_id, photo_id, path, done) VALUES (?1, ?2, ?3, 0)
                 ON CONFLICT (run_id, photo_id) DO UPDATE SET path = excluded.path, done = 0",
                params![id, photo, path.to_string_lossy()],
            )
            .map_err(db_err)?;
        self.touch_run(id)
    }

    /// A run's Photo is written, whole, at `path`.
    pub fn finish_run_item(&self, id: RunId, photo: PhotoId, path: &std::path::Path) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO run_items (run_id, photo_id, path, done) VALUES (?1, ?2, ?3, 1)
                 ON CONFLICT (run_id, photo_id) DO UPDATE SET path = excluded.path, done = 1",
                params![id, photo, path.to_string_lossy()],
            )
            .map_err(db_err)?;
        self.touch_run(id)
    }

    /// Mark a run done or stopped. A stopped Export clears what it had claimed but not
    /// finished (an empty name, a temporary file); its whole files stay.
    pub fn end_run(&self, id: RunId, stopped: bool) -> Result<Run> {
        if stopped {
            for (_, path, done) in self.run_items(id)? {
                if let (Some(p), false) = (path, done) {
                    crate::export::clear_unfinished(&p);
                    crate::export::clear_unfinished(&p.with_extension("xmp"));
                }
            }
        }
        self.set_run_status(id, if stopped { "stopped" } else { "done" }, None)?;
        self.run(id)
    }

    fn touch_run(&self, id: RunId) -> Result<()> {
        self.db
            .execute(
                "UPDATE runs SET updated_at = ?2 WHERE id = ?1",
                params![id, time::now()],
            )
            .map(|_| ())
            .map_err(db_err)
    }
}
