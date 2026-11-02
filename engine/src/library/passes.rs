//! Cull Rounds (GLOSSARY: Round): repeat Cull passes over the Picks (Greg, 2026-11-01: "take
//! all the picks, restack them, and let me run through them one more time"). Pass 1 is the
//! Verdicts. Pass k >= 2 runs over the Photos that reached Round k-1 and marks each kept or
//! cut. A Pick's Round is 1 plus the passes it was kept in, in a row from pass 2, so a cut
//! never takes the Pick away: it only stops the Photo going further. Marks are logged like
//! Ratings, so a batch undoes them.

use super::{db_err, time, Author, BatchId, Library, PhotoId};
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension};

/// The highest pass a mark can be for: far beyond any real cull, but a typo stays an error.
pub const MAX_PASS: u8 = 20;

/// Where a Photo stands across Rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Round {
    /// 0: not a Pick. 1: a Pick. k: kept in passes 2 to k as well.
    pub round: u8,
    /// The pass that cut it, if one did.
    pub cut_in: Option<u8>,
}

/// A Round from a Photo's Verdict and its marks (pass, kept), in any order.
pub fn round_of(pick: bool, marks: &[(u8, bool)]) -> Round {
    if !pick {
        return Round::default();
    }
    let mut round = 1u8;
    loop {
        match marks.iter().find(|(p, _)| *p == round + 1) {
            Some((_, true)) => round += 1,
            Some((p, false)) => {
                return Round {
                    round,
                    cut_in: Some(*p),
                }
            }
            None => {
                return Round {
                    round,
                    cut_in: None,
                }
            }
        }
    }
}

impl Library {
    /// A Photo's marks for pass 2 and on, as (pass, kept).
    pub(crate) fn pass_marks(&self, photo: PhotoId) -> Result<Vec<(u8, bool)>> {
        let mut q = self
            .db
            .prepare_cached("SELECT pass, kept FROM pass_marks WHERE photo_id = ?1 ORDER BY pass")
            .map_err(db_err)?;
        let rows = q
            .query_map([photo], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Every Photo's marks, for listing many Photos at once.
    pub fn all_pass_marks(&self) -> Result<std::collections::HashMap<PhotoId, Vec<(u8, bool)>>> {
        let mut q = self
            .db
            .prepare("SELECT photo_id, pass, kept FROM pass_marks")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| {
                Ok((r.get::<_, PhotoId>(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))
            })
            .map_err(db_err)?;
        let mut out: std::collections::HashMap<PhotoId, Vec<(u8, bool)>> = Default::default();
        for row in rows {
            let (id, pass, kept) = row.map_err(db_err)?;
            out.entry(id).or_default().push((pass, kept));
        }
        Ok(out)
    }

    /// Mark Photos kept (`Some(true)`), cut (`Some(false)`) or neither (`None`) in `pass`
    /// (2 or more; pass 1 is the Verdict). Unknown Photo ids are an error and nothing changes.
    pub fn set_pass_mark(
        &mut self,
        photos: &[PhotoId],
        pass: u8,
        kept: Option<bool>,
        author: Author,
        batch: Option<BatchId>,
    ) -> Result<()> {
        if !(2..=MAX_PASS).contains(&pass) {
            return Err(Error(format!(
                "pass must be 2 to {MAX_PASS} (pass 1 is the flag), not {pass}"
            )));
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
            match kept {
                Some(k) => tx.execute(
                    "INSERT OR REPLACE INTO pass_marks (photo_id, pass, kept, author, batch_id, at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, pass, k, author.as_str(), batch, at],
                ),
                None => tx.execute(
                    "DELETE FROM pass_marks WHERE photo_id = ?1 AND pass = ?2",
                    params![id, pass],
                ),
            }
            .map_err(db_err)?;
            tx.execute(
                "INSERT INTO pass_log (photo_id, pass, kept, author, batch_id, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, pass, kept, author.as_str(), batch, at],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        for &id in photos {
            self.log_usage(
                id,
                "pass",
                serde_json::json!({ "pass": pass, "kept": kept, "author": author.as_str(), "batch_id": batch }),
            );
        }
        Ok(())
    }

    /// Put each pass mark `batch` changed back as it was before, logged in `undo`. Returns the
    /// Photos whose marks went back.
    pub(crate) fn undo_pass_marks(
        &mut self,
        batch: BatchId,
        undo: BatchId,
        author: Author,
    ) -> Result<Vec<PhotoId>> {
        let firsts: Vec<(PhotoId, u8, i64)> = {
            let mut q = self
                .db
                .prepare(
                    "SELECT photo_id, pass, MIN(id) FROM pass_log WHERE batch_id = ?1
                     GROUP BY photo_id, pass ORDER BY photo_id, pass",
                )
                .map_err(db_err)?;
            let rows = q
                .query_map([batch], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        let mut photos = Vec::new();
        for (photo, pass, first) in firsts {
            let prior: Option<(Option<bool>, String)> = self
                .db
                .query_row(
                    "SELECT kept, author FROM pass_log
                     WHERE photo_id = ?1 AND pass = ?2 AND id < ?3 ORDER BY id DESC LIMIT 1",
                    params![photo, pass, first],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            let (kept, by) = prior.unwrap_or((None, author.as_str().into()));
            let at = time::now();
            match kept {
                Some(k) => self.db.execute(
                    "INSERT OR REPLACE INTO pass_marks (photo_id, pass, kept, author, batch_id, at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![photo, pass, k, by, undo, at],
                ),
                None => self.db.execute(
                    "DELETE FROM pass_marks WHERE photo_id = ?1 AND pass = ?2",
                    params![photo, pass],
                ),
            }
            .map_err(db_err)?;
            self.db
                .execute(
                    "INSERT INTO pass_log (photo_id, pass, kept, author, batch_id, at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![photo, pass, kept, by, undo, at],
                )
                .map_err(db_err)?;
            if photos.last() != Some(&photo) {
                photos.push(photo);
            }
        }
        Ok(photos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_counts_passes_kept_in_a_row_and_stops_at_the_first_cut() {
        assert_eq!(round_of(false, &[(2, true)]), Round::default());
        assert_eq!(
            round_of(true, &[]),
            Round {
                round: 1,
                cut_in: None
            }
        );
        assert_eq!(
            round_of(true, &[(3, true), (2, true)]),
            Round {
                round: 3,
                cut_in: None
            }
        );
        assert_eq!(
            round_of(true, &[(2, true), (3, false)]),
            Round {
                round: 2,
                cut_in: Some(3)
            }
        );
        // A mark past a gap doesn't count: pass 3 only ran over Round 2.
        assert_eq!(
            round_of(true, &[(3, true)]),
            Round {
                round: 1,
                cut_in: None
            }
        );
    }
}
