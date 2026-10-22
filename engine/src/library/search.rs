//! Finding Photos by words: every word of the query must match the start of a word in the
//! Photo's Keywords (kept or suggested), its caption, its file name or its Shoot's name.
//! Kept Keywords count most. Meaning search (a text-image model) adds to this later; the
//! words stay the part a person can predict.

use super::{db_err, Library, PhotoId, ShootId};
use crate::Result;
use serde::Serialize;
use std::collections::HashMap;

/// One Photo a search found, best first, with the words that found it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hit {
    pub photo_id: PhotoId,
    pub shoot_id: ShootId,
    pub score: f32,
    /// What matched, for the palette's caption: Keyword leaves first, then "caption".
    pub matched: Vec<String>,
}

struct Doc {
    shoot_id: ShootId,
    /// (text, weight, label when it matches): each Keyword level, the caption, the file
    /// name, the Shoot name.
    fields: Vec<(String, f32, String)>,
}

impl Library {
    /// Photos whose words match every word of `query`, best first (ties in capture order),
    /// optionally within one Shoot.
    pub fn search_words(&self, query: &str, shoot: Option<ShootId>) -> Result<Vec<Hit>> {
        let terms = words(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let mut docs: Vec<(PhotoId, Doc)> = Vec::new();
        let mut at: HashMap<PhotoId, usize> = HashMap::new();
        {
            let mut q = self
                .db
                .prepare(
                    "SELECT p.id, p.shoot_id, p.file_name, s.name FROM photos p JOIN shoots s ON s.id = p.shoot_id
                     WHERE (?1 IS NULL OR p.shoot_id = ?1) ORDER BY p.captured_at, p.file_name",
                )
                .map_err(db_err)?;
            let rows = q
                .query_map([shoot], |r| {
                    Ok((
                        r.get::<_, PhotoId>(0)?,
                        r.get::<_, ShootId>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (id, shoot_id, file, shoot_name) = row.map_err(db_err)?;
                at.insert(id, docs.len());
                docs.push((
                    id,
                    Doc {
                        shoot_id,
                        fields: vec![
                            (file.clone(), 0.5, file),
                            (shoot_name.replace(['_', '-'], " "), 0.5, shoot_name),
                        ],
                    },
                ));
            }
        }
        {
            let mut q = self
                .db
                .prepare("SELECT photo_id, keyword, state FROM photo_keywords WHERE state != 'dismissed'")
                .map_err(db_err)?;
            let rows = q
                .query_map([], |r| {
                    Ok((
                        r.get::<_, PhotoId>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (id, keyword, state) = row.map_err(db_err)?;
                let Some(&i) = at.get(&id) else { continue };
                let weight = if state == "kept" { 3.0 } else { 2.0 };
                let doc = &mut docs[i].1;
                let levels: Vec<&str> = keyword.split('|').skip(1).collect();
                for (n, level) in levels.iter().enumerate() {
                    // The leaf is what the Keyword is about; its parents count a little less.
                    let w = if n + 1 == levels.len() {
                        weight
                    } else {
                        weight * 0.6
                    };
                    doc.fields.push((level.to_string(), w, level.to_string()));
                }
            }
        }
        {
            let mut q = self
                .db
                .prepare("SELECT photo_id, text FROM captions")
                .map_err(db_err)?;
            let rows = q
                .query_map([], |r| {
                    Ok((r.get::<_, PhotoId>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(db_err)?;
            for row in rows {
                let (id, text) = row.map_err(db_err)?;
                if let Some(&i) = at.get(&id) {
                    docs[i].1.fields.push((text, 1.0, "caption".into()));
                }
            }
        }
        let mut hits = Vec::new();
        for (id, doc) in docs {
            let mut score = 0.0;
            let mut matched: Vec<String> = Vec::new();
            let mut all = true;
            for w in &terms {
                let best = doc
                    .fields
                    .iter()
                    .filter(|(text, _, _)| words(text).iter().any(|t| t.starts_with(w.as_str())))
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                match best {
                    Some((_, weight, label)) => {
                        score += weight;
                        if !matched.contains(label) {
                            matched.push(label.clone());
                        }
                    }
                    None => {
                        all = false;
                        break;
                    }
                }
            }
            if all {
                hits.push(Hit {
                    photo_id: id,
                    shoot_id: doc.shoot_id,
                    score,
                    matched,
                });
            }
        }
        // Stable, so equal scores keep capture order.
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(hits)
    }
}

/// Lowercase words, split on anything that isn't a letter or digit.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A saved search, shown in the sidebar above the Shoots.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Album {
    pub id: i64,
    pub name: String,
    /// What it finds: `{"text": "autumn friends"}`.
    pub query: serde_json::Value,
    pub created_at: String,
}

impl Library {
    /// Every Album, oldest first.
    pub fn albums(&self) -> Result<Vec<Album>> {
        let mut q = self
            .db
            .prepare("SELECT id, name, query, created_at FROM albums ORDER BY id")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, query, created_at) = row.map_err(db_err)?;
            out.push(Album {
                id,
                name,
                query: serde_json::from_str(&query).unwrap_or_default(),
                created_at,
            });
        }
        Ok(out)
    }

    /// Save a search as an Album; returns it.
    pub fn save_album(&mut self, name: &str, query: &serde_json::Value) -> Result<Album> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::Error("an Album needs a name".into()));
        }
        let at = super::time::now();
        self.db
            .execute(
                "INSERT INTO albums (name, query, created_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![name, query.to_string(), at],
            )
            .map_err(db_err)?;
        let id = self.db.last_insert_rowid();
        Ok(Album {
            id,
            name: name.to_string(),
            query: query.clone(),
            created_at: at,
        })
    }

    /// Rename an Album, or change what it finds.
    pub fn edit_album(
        &mut self,
        id: i64,
        name: Option<&str>,
        query: Option<&serde_json::Value>,
    ) -> Result<()> {
        if name.is_some_and(|n| n.trim().is_empty()) {
            return Err(crate::Error("an Album needs a name".into()));
        }
        let n = self
            .db
            .execute(
                "UPDATE albums SET name = COALESCE(?2, name), query = COALESCE(?3, query) WHERE id = ?1",
                rusqlite::params![id, name.map(str::trim), query.map(|q| q.to_string())],
            )
            .map_err(db_err)?;
        if n == 0 {
            return Err(crate::Error(format!("no Album with id {id}")));
        }
        Ok(())
    }

    /// Delete an Album (its Photos stay where they are).
    pub fn delete_album(&mut self, id: i64) -> Result<()> {
        let n = self
            .db
            .execute("DELETE FROM albums WHERE id = ?1", [id])
            .map_err(db_err)?;
        if n == 0 {
            return Err(crate::Error(format!("no Album with id {id}")));
        }
        Ok(())
    }
}
