//! Batch undo and Version history. Undo never deletes Recipe Versions or Verdict and
//! Rating history: it appends new ones that put back what each Photo had before the batch, so the undo
//! itself shows in History and can be undone in turn. A Look the batch tuned gets its
//! earlier version appended the same way (so Redo works); a Look the batch created is
//! removed.

use super::{db_err, Author, BatchId, Library, PhotoId, VersionInfo};
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct UndoReport {
    /// The batch that was undone, and the new batch holding the undo.
    pub undone: BatchId,
    pub label: String,
    pub batch_id: BatchId,
    /// Photos whose Recipe went back, and Photos whose Verdict went back.
    pub recipes: Vec<PhotoId>,
    pub verdicts: Vec<PhotoId>,
    /// Photos whose star Rating went back.
    pub ratings: Vec<PhotoId>,
    pub looks: usize,
}

impl Library {
    /// Every saved Version of a Photo's Recipe, oldest first, with its batch's label.
    pub fn versions(&self, photo: PhotoId) -> Result<Vec<VersionInfo>> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT v.version, v.author, v.batch_id, b.label, v.at, v.recipe
                 FROM recipe_versions v LEFT JOIN batches b ON b.id = v.batch_id
                 WHERE v.photo_id = ?1 ORDER BY v.version",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([photo], |r| {
                let author: String = r.get(1)?;
                let recipe: String = r.get(5)?;
                Ok(VersionInfo {
                    version: r.get(0)?,
                    author: author.parse().unwrap_or(Author::Person),
                    batch_id: r.get(2)?,
                    label: r.get(3)?,
                    at: r.get(4)?,
                    recipe: serde_json::from_str(&recipe).unwrap_or_default(),
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Photos whose Recipe, and Photos whose Verdict, changed in `batch`.
    pub fn batch_photos(&self, batch: BatchId) -> Result<(Vec<PhotoId>, Vec<PhotoId>)> {
        let ids = |sql: &str| -> Result<Vec<PhotoId>> {
            let mut stmt = self.db.prepare(sql).map_err(db_err)?;
            let rows = stmt.query_map([batch], |r| r.get(0)).map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>().map_err(db_err)
        };
        Ok((
            ids("SELECT DISTINCT photo_id FROM recipe_versions WHERE batch_id = ?1 ORDER BY photo_id")?,
            ids("SELECT DISTINCT photo_id FROM verdict_log WHERE batch_id = ?1 ORDER BY photo_id")?,
        ))
    }

    /// The newest batch, optionally only one made by `author`.
    pub fn last_batch(&self, author: Option<Author>) -> Result<Option<(BatchId, String)>> {
        self.db
            .query_row(
                "SELECT id, label FROM batches WHERE (?1 IS NULL OR author = ?1)
                 AND label NOT LIKE 'Undo:%' ORDER BY id DESC LIMIT 1",
                [author.map(|a| a.as_str())],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)
    }

    /// Put back every Recipe, Verdict, Rating and Look as it was before `batch`. Changes made
    /// after the batch to the same Photos are overwritten, as with Lightroom's history.
    /// All in one transaction: a crash midway leaves the batch fully in place.
    pub fn undo_batch(&mut self, batch: BatchId, author: Author) -> Result<UndoReport> {
        self.atomic(|lib| lib.undo_batch_now(batch, author))
    }

    fn undo_batch_now(&mut self, batch: BatchId, author: Author) -> Result<UndoReport> {
        let label: String = self
            .db
            .query_row("SELECT label FROM batches WHERE id = ?1", [batch], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| Error(format!("no batch with id {batch}")))?;

        // Recipe: the Version just before the batch's first Version of each Photo.
        let firsts: Vec<(PhotoId, i64)> = {
            let mut stmt = self
                .db
                .prepare(
                    "SELECT photo_id, MIN(version) FROM recipe_versions WHERE batch_id = ?1
                     GROUP BY photo_id ORDER BY photo_id",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([batch], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        let mut recipes = Vec::new();
        let mut restore = Vec::new();
        for (photo, first) in firsts {
            let before: Option<String> = self
                .db
                .query_row(
                    "SELECT recipe FROM recipe_versions WHERE photo_id = ?1 AND version < ?2
                     ORDER BY version DESC LIMIT 1",
                    params![photo, first],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?;
            let before = match before {
                Some(json) => serde_json::from_str(&json).unwrap_or_default(),
                None => crate::recipe::Recipe::default().to_value(),
            };
            restore.push((photo, before));
            recipes.push(photo);
        }

        // Verdicts: the log entry just before the batch's first entry for each Photo.
        let vfirsts: Vec<(PhotoId, i64)> = {
            let mut stmt = self
                .db
                .prepare(
                    "SELECT photo_id, MIN(id) FROM verdict_log WHERE batch_id = ?1
                     GROUP BY photo_id ORDER BY photo_id",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([batch], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        type Prior = Option<(String, String, Option<String>)>;
        let mut vrestore: Vec<(PhotoId, Prior)> = Vec::new();
        for (photo, first) in vfirsts {
            let prior: Prior = self
                .db
                .query_row(
                    "SELECT verdict, author, reason FROM verdict_log
                     WHERE photo_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT 1",
                    params![photo, first],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(db_err)?;
            vrestore.push((photo, prior));
        }

        // Ratings: likewise, from the rating log.
        let rfirsts: Vec<(PhotoId, i64)> = {
            let mut stmt = self
                .db
                .prepare(
                    "SELECT photo_id, MIN(id) FROM rating_log WHERE batch_id = ?1
                     GROUP BY photo_id ORDER BY photo_id",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([batch], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        let mut rrestore: Vec<(PhotoId, Option<(i64, String)>)> = Vec::new();
        for (photo, first) in rfirsts {
            let prior = self
                .db
                .query_row(
                    "SELECT stars, author FROM rating_log
                     WHERE photo_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT 1",
                    params![photo, first],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            rrestore.push((photo, prior));
        }

        let new_batch = self.begin_batch(author, &format!("Undo: {label}"))?;
        for (photo, recipe) in &restore {
            self.write_recipe(*photo, recipe, author, Some(new_batch))?;
        }
        let mut verdicts = Vec::new();
        for (photo, prior) in vrestore {
            // Written directly, not through set_verdict: putting a Verdict back is not
            // the person overruling the AI, so it must not log an Override.
            let at = super::time::now();
            match &prior {
                Some((v, by, reason)) => {
                    self.db
                        .execute(
                            "INSERT OR REPLACE INTO verdicts (photo_id, verdict, author, reason, batch_id, at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![photo, v, by, reason, new_batch, at],
                        )
                        .map_err(db_err)?;
                }
                None => {
                    // Never flagged before: back to Unflagged by nobody.
                    self.db
                        .execute("DELETE FROM verdicts WHERE photo_id = ?1", [photo])
                        .map_err(db_err)?;
                }
            }
            let (v, by, reason) =
                prior.unwrap_or(("unflagged".into(), author.as_str().into(), None));
            self.db
                .execute(
                    "INSERT INTO verdict_log (photo_id, verdict, author, reason, batch_id, at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![photo, v, by, reason, new_batch, at],
                )
                .map_err(db_err)?;
            verdicts.push(photo);
        }
        let mut ratings = Vec::new();
        for (photo, prior) in rrestore {
            // Written directly for the same reason as Verdicts: no Override.
            let at = super::time::now();
            match &prior {
                Some((stars, by)) => self
                    .db
                    .execute(
                        "INSERT OR REPLACE INTO ratings (photo_id, stars, author, batch_id, at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![photo, stars, by, new_batch, at],
                    )
                    .map_err(db_err)?,
                None => self
                    .db
                    .execute("DELETE FROM ratings WHERE photo_id = ?1", [photo])
                    .map_err(db_err)?,
            };
            let (stars, by) = prior.unwrap_or((0, author.as_str().into()));
            self.db
                .execute(
                    "INSERT INTO rating_log (photo_id, stars, author, batch_id, at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![photo, stars, by, new_batch, at],
                )
                .map_err(db_err)?;
            ratings.push(photo);
        }
        let looks = self.undo_looks(batch, new_batch)?;
        Ok(UndoReport {
            undone: batch,
            label,
            batch_id: new_batch,
            recipes,
            verdicts,
            ratings,
            looks,
        })
    }
}

impl Library {
    /// Put back each Look `batch` saved: its version from before the batch, appended in
    /// `undo`, or gone if the batch created it. Returns how many Looks changed.
    fn undo_looks(&mut self, batch: BatchId, undo: BatchId) -> Result<usize> {
        let id_of = |json: &str| -> Option<String> {
            serde_json::from_str::<serde_json::Value>(json)
                .ok()?
                .get("id")?
                .as_str()
                .map(str::to_string)
        };
        let saved: Vec<(i64, i64, String)> = {
            let mut stmt = self
                .db
                .prepare("SELECT id, shoot_id, look FROM looks WHERE batch_id = ?1 ORDER BY id")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([batch], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        let mut done: Vec<(i64, String)> = Vec::new();
        for (row, shoot, json) in saved {
            let Some(look) = id_of(&json) else { continue };
            if done.contains(&(shoot, look.clone())) {
                continue;
            }
            let earlier: Vec<String> = {
                let mut stmt = self
                    .db
                    .prepare(
                        "SELECT look FROM looks WHERE shoot_id = ?1 AND id < ?2 ORDER BY id DESC",
                    )
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map(params![shoot, row], |r| r.get(0))
                    .map_err(db_err)?;
                rows.collect::<std::result::Result<_, _>>()
                    .map_err(db_err)?
            };
            match earlier
                .into_iter()
                .find(|j| id_of(j).as_deref() == Some(look.as_str()))
            {
                Some(prior) => {
                    self.db
                        .execute(
                            "INSERT INTO looks (shoot_id, look, at, batch_id) VALUES (?1, ?2, ?3, ?4)",
                            params![shoot, prior, super::time::now(), undo],
                        )
                        .map_err(db_err)?;
                }
                None => {
                    let rows: Vec<(i64, String)> = {
                        let mut stmt = self
                            .db
                            .prepare(
                                "SELECT id, look FROM looks WHERE batch_id = ?1 AND shoot_id = ?2",
                            )
                            .map_err(db_err)?;
                        let rows = stmt
                            .query_map(params![batch, shoot], |r| Ok((r.get(0)?, r.get(1)?)))
                            .map_err(db_err)?;
                        rows.collect::<std::result::Result<_, _>>()
                            .map_err(db_err)?
                    };
                    for (id, j) in rows {
                        if id_of(&j).as_deref() == Some(look.as_str()) {
                            self.db
                                .execute("DELETE FROM looks WHERE id = ?1", [id])
                                .map_err(db_err)?;
                        }
                    }
                }
            }
            done.push((shoot, look));
        }
        Ok(done.len())
    }
}
