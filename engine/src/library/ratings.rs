//! Star Ratings (Lightroom's 0-5 stars) and a Shoot's Best Photos. Ratings work like
//! Verdicts: the AI suggests, the person's own Rating always wins, and every change is
//! logged so a batch undoes together. Best Photos are the AI's short list of a Shoot's
//! strongest frames, replaced by each cull like Stacks.

use super::{db_err, time, Author, BatchId, Library, PhotoId, ShootId};
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

/// One entry of a Shoot's Best Photos, best first.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BestPhoto {
    pub photo_id: PhotoId,
    pub rank: i64,
}

/// A Photo's Verdict and star Rating and who set each: what the UI shows on a tile.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CullState {
    pub id: PhotoId,
    pub verdict: String,
    pub verdict_by: Option<String>,
    pub stars: u8,
    pub stars_by: Option<String>,
}

impl Library {
    /// The Verdict and Rating of each Photo, in the order asked. Unknown ids are an error.
    pub fn cull_states(&self, photos: &[PhotoId]) -> Result<Vec<CullState>> {
        let mut q = self
            .db
            .prepare(
                "SELECT p.id, COALESCE(v.verdict, 'unflagged'), v.author, COALESCE(r.stars, 0), r.author
                 FROM photos p LEFT JOIN verdicts v ON v.photo_id = p.id
                 LEFT JOIN ratings r ON r.photo_id = p.id WHERE p.id = ?1",
            )
            .map_err(db_err)?;
        photos
            .iter()
            .map(|&id| {
                q.query_row([id], |r| {
                    Ok(CullState {
                        id: r.get(0)?,
                        verdict: r.get(1)?,
                        verdict_by: r.get(2)?,
                        stars: r.get(3)?,
                        stars_by: r.get(4)?,
                    })
                })
                .optional()
                .map_err(db_err)?
                .ok_or_else(|| Error(format!("no Photo with id {id}")))
            })
            .collect()
    }

    /// Set one star Rating (0-5) on many Photos. A person changing an AI Rating is logged
    /// as an Override. Unknown Photo ids are an error and nothing is changed.
    pub fn set_rating(
        &mut self,
        photos: &[PhotoId],
        stars: u8,
        author: Author,
        batch: Option<BatchId>,
    ) -> Result<()> {
        if stars > 5 {
            return Err(Error(format!("stars must be 0 to 5, not {stars}")));
        }
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        for &id in photos {
            let exists = tx
                .query_row("SELECT 1 FROM photos WHERE id = ?1", [id], |_| Ok(()))
                .optional()
                .map_err(db_err)?
                .is_some();
            if !exists {
                return Err(Error(format!("no Photo with id {id}")));
            }
            let prev: Option<(u8, String)> = tx
                .query_row(
                    "SELECT stars, author FROM ratings WHERE photo_id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            if let Some((ai_stars, prev_author)) = prev {
                if author == Author::Person && prev_author == "ai" && ai_stars != stars {
                    tx.execute(
                        "INSERT INTO overrides (photo_id, field, ai_value, person_value, at)
                         VALUES (?1, 'rating', ?2, ?3, ?4)",
                        params![id, ai_stars.to_string(), stars.to_string(), at],
                    )
                    .map_err(db_err)?;
                }
            }
            tx.execute(
                "INSERT OR REPLACE INTO ratings (photo_id, stars, author, batch_id, at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, stars, author.as_str(), batch, at],
            )
            .map_err(db_err)?;
            tx.execute(
                "INSERT INTO rating_log (photo_id, stars, author, batch_id, at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, stars, author.as_str(), batch, at],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        for &id in photos {
            self.log_usage(
                id,
                "stars",
                serde_json::json!({ "stars": stars, "author": author.as_str(), "batch_id": batch }),
            );
        }
        Ok(())
    }

    /// Replace a Shoot's Best Photos with `photos`, best first. Photos not in the Shoot
    /// are an error and nothing changes.
    pub fn replace_best_photos(&mut self, shoot: ShootId, photos: &[PhotoId]) -> Result<()> {
        let tx = self.db.transaction().map_err(db_err)?;
        tx.execute("DELETE FROM best_photos WHERE shoot_id = ?1", [shoot])
            .map_err(db_err)?;
        for (i, &photo) in photos.iter().enumerate() {
            let in_shoot: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM photos WHERE id = ?1 AND shoot_id = ?2",
                    [photo, shoot],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if in_shoot == 0 {
                return Err(Error(format!("Photo {photo} is not in Shoot {shoot}")));
            }
            tx.execute(
                "INSERT OR REPLACE INTO best_photos (photo_id, shoot_id, rank) VALUES (?1, ?2, ?3)",
                params![photo, shoot, i as i64 + 1],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    /// A Shoot's Best Photos, best first, leaving out any the person has since Rejected.
    pub fn best_photos(&self, shoot: ShootId) -> Result<Vec<BestPhoto>> {
        let mut q = self
            .db
            .prepare(
                "SELECT b.photo_id, b.rank FROM best_photos b
                 LEFT JOIN verdicts v ON v.photo_id = b.photo_id
                 WHERE b.shoot_id = ?1
                   AND NOT (COALESCE(v.verdict, '') = 'reject' AND v.author = 'person')
                 ORDER BY b.rank",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map([shoot], |r| {
                Ok(BestPhoto {
                    photo_id: r.get(0)?,
                    rank: r.get(1)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }
}
