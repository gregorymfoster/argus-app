//! Keywords and captions on each Photo (docs/naming.md). Keywords are hierarchical paths
//! under a Facet, Lightroom style (`Places|Portugal|Lisbon`); the caption is one or two
//! sentences (IPTC Description). The AI only suggests: its Keywords and caption stay
//! `suggested` until the person keeps them, and a dismissed Keyword is remembered so the
//! AI doesn't offer it again. The person's own always win.

use super::{db_err, time, Author, Library, PhotoId};
use crate::naming::Keyword;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// One Keyword on a Photo, as the loupe's Info panel shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PhotoKeyword {
    /// The full path: `Places|Portugal|Lisbon`.
    pub keyword: String,
    /// `People`, `Places`, `Events`, `Activities` or `Subjects`.
    pub facet: String,
    /// The narrowest level: `Lisbon`.
    pub leaf: String,
    pub author: Author,
    /// True until the person keeps an AI suggestion.
    pub suggested: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Caption {
    pub text: String,
    pub author: Author,
    pub suggested: bool,
}

/// A Photo's caption and Keywords (dismissed ones left out), Facets in order.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Tags {
    pub photo_id: PhotoId,
    pub caption: Option<Caption>,
    pub keywords: Vec<PhotoKeyword>,
}

/// A Keyword in use across the Library and how many Photos keep it, for autocomplete.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KeywordCount {
    pub keyword: String,
    pub photos: i64,
}

/// What a person's change did, per Keyword.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct TagReport {
    pub changed: usize,
}

impl Library {
    /// Caption and Keywords of each Photo, in the order asked. Unknown ids are an error.
    pub fn tags(&self, photos: &[PhotoId]) -> Result<Vec<Tags>> {
        let mut kw = self
            .db
            .prepare(
                "SELECT keyword, author, state FROM photo_keywords
                 WHERE photo_id = ?1 AND state != 'dismissed' ORDER BY keyword COLLATE NOCASE",
            )
            .map_err(db_err)?;
        let mut cap = self
            .db
            .prepare("SELECT text, author, state FROM captions WHERE photo_id = ?1")
            .map_err(db_err)?;
        photos
            .iter()
            .map(|&id| {
                check_photo(&self.db, id)?;
                let rows = kw
                    .query_map([id], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(db_err)?;
                let mut keywords = Vec::new();
                for row in rows {
                    let (path, author, state) = row.map_err(db_err)?;
                    let Some(k) = Keyword::parse(&path) else {
                        continue;
                    };
                    keywords.push(PhotoKeyword {
                        keyword: k.path(),
                        facet: k.facet.name().to_string(),
                        leaf: k.leaf().to_string(),
                        author: author.parse()?,
                        suggested: state == "suggested",
                    });
                }
                // Facets in their fixed order (People first), then by path.
                keywords.sort_by_key(|k| {
                    crate::naming::Facet::ALL
                        .iter()
                        .position(|f| f.name() == k.facet)
                });
                let caption = cap
                    .query_row([id], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    })
                    .optional()
                    .map_err(db_err)?
                    .map(|(text, author, state)| -> Result<Caption> {
                        Ok(Caption {
                            text,
                            author: author.parse()?,
                            suggested: state == "suggested",
                        })
                    })
                    .transpose()?;
                Ok(Tags {
                    photo_id: id,
                    caption,
                    keywords,
                })
            })
            .collect()
    }

    /// The Keywords a Photo keeps (no suggestions), for XMP and search.
    pub fn kept_keywords(&self, photo: PhotoId) -> Result<Vec<Keyword>> {
        let mut q = self
            .db
            .prepare(
                "SELECT keyword FROM photo_keywords WHERE photo_id = ?1 AND state = 'kept'
                 ORDER BY keyword COLLATE NOCASE",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map([photo], |r| r.get::<_, String>(0))
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.extend(Keyword::parse(&row.map_err(db_err)?));
        }
        Ok(out)
    }

    /// The caption a Photo keeps, if any.
    pub fn kept_caption(&self, photo: PhotoId) -> Result<Option<String>> {
        self.db
            .query_row(
                "SELECT text FROM captions WHERE photo_id = ?1 AND state = 'kept'",
                [photo],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)
    }

