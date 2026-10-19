//! The usage log: an append-only record of what happened to each Photo (where it started,
//! every AI take, what the person kept, flagged and exported), so benches and a later
//! "learn my edits" can run on real use. Kept in the Library, never uploaded. Logging is
//! best-effort: a failed write never fails the edit it describes. Format: docs/usage-log.md.

use super::{db_err, time, BatchId, Library, PhotoId};
use crate::Result;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

/// The setting that turns recording on or off (Settings › General). On by default.
pub const RECORD_KEY: &str = "record_usage";

#[derive(Debug, Clone, Serialize)]
pub struct UsageEvent {
    pub id: i64,
    pub photo_id: PhotoId,
    pub kind: String,
    pub data: Value,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub recording: bool,
    pub events: i64,
    pub photos: i64,
    pub takes: i64,
    pub exports: i64,
    pub since: Option<String>,
}

impl Library {
    pub fn recording_usage(&self) -> bool {
        !matches!(self.setting(RECORD_KEY), Ok(Some(Value::Bool(false))))
    }

    pub fn set_recording_usage(&self, on: bool) -> Result<()> {
        self.set_setting(RECORD_KEY, &json!(on))
    }

    /// Append one event. A Photo's first event is preceded by a `start` holding the Recipe
    /// it had then (null: never edited, so the default develop).
    pub(crate) fn log_usage(&self, photo: PhotoId, kind: &str, data: Value) {
        if !self.recording_usage() {
            return;
        }
        if let Err(e) = self.try_log(photo, kind, data) {
            eprintln!("argus: usage log: {e}");
        }
    }

    fn try_log(&self, photo: PhotoId, kind: &str, data: Value) -> Result<()> {
        self.try_start(photo)?;
        self.insert_event(photo, kind, &data)
    }

    /// Log a Photo's `start` if it has no events yet. Write paths call this before they
    /// change the Photo, so the start is the state before the first change.
    pub(crate) fn log_start(&self, photo: PhotoId) {
        if self.recording_usage() {
            if let Err(e) = self.try_start(photo) {
                eprintln!("argus: usage log: {e}");
            }
        }
    }

    fn try_start(&self, photo: PhotoId) -> Result<()> {
        let seen = self
            .db
            .query_row(
                "SELECT 1 FROM usage_events WHERE photo_id = ?1 LIMIT 1",
                [photo],
                |_| Ok(()),
            )
            .optional()
            .map_err(db_err)?
            .is_some();
        if seen {
            return Ok(());
        }
        let start = self.read_recipe(photo)?;
        self.insert_event(
            photo,
            "start",
            &json!({
                "recipe": start.as_ref().map(|v| &v.recipe),
                "version": start.as_ref().map(|v| v.version),
            }),
        )
    }

    fn insert_event(&self, photo: PhotoId, kind: &str, data: &Value) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO usage_events (photo_id, kind, data, at) VALUES (?1, ?2, ?3, ?4)",
                params![photo, kind, data.to_string(), time::now()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// A Recipe Version was written: log it with who wrote it and the batch's label, which
    /// says where it came from ("Auto on import", an Agent request, "Suggestion: ...").
    pub(crate) fn log_recipe(
        &self,
        photo: PhotoId,
        version: i64,
        recipe: &Value,
        author: &str,
        batch: Option<BatchId>,
    ) {
        if !self.recording_usage() {
            return;
        }
        let label: Option<String> = batch.and_then(|b| {
            self.db
                .query_row("SELECT label FROM batches WHERE id = ?1", [b], |r| r.get(0))
                .ok()
        });
        self.log_usage(
            photo,
            "recipe",
            json!({ "version": version, "author": author, "batch_id": batch,
                    "label": label, "recipe": recipe }),
        );
    }

    /// Every event, oldest first, for one Photo or all of them.
    pub fn usage_events(&self, photo: Option<PhotoId>) -> Result<Vec<UsageEvent>> {
        let mut q = self
            .db
            .prepare(
                "SELECT id, photo_id, kind, data, at FROM usage_events
                 WHERE ?1 IS NULL OR photo_id = ?1 ORDER BY id",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map([photo], |r| {
                let data: String = r.get(3)?;
                Ok(UsageEvent {
                    id: r.get(0)?,
                    photo_id: r.get(1)?,
                    kind: r.get(2)?,
                    data: serde_json::from_str(&data).unwrap_or(Value::Null),
                    at: r.get(4)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    pub fn usage_summary(&self) -> Result<UsageSummary> {
        let count =
            |sql: &str| -> Result<i64> { self.db.query_row(sql, [], |r| r.get(0)).map_err(db_err) };
        Ok(UsageSummary {
            recording: self.recording_usage(),
            events: count("SELECT COUNT(*) FROM usage_events")?,
            photos: count("SELECT COUNT(DISTINCT photo_id) FROM usage_events")?,
            takes: count(
                "SELECT COUNT(*) FROM usage_events WHERE kind IN ('take', 'agent')
                 OR (kind = 'recipe' AND json_extract(data, '$.author') = 'ai'
                     AND json_extract(data, '$.label') LIKE 'Auto%')",
            )?,
            exports: count("SELECT COUNT(*) FROM usage_events WHERE kind = 'export'")?,
            since: self
                .db
                .query_row("SELECT MIN(at) FROM usage_events", [], |r| r.get(0))
                .map_err(db_err)?,
        })
    }

    /// Delete the whole usage log. Returns how many events went.
    pub fn clear_usage(&self) -> Result<i64> {
        let n = self
            .db
            .execute("DELETE FROM usage_events", [])
            .map_err(db_err)?;
        Ok(n as i64)
    }
}
