//! Suggestions: whole Recipes the AI proposes for a Photo, kept apart from its Recipe
//! until the person accepts one.

use super::{db_err, time, Author, BatchId, Library, PhotoId, ShootId};
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub id: i64,
    pub photo_id: PhotoId,
    pub job_id: Option<i64>,
    /// Set when the Suggestion matches this reference Photo's edit ("apply to the rest").
    pub reference_id: Option<PhotoId>,
    pub rank: i64,
    pub title: String,
    pub note: String,
    pub recipe: Value,
    /// Library-relative JPEG of the Photo with this Recipe, if one was rendered.
    pub preview: Option<String>,
    /// open | accepted | dismissed
    pub status: String,
    pub at: String,
}

const COLS: &str =
    "s.id, s.photo_id, s.job_id, s.rank, s.title, s.note, s.recipe, s.preview, s.status, s.at, s.reference_id";

fn suggestion(r: &Row) -> rusqlite::Result<Suggestion> {
    let recipe: String = r.get(6)?;
    Ok(Suggestion {
        id: r.get(0)?,
        photo_id: r.get(1)?,
        job_id: r.get(2)?,
        rank: r.get(3)?,
        title: r.get(4)?,
        note: r.get(5)?,
        recipe: serde_json::from_str(&recipe).unwrap_or_default(),
        preview: r.get(7)?,
        status: r.get(8)?,
        at: r.get(9)?,
        reference_id: r.get(10)?,
    })
}