    /// Add Keywords to Photos as the person: kept at once, over any suggestion or dismissal.
    pub fn add_keywords(&mut self, photos: &[PhotoId], keywords: &[Keyword]) -> Result<TagReport> {
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        let mut report = TagReport::default();
        for &id in photos {
            check_photo(&tx, id)?;
            for k in keywords {
                report.changed += tx
                    .execute(
                        "INSERT INTO photo_keywords (photo_id, keyword, author, state, at)
                         VALUES (?1, ?2, 'person', 'kept', ?3)
                         ON CONFLICT (photo_id, keyword) DO UPDATE SET state = 'kept', at = ?3
                         WHERE state != 'kept'",
                        params![id, k.path(), at],
                    )
                    .map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(report)
    }

    /// Take Keywords off Photos as the person. An AI Keyword is remembered as dismissed so
    /// the AI doesn't suggest it again; the person's own is simply gone.
    pub fn remove_keywords(
        &mut self,
        photos: &[PhotoId],
        keywords: &[Keyword],
    ) -> Result<TagReport> {
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        let mut report = TagReport::default();
        for &id in photos {
            check_photo(&tx, id)?;
            for k in keywords {
                let path = k.path();
                report.changed += tx
                    .execute(
                        "DELETE FROM photo_keywords WHERE photo_id = ?1 AND keyword = ?2 AND author = 'person'",
                        params![id, path],
                    )
                    .map_err(db_err)?;
                report.changed += tx
                    .execute(
                        "UPDATE photo_keywords SET state = 'dismissed', at = ?3
                         WHERE photo_id = ?1 AND keyword = ?2 AND state != 'dismissed'",
                        params![id, path, at],
                    )
                    .map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(report)
    }

    /// Keep the AI's suggestions on Photos: these Keywords, or all of them and the caption
    /// when `keywords` is None (Accept all).
    pub fn accept_tags(
        &mut self,
        photos: &[PhotoId],
        keywords: Option<&[Keyword]>,
    ) -> Result<TagReport> {
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        let mut report = TagReport::default();
        for &id in photos {
            check_photo(&tx, id)?;
            match keywords {
                Some(ks) => {
                    for k in ks {
                        report.changed += tx
                            .execute(
                                "UPDATE photo_keywords SET state = 'kept', at = ?3
                                 WHERE photo_id = ?1 AND keyword = ?2 AND state = 'suggested'",
                                params![id, k.path(), at],
                            )
                            .map_err(db_err)?;
                    }
                }
                None => {
                    report.changed += tx
                        .execute(
                            "UPDATE photo_keywords SET state = 'kept', at = ?2
                             WHERE photo_id = ?1 AND state = 'suggested'",
                            params![id, at],
                        )
                        .map_err(db_err)?;
                    report.changed += tx
                        .execute(
                            "UPDATE captions SET state = 'kept', at = ?2
                             WHERE photo_id = ?1 AND state = 'suggested'",
                            params![id, at],
                        )
                        .map_err(db_err)?;
                }
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(report)
    }

    /// Set the caption of Photos as the person (kept); empty text removes it.
    pub fn set_caption(&mut self, photos: &[PhotoId], text: &str) -> Result<TagReport> {
        let text = text.trim();
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        let mut report = TagReport::default();
        for &id in photos {
            check_photo(&tx, id)?;
            report.changed += if text.is_empty() {
                tx.execute("DELETE FROM captions WHERE photo_id = ?1", [id])
            } else {
                tx.execute(
                    "INSERT INTO captions (photo_id, text, author, state, at) VALUES (?1, ?2, 'person', 'kept', ?3)
                     ON CONFLICT (photo_id) DO UPDATE SET text = ?2, author = 'person', state = 'kept', at = ?3",
                    params![id, text, at],
                )
            }
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(report)
    }

    /// The AI's caption and Keywords for one Photo, replacing its earlier suggestions.
    /// Nothing the person kept, added or dismissed changes; a caption is only suggested
    /// when the Photo has no kept one. People are never the AI's to name, so Keywords
    /// under People are dropped. Returns how many Keywords were suggested.
    pub fn suggest_tags(
        &mut self,
        photo: PhotoId,
        caption: Option<&str>,
        keywords: &[Keyword],
    ) -> Result<usize> {
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        check_photo(&tx, photo)?;
        tx.execute(
            "DELETE FROM photo_keywords WHERE photo_id = ?1 AND state = 'suggested'",
            [photo],
        )
        .map_err(db_err)?;
        let mut n = 0;
        for k in keywords
            .iter()
            .filter(|k| k.facet != crate::naming::Facet::People)
        {
            n += tx
                .execute(
                    "INSERT INTO photo_keywords (photo_id, keyword, author, state, at)
                     VALUES (?1, ?2, 'ai', 'suggested', ?3) ON CONFLICT DO NOTHING",
                    params![photo, k.path(), at],
                )
                .map_err(db_err)?;
        }
        if let Some(text) = caption.map(str::trim).filter(|t| !t.is_empty()) {
            tx.execute(
                "INSERT INTO captions (photo_id, text, author, state, at) VALUES (?1, ?2, 'ai', 'suggested', ?3)
                 ON CONFLICT (photo_id) DO UPDATE SET text = ?2, author = 'ai', at = ?3 WHERE state = 'suggested'",
                params![photo, text, at],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(n)
    }

    /// Whether the AI has tagged this Photo before (any AI Keyword or caption, in any state).
    pub fn has_ai_tags(&self, photo: PhotoId) -> Result<bool> {
        self.db
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM photo_keywords WHERE photo_id = ?1 AND author = 'ai')
                     OR EXISTS (SELECT 1 FROM captions WHERE photo_id = ?1 AND author = 'ai')",
                [photo],
                |r| r.get(0),
            )
            .map_err(db_err)
    }

    /// Kept Keywords across the Library, most used first; with `prefix`, only those whose
    /// path or any level starts with it (case-insensitive), for autocomplete.
    pub fn keyword_counts(&self, prefix: Option<&str>, limit: usize) -> Result<Vec<KeywordCount>> {
        let mut q = self
            .db
            .prepare(
                "SELECT keyword, COUNT(*) FROM photo_keywords WHERE state = 'kept'
                 GROUP BY keyword ORDER BY COUNT(*) DESC, keyword COLLATE NOCASE",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| {
                Ok(KeywordCount {
                    keyword: r.get(0)?,
                    photos: r.get(1)?,
                })
            })
            .map_err(db_err)?;
        let p = prefix
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty());
        let mut out = Vec::new();
        for row in rows {
            let k = row.map_err(db_err)?;
            let hit = match &p {
                None => true,
                Some(p) => {
                    let lower = k.keyword.to_lowercase();
                    lower.starts_with(p.as_str())
                        || lower.split('|').any(|l| l.starts_with(p.as_str()))
                }
            };
            if hit {
                out.push(k);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }
}

fn check_photo(tx: &Connection, id: PhotoId) -> Result<()> {
    let exists = tx
        .query_row("SELECT 1 FROM photos WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map_err(db_err)?
        .is_some();
    exists
        .then_some(())
        .ok_or_else(|| Error(format!("no Photo with id {id}")))
}
