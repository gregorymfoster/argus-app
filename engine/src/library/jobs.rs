//! The background queue's storage. What each kind of job does lives in [`crate::jobs`].

use super::{db_err, time, Library};
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type JobId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Done => "done",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for JobStatus {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        serde_json::from_value(Value::String(s.into()))
            .map_err(|_| Error(format!("unknown job status {s:?}")))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: JobId,
    pub kind: String,
    pub args: Value,
    pub priority: i64,
    pub label: String,
    pub status: JobStatus,
    pub done: i64,
    pub total: i64,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// How the queue stands, for the app's progress line.
#[derive(Debug, Clone, Serialize)]
pub struct QueueSummary {
    pub running: Option<Job>,
    pub queued: i64,
    pub failed: i64,
    pub done_today: i64,
    /// Rough seconds left: queued jobs times the recent average of each kind.
    pub eta_s: Option<f64>,
}

const COLS: &str = "id, kind, args, priority, label, status, done, total, result, error, created_at, started_at, finished_at";

fn job(r: &Row) -> rusqlite::Result<Job> {
    let json = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok());
    let status: String = r.get(5)?;
    Ok(Job {
        id: r.get(0)?,
        kind: r.get(1)?,
        args: json(r.get(2)?).unwrap_or(Value::Null),
        priority: r.get(3)?,
        label: r.get(4)?,
        status: status.parse().unwrap_or(JobStatus::Failed),
        done: r.get(6)?,
        total: r.get(7)?,
        result: json(r.get(8)?),
        error: r.get(9)?,
        created_at: r.get(10)?,
        started_at: r.get(11)?,
        finished_at: r.get(12)?,
    })
}

impl Library {
    /// One Photo by id, or None.
    pub fn photo(&self, id: super::PhotoId) -> Result<Option<super::Photo>> {
        Ok(self
            .list_photos(&super::PhotoFilter::default())?
            .into_iter()
            .find(|p| p.id == id))
    }

