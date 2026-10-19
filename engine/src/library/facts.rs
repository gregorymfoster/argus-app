//! The Facts table: rows the Analyzers write and read (ADR 0013). Only
//! [`crate::analyzers`] calls these; everyone else asks it for Facts.

use super::{db_err, time, Library, PhotoId, CACHE_DIR};
use crate::Result;
use rusqlite::{params, OptionalExtension};
use std::path::PathBuf;

/// One stored Fact, as the store keeps it.
#[derive(Debug, Clone)]
pub struct FactRow {
    pub version: u32,
    pub input_hash: String,
    pub value: serde_json::Value,
    pub blob: Option<Vec<u8>>,
}

/// Blobs larger than this go to a file in `cache/facts/` so `library.db` and its daily
/// Backups stay small.
const INLINE_BLOB: usize = 64 * 1024;

impl Library {
    pub(crate) fn fact(&self, photo: PhotoId, analyzer: &str) -> Result<Option<FactRow>> {
        let row = self
            .db
            .query_row(
                "SELECT version, input_hash, value, blob, blob_file FROM facts
                 WHERE photo_id = ?1 AND analyzer = ?2",
                params![photo, analyzer],
                |r| {
                    Ok((
                        r.get::<_, u32>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<Vec<u8>>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(db_err)?;
        let Some((version, input_hash, value, blob, file)) = row else {
            return Ok(None);
        };
        let blob = match file {
            // A blob file someone cleared makes the Fact missing, so it's recomputed.
            Some(f) => match std::fs::read(self.root.join(f)) {
                Ok(b) => Some(b),
                Err(_) => return Ok(None),
            },
            None => blob,
        };
        let Ok(value) = serde_json::from_str(&value) else {
            return Ok(None);
        };
        Ok(Some(FactRow {
            version,
            input_hash,
            value,
            blob,
        }))
    }

    pub(crate) fn put_fact(&self, photo: PhotoId, analyzer: &str, row: &FactRow) -> Result<()> {
        let (blob, file) = match &row.blob {
            Some(b) if b.len() > INLINE_BLOB => {
                let rel = PathBuf::from(CACHE_DIR)
                    .join("facts")
                    .join(analyzer)
                    .join(format!("{photo}.bin"));
                let path = self.root.join(&rel);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| crate::Error(format!("facts folder: {e}")))?;
                }
                std::fs::write(&path, b)
                    .map_err(|e| crate::Error(format!("write {}: {e}", path.display())))?;
                (None, Some(rel.to_string_lossy().replace('\\', "/")))
            }
            b => (b.as_deref(), None),
        };
        self.db
            .execute(
                "INSERT OR REPLACE INTO facts
                 (photo_id, analyzer, version, input_hash, value, blob, blob_file, computed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    photo,
                    analyzer,
                    row.version,
                    row.input_hash,
                    row.value.to_string(),
                    blob,
                    file,
                    time::now()
                ],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Drop every Fact (they are a cache); their blob files go with `cache/`.
    pub(crate) fn clear_facts(&self) -> Result<()> {
        self.db.execute("DELETE FROM facts", []).map_err(db_err)?;
        Ok(())
    }
}
