//! Renaming a Shoot. A Shoot's name is also the name of the folder Import copied its
//! originals into (`originals/2026-10-18_Boat-Trip`, docs/naming.md), so a rename moves
//! that folder too and rewrites the Photos' relative paths, keeping Argus and Finder in
//! step (ADR 0011, "Renaming a Shoot").

use super::{db_err, Library, ShootId, ORIGINALS_DIR};
use crate::{naming, Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct RenameReport {
    pub shoot_id: ShootId,
    /// The stored name, by the naming rules: `2026-10-10_Boat-Tour`.
    pub name: String,
    /// The Shoot's originals folder after the rename, relative to the Library, when its
    /// originals live inside the Library.
    pub originals: Option<String>,
}

impl Library {
    /// Rename a Shoot. `title` is what the person typed ("Boat tour"); it becomes
    /// `<date>_Boat-tour`, keeping the Shoot's date unless `title` starts with a new one.
    /// When Import copied the Shoot's originals into the Library, their folder is renamed
    /// to match (with `-2` if that name is taken) and the Photos' paths follow in one
    /// transaction; if the database can't be written the folder is moved back.
    pub fn rename_shoot(&mut self, shoot: ShootId, title: &str) -> Result<RenameReport> {
        let (old, first): (String, Option<String>) = self
            .db
            .query_row(
                "SELECT s.name, MIN(p.captured_at) FROM shoots s
                 LEFT JOIN photos p ON p.shoot_id = s.id WHERE s.id = ?1 GROUP BY s.id",
                [shoot],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| Error(format!("no Shoot {shoot}")))?;
        if title.trim().is_empty() {
            return Err(Error("a Shoot needs a name".into()));
        }
        let date = naming::parse_shoot_name(&old).date.or(first);
        let name = naming::shoot_slug(title, date.as_deref());

        let folder = self.own_originals_folder(shoot)?;
        let mut moved: Option<(PathBuf, PathBuf)> = None;
        let mut new_folder = folder.clone();
        if let Some(old_rel) = &folder {
            let old_leaf = old_rel.trim_start_matches(&format!("{ORIGINALS_DIR}/"));
            let base = self.root.join(ORIGINALS_DIR);
            // A case-only change (`Boat-trip` -> `Boat-Trip`) isn't a clash with itself on
            // macOS's case-insensitive disks.
            let leaf = naming::unique_name(&name, |n| {
                !n.eq_ignore_ascii_case(old_leaf) && base.join(n).exists()
            });
            let from = self.root.join(old_rel);
            if leaf != old_leaf && from.is_dir() {
                let to = base.join(&leaf);
                std::fs::rename(&from, &to).map_err(|e| {
                    Error(format!(
                        "rename {} to {}: {e}",
                        from.display(),
                        to.display()
                    ))
                })?;
                moved = Some((from, to));
                new_folder = Some(format!("{ORIGINALS_DIR}/{leaf}"));
            }
        }

        let mut write = || -> Result<()> {
            let tx = self.db.transaction().map_err(db_err)?;
            tx.execute(
                "UPDATE shoots SET name = ?1 WHERE id = ?2",
                params![name, shoot],
            )
            .map_err(db_err)?;
            if let (Some(_), Some(from), Some(to)) = (&moved, &folder, &new_folder) {
                let (from, to) = (format!("{from}/"), format!("{to}/"));
                for col in ["path", "jpeg"] {
                    tx.execute(
                        &format!(
                            "UPDATE photos SET {col} = ?1 || substr({col}, ?2)
                             WHERE shoot_id = ?3 AND substr({col}, 1, ?4) = ?5"
                        ),
                        params![to, from.len() as i64 + 1, shoot, from.len() as i64, from],
                    )
                    .map_err(db_err)?;
                }
            }
            tx.commit().map_err(db_err)
        };
        if let Err(e) = write() {
            if let Some((from, to)) = &moved {
                let _ = std::fs::rename(to, from);
            }
            return Err(e);
        }
        Ok(RenameReport {
            shoot_id: shoot,
            name,
            originals: new_folder,
        })
    }

    /// `originals/<folder>` when every Photo of this Shoot that lives inside the Library
    /// sits in that one folder and no other Shoot's Photo does. Otherwise (originals read
    /// in place, or a folder shared with another Shoot) a rename leaves files alone.
    fn own_originals_folder(&self, shoot: ShootId) -> Result<Option<String>> {
        let prefix = format!("{ORIGINALS_DIR}/");
        let folder_of = |p: &str| -> Option<String> {
            let rest = p.strip_prefix(&prefix)?;
            let (leaf, _) = rest.split_once('/')?;
            Some(format!("{prefix}{leaf}"))
        };
        let mut q = self
            .db
            .prepare("SELECT path, jpeg FROM photos WHERE shoot_id = ?1")
            .map_err(db_err)?;
        let rows = q
            .query_map([shoot], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .map_err(db_err)?;
        let mut folders = std::collections::BTreeSet::new();
        for row in rows {
            let (path, jpeg) = row.map_err(db_err)?;
            for p in std::iter::once(path).chain(jpeg) {
                if let Some(f) = folder_of(&p) {
                    folders.insert(f);
                }
            }
        }
        let folder = match folders.len() {
            1 => folders.pop_first().expect("one folder"),
            _ => return Ok(None),
        };
        let dir = format!("{folder}/");
        let shared: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE shoot_id != ?1
                 AND (substr(path, 1, ?2) = ?3 OR substr(jpeg, 1, ?2) = ?3)",
                params![shoot, dir.len() as i64, dir],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        Ok((shared == 0).then_some(folder))
    }
}