impl Library {
    /// Replace a Photo's open Suggestions of the same flavour (fresh, or matched to a
    /// reference) with new ones, rank = order given. Their preview files go too.
    pub fn replace_suggestions(
        &mut self,
        photo: PhotoId,
        job: Option<i64>,
        reference: Option<PhotoId>,
        list: &[(String, String, Value, Option<String>)],
    ) -> Result<Vec<i64>> {
        let tx = self.db.transaction().map_err(db_err)?;
        let same = "photo_id = ?1 AND status = 'open' AND (reference_id IS NULL) = (?2 IS NULL)";
        let old: Vec<String> = {
            let mut q = tx
                .prepare(&format!(
                    "SELECT preview FROM suggestions WHERE {same} AND preview IS NOT NULL"
                ))
                .map_err(db_err)?;
            let rows = q
                .query_map(params![photo, reference], |r| r.get(0))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        tx.execute(
            &format!("DELETE FROM suggestions WHERE {same}"),
            params![photo, reference],
        )
        .map_err(db_err)?;
        let at = time::now();
        let mut ids = Vec::new();
        for (i, (title, note, recipe, preview)) in list.iter().enumerate() {
            tx.execute(
                "INSERT INTO suggestions (photo_id, job_id, reference_id, rank, title, note, recipe, preview, status, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open', ?9)",
                params![photo, job, reference, i as i64 + 1, title, note, recipe.to_string(), preview, at],
            )
            .map_err(db_err)?;
            ids.push(tx.last_insert_rowid());
        }
        tx.commit().map_err(db_err)?;
        for p in old {
            let _ = std::fs::remove_file(self.root.join(p));
        }
        self.log_takes(photo, job, reference, &ids, list);
        Ok(ids)
    }

    /// Open and accepted Suggestions (or dismissed ones too, with `all`) for one Photo or a
    /// whole Shoot, best first.
    pub fn suggestions(
        &self,
        photo: Option<PhotoId>,
        shoot: Option<ShootId>,
        all: bool,
    ) -> Result<Vec<Suggestion>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {COLS} FROM suggestions s JOIN photos p ON p.id = s.photo_id
                 WHERE (?1 IS NULL OR s.photo_id = ?1) AND (?2 IS NULL OR p.shoot_id = ?2)
                   AND (?3 OR s.status != 'dismissed')
                 ORDER BY p.captured_at, s.rank"
            ))
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![photo, shoot, all], suggestion)
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    pub fn suggestion(&self, id: i64) -> Result<Suggestion> {
        self.db
            .query_row(
                &format!("SELECT {COLS} FROM suggestions s WHERE s.id = ?1"),
                [id],
                suggestion,
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| Error(format!("no Suggestion with id {id}")))
    }

    /// Save a Suggestion as the Photo's new Recipe Version (one undoable batch). The
    /// Photo's other Suggestions stay, so the person can switch.
    pub fn accept_suggestion(
        &mut self,
        id: i64,
        author: Author,
    ) -> Result<(BatchId, i64, Suggestion)> {
        let (batch, mut done) = self.accept_suggestions(&[id], author, None)?;
        let (version, s) = done.remove(0);
        Ok((batch, version, s))
    }

    /// Accept several Suggestions (one per Photo, say the first take on every selected
    /// Photo) as one undoable batch. Returns the batch and each new Version. `amount` is the
    /// take's Amount slider (1 = as proposed; see [`Self::suggestion_at`]).
    pub fn accept_suggestions(
        &mut self,
        ids: &[i64],
        author: Author,
        amount: Option<f64>,
    ) -> Result<(BatchId, Vec<(i64, Suggestion)>)> {
        let list = ids
            .iter()
            .map(|&id| self.suggestion(id))
            .collect::<Result<Vec<_>>>()?;
        let label = match list.as_slice() {
            [one] => format!("Suggestion: {}", one.title),
            many => format!("Suggestions on {} Photos", many.len()),
        };
        let batch = self.begin_batch(author, &label)?;
        let mut out = Vec::new();
        for s in list {
            let recipe = match amount {
                Some(a) if a != 1.0 => self.suggestion_at(&s, a)?,
                _ => s.recipe.clone(),
            };
            let version = self.write_recipe(s.photo_id, &recipe, Author::Ai, Some(batch))?;
            // Only the newest accepted one reads as accepted; an earlier pick is open again.
            self.db
                .execute(
                    "UPDATE suggestions SET status = CASE WHEN id = ?1 THEN 'accepted' ELSE 'open' END
                     WHERE photo_id = ?2 AND (id = ?1 OR status = 'accepted')",
                    params![s.id, s.photo_id],
                )
                .map_err(db_err)?;
            self.log_usage(
                s.photo_id,
                "take_accepted",
                serde_json::json!({ "suggestion_id": s.id, "version": version, "batch_id": batch, "amount": amount.unwrap_or(1.0) }),
            );
            out.push((version, s));
        }
        Ok((batch, out))
    }

    /// A Suggestion's Recipe at `amount` of the way from the Photo's current Recipe, like
    /// the Amount slider on a Lightroom preset (0 = the Photo as it is, 1 = the take, up to
    /// 1.5 pushes past it).
    pub fn suggestion_at(&self, s: &Suggestion, amount: f64) -> Result<serde_json::Value> {
        use crate::recipe::Recipe;
        let to = Recipe::from_value(&s.recipe)?;
        let from = match self.read_recipe(s.photo_id)? {
            Some(v) => Recipe::from_value(&v.recipe)?,
            None => Recipe::default(),
        };
        Ok(Recipe::blend(&from, &to, amount.clamp(0.0, 1.5)).to_value())
    }

    pub fn dismiss_suggestion(&self, id: i64) -> Result<()> {
        if let Ok(s) = self.suggestion(id) {
            self.log_usage(
                s.photo_id,
                "take_dismissed",
                serde_json::json!({ "suggestion_id": id }),
            );
        }
        let n = self
            .db
            .execute(
                "UPDATE suggestions SET status = 'dismissed' WHERE id = ?1",
                [id],
            )
            .map_err(db_err)?;
        if n == 0 {
            return Err(Error(format!("no Suggestion with id {id}")));
        }
        Ok(())
    }

    /// Log each new take with what made it: the job's kind and args, its Provider and
    /// model, the Argus version (which pins the prompt and loop) and how long it took.
    fn log_takes(
        &self,
        photo: PhotoId,
        job: Option<i64>,
        reference: Option<PhotoId>,
        ids: &[i64],
        list: &[(String, String, Value, Option<String>)],
    ) {
        if !self.recording_usage() {
            return;
        }
        let job = job.and_then(|j| self.job(j).ok());
        let provider = job
            .as_ref()
            .and_then(|j| j.args.get("provider").and_then(Value::as_str));
        let (provider, model) = crate::usage::provider_model(provider);
        let seconds = job
            .as_ref()
            .and_then(|j| time::seconds_between(j.started_at.as_deref()?, &time::now()));
        for (id, (title, note, recipe, _)) in ids.iter().zip(list) {
            self.log_usage(
                photo,
                "take",
                serde_json::json!({
                    "source": if reference.is_some() { "match" } else { "suggestion" },
                    "suggestion_id": id, "title": title, "note": note, "recipe": recipe,
                    "reference_id": reference, "job_id": job.as_ref().map(|j| j.id),
                    "job_kind": job.as_ref().map(|j| &j.kind),
                    "job_args": job.as_ref().map(|j| &j.args),
                    "provider": provider, "model": model,
                    "argus": env!("CARGO_PKG_VERSION"), "seconds": seconds,
                }),
            );
        }
    }
}