    /// A raw value from the Library's settings table.
    pub fn setting(&self, key: &str) -> Result<Option<Value>> {
        let v: Option<String> = self
            .db
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_err)?;
        Ok(v.and_then(|v| serde_json::from_str(&v).ok()))
    }

    pub fn set_setting(&self, key: &str, value: &Value) -> Result<()> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                params![key, value.to_string()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Queue a job. With a `dedupe_key` already queued or running, returns that job instead.
    pub fn enqueue_job(
        &mut self,
        kind: &str,
        args: &Value,
        priority: i64,
        label: &str,
        dedupe_key: Option<&str>,
    ) -> Result<(JobId, bool)> {
        if let Some(key) = dedupe_key {
            let existing: Option<JobId> = self
                .db
                .query_row(
                    "SELECT id FROM jobs WHERE dedupe_key = ?1 AND status IN ('queued', 'running')",
                    [key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?;
            if let Some(id) = existing {
                return Ok((id, false));
            }
        }
        self.db
            .execute(
                "INSERT INTO jobs (kind, args, priority, dedupe_key, label, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6)",
                params![
                    kind,
                    args.to_string(),
                    priority,
                    dedupe_key,
                    label,
                    time::now()
                ],
            )
            .map_err(db_err)?;
        Ok((self.db.last_insert_rowid(), true))
    }

    /// Raise a queued job's priority to at least `priority`. Returns whether one was raised.
    pub fn raise_job(&mut self, dedupe_key: &str, priority: i64) -> Result<bool> {
        let n = self
            .db
            .execute(
                "UPDATE jobs SET priority = ?2 WHERE dedupe_key = ?1 AND status = 'queued' AND priority < ?2",
                params![dedupe_key, priority],
            )
            .map_err(db_err)?;
        Ok(n > 0)
    }

    /// Take the next job (highest priority, then oldest) and mark it running.
    pub fn claim_job(&mut self) -> Result<Option<Job>> {
        self.claim_job_at_least(i64::MIN)
    }

    /// [`Self::claim_job`], but only a job of at least `min_priority` (on battery, only
    /// the work the person is waiting on runs).
    pub fn claim_job_at_least(&mut self, min_priority: i64) -> Result<Option<Job>> {
        self.claim_job_holding(min_priority, &[], i64::MAX)
    }

    /// [`Self::claim_job_at_least`], also passing over jobs of the `held` kinds below
    /// `held_below` priority (model work waiting for the Mac to be idle).
    pub fn claim_job_holding(
        &mut self,
        min_priority: i64,
        held: &[&str],
        held_below: i64,
    ) -> Result<Option<Job>> {
        let held_list = held
            .iter()
            .map(|k| format!("'{}'", k.replace('\'', "")))
            .collect::<Vec<_>>()
            .join(",");
        // SQLite takes an empty `IN ()`: then nothing is held.
        let hold = format!(" AND (priority >= ?2 OR kind NOT IN ({held_list}))");
        let tx = self.db.transaction().map_err(db_err)?;
        let next: Option<Job> = tx
            .query_row(
                &format!("SELECT {COLS} FROM jobs WHERE status = 'queued' AND priority >= ?1{hold} ORDER BY priority DESC, id LIMIT 1"),
                params![min_priority, held_below],
                job,
            )
            .optional()
            .map_err(db_err)?;
        let Some(mut j) = next else { return Ok(None) };
        let now = time::now();
        // Another process may have taken it between the read and the write.
        let took = tx
            .execute(
                "UPDATE jobs SET status = 'running', started_at = ?2 WHERE id = ?1 AND status = 'queued'",
                params![j.id, now],
            )
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        if took == 0 {
            return Ok(None);
        }
        j.status = JobStatus::Running;
        j.started_at = Some(now);
        Ok(Some(j))
    }

    pub fn job_progress(&self, id: JobId, done: i64, total: i64) -> Result<()> {
        self.db
            .execute(
                "UPDATE jobs SET done = ?2, total = ?3 WHERE id = ?1",
                params![id, done, total],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Finish a running job with its result or error. A job cancelled while it ran stays cancelled.
    pub fn finish_job(&self, id: JobId, outcome: std::result::Result<&Value, &str>) -> Result<()> {
        let (status, result, error) = match outcome {
            Ok(v) => ("done", Some(v.to_string()), None),
            Err(e) => ("failed", None, Some(e.to_string())),
        };
        self.db
            .execute(
                "UPDATE jobs SET status = ?2, result = ?3, error = ?4, finished_at = ?5
                 WHERE id = ?1 AND status = 'running'",
                params![id, status, result, error, time::now()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Cancel a queued or running job (a running one stops at its next checkpoint).
    pub fn cancel_job(&self, id: JobId) -> Result<bool> {
        let n = self
            .db
            .execute(
                "UPDATE jobs SET status = 'cancelled', finished_at = ?2 WHERE id = ?1 AND status IN ('queued', 'running')",
                params![id, time::now()],
            )
            .map_err(db_err)?;
        Ok(n > 0)
    }

    /// Put queued jobs in this order (first runs first), like dragging them in the Background
    /// work panel. They get strictly falling priorities from the highest they had, so they
    /// keep their place against other work; jobs not listed, or not queued, are left alone.
    /// Work for the open Photo (priority `PRIORITY_FOCUS` and up) always stays first and runs
    /// at Full, so it never moves and nothing is dragged into its band. Returns how many moved.
    pub fn reorder_jobs(&mut self, order: &[JobId]) -> Result<usize> {
        use crate::jobs::PRIORITY_FOCUS;
        let tx = self.db.transaction().map_err(db_err)?;
        let mut movable = Vec::new();
        for id in order {
            let p: Option<i64> = tx
                .query_row(
                    "SELECT priority FROM jobs WHERE id = ?1 AND status = 'queued' AND priority < ?2",
                    params![id, PRIORITY_FOCUS],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?;
            if let Some(p) = p {
                movable.push((*id, p));
            }
        }
        let Some(top) = movable.iter().map(|(_, p)| *p).max() else {
            return Ok(0);
        };
        let mut n = 0;
        for (i, (id, _)) in movable.iter().enumerate() {
            n += tx
                .execute(
                    "UPDATE jobs SET priority = ?2 WHERE id = ?1",
                    params![id, top - i as i64],
                )
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(n)
    }

    /// Queue failed jobs again (a model that was missing is back). Only the newest of each
    /// dedupe key runs again, and none whose key is already queued or running; the rest are
    /// cancelled so the failed count clears. Returns how many were queued.
    pub fn retry_failed_jobs(&self) -> Result<usize> {
        let n = self
            .db
            .execute(
                "UPDATE jobs SET status = 'queued', error = NULL, result = NULL, done = 0,
                        started_at = NULL, finished_at = NULL
                 WHERE status = 'failed' AND (dedupe_key IS NULL OR (
                   id = (SELECT MAX(id) FROM jobs f WHERE f.status = 'failed' AND f.dedupe_key = jobs.dedupe_key)
                   AND NOT EXISTS (SELECT 1 FROM jobs o WHERE o.dedupe_key = jobs.dedupe_key
                                   AND o.status IN ('queued', 'running'))))",
                [],
            )
            .map_err(db_err)?;
        self.db
            .execute(
                "UPDATE jobs SET status = 'cancelled' WHERE status = 'failed'",
                [],
            )
            .map_err(db_err)?;
        Ok(n)
    }

    pub fn job(&self, id: JobId) -> Result<Job> {
        self.db
            .query_row(&format!("SELECT {COLS} FROM jobs WHERE id = ?1"), [id], job)
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| Error(format!("no job with id {id}")))
    }

    /// Jobs, newest first, optionally of one status and kind.
    pub fn jobs(
        &self,
        status: Option<JobStatus>,
        kind: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Job>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {COLS} FROM jobs WHERE (?1 IS NULL OR status = ?1) AND (?2 IS NULL OR kind = ?2)
                 ORDER BY id DESC LIMIT ?3"
            ))
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![status.map(|s| s.as_str()), kind, limit], job)
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Put jobs a quit or crash left running back in the queue. Returns how many.
    pub fn requeue_stale_jobs(&self) -> Result<usize> {
        self.db
            .execute(
                "UPDATE jobs SET status = 'queued', started_at = NULL WHERE status = 'running'",
                [],
            )
            .map_err(db_err)
    }

    /// Average seconds a job of `kind` took, over the last 20 that finished.
    pub fn average_job_seconds(&self, kind: &str) -> Result<Option<f64>> {
        self.db
            .query_row(
                "SELECT AVG(julianday(finished_at) - julianday(started_at)) * 86400 FROM
                 (SELECT finished_at, started_at FROM jobs WHERE kind = ?1 AND status = 'done'
                  ORDER BY id DESC LIMIT 20)",
                [kind],
                |r| r.get(0),
            )
            .map_err(db_err)
    }

    /// Seconds since a job started, if it has.
    pub fn job_elapsed_seconds(&self, id: JobId) -> Result<Option<f64>> {
        self.db
            .query_row(
                "SELECT (julianday('now') - julianday(started_at)) * 86400 FROM jobs WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)
            .map(Option::flatten)
    }

    /// The queued or running job with this dedupe key, if any.
    pub fn active_job(&self, dedupe_key: &str) -> Result<Option<Job>> {
        self.db
            .query_row(
                &format!("SELECT {COLS} FROM jobs WHERE dedupe_key = ?1 AND status IN ('queued', 'running') ORDER BY id DESC LIMIT 1"),
                [dedupe_key],
                job,
            )
            .optional()
            .map_err(db_err)
    }

    /// The jobs that run before queued job `id`: every running job, then queued ones of
    /// higher priority (or equal and older), in the order they'll run.
    pub fn jobs_ahead(&self, id: JobId) -> Result<Vec<Job>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {COLS} FROM jobs, (SELECT priority AS p FROM jobs WHERE id = ?1) me
                 WHERE status = 'running' OR (status = 'queued' AND id != ?1
                   AND (priority > me.p OR (priority = me.p AND id < ?1)))
                 ORDER BY status = 'queued', priority DESC, id"
            ))
            .map_err(db_err)?;
        let rows = stmt.query_map([id], job).map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Whether any job waits to run: the worker's cheap check on each idle round.
    pub fn has_queued_jobs(&self) -> Result<bool> {
        self.db
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM jobs WHERE status = 'queued')",
                [],
                |r| r.get(0),
            )
            .map_err(db_err)
    }

    pub fn queue_summary(&self) -> Result<QueueSummary> {
        let count =
            |sql: &str| -> Result<i64> { self.db.query_row(sql, [], |r| r.get(0)).map_err(db_err) };
        let running = self
            .db
            .query_row(
                &format!(
                    "SELECT {COLS} FROM jobs WHERE status = 'running' ORDER BY started_at LIMIT 1"
                ),
                [],
                job,
            )
            .optional()
            .map_err(db_err)?;
        let today = &time::now()[..10];
        let done_today: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE status = 'done' AND finished_at >= ?1",
                [today],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        // Average seconds per kind over the last 20 finished jobs of that kind.
        let mut stmt = self
            .db
            .prepare("SELECT kind, COUNT(*) FROM jobs WHERE status = 'queued' GROUP BY kind")
            .map_err(db_err)?;
        let queued_kinds: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        let mut eta = Some(0.0);
        for (kind, n) in &queued_kinds {
            let avg = self.average_job_seconds(kind)?;
            eta = match (eta, avg) {
                (Some(e), Some(a)) => Some(e + a * *n as f64),
                _ => None,
            };
        }
        Ok(QueueSummary {
            running,
            queued: count("SELECT COUNT(*) FROM jobs WHERE status = 'queued'")?,
            failed: count("SELECT COUNT(*) FROM jobs WHERE status = 'failed'")?,
            done_today,
            eta_s: eta.map(|e| e.round()),
        })
    }
}
