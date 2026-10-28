//! Renaming a Shoot, and moving Photos between Shoots. A Shoot's name is also the name of
//! the folder Import copied its originals into (`originals/2026-10-18_Boat-Trip`,
//! docs/naming.md), so a rename moves that folder too and rewrites the Photos' relative
//! paths, keeping Argus and Finder in step (ADR 0011, "Renaming a Shoot"). Moving a Photo
//! to another Shoot moves its copied original into that Shoot's folder the same way.

use super::{db_err, Library, PhotoId, ShootId, ORIGINALS_DIR};
use crate::{naming, Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Where [`Library::move_photos`] puts the Photos.
#[derive(Debug, Clone)]
pub enum MoveTo {
    Shoot(ShootId),
    /// A new Shoot with this title (named by the naming rules, dated by its first Photo).
    New(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct PhotoMoveReport {
    pub shoot_id: ShootId,
    pub name: String,
    pub moved: usize,
    /// Shoots the move left without Photos; they are removed.
    pub removed_shoots: Vec<ShootId>,
}

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

impl Library {
    /// Move Photos to another Shoot (every Photo belongs to exactly one). Copied originals
    /// follow into the target's own originals folder when it has one, or when the move
    /// makes a new Shoot, so Finder matches Argus; originals read in place stay put. A
    /// moved Photo leaves its Stack and its old Shoot's Best Photos (the next cull redoes
    /// them), and a Shoot left empty is removed. Files are moved back if the database
    /// can't be written.
    pub fn move_photos(&mut self, photos: &[PhotoId], to: MoveTo) -> Result<PhotoMoveReport> {
        if photos.is_empty() {
            return Err(Error("move_photos needs photo_ids".into()));
        }
        let mut rows = Vec::new();
        for &id in photos {
            let row: (ShootId, String, Option<String>, String) = self
                .db
                .query_row(
                    "SELECT shoot_id, path, jpeg, captured_at FROM photos WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(db_err)?
                .ok_or_else(|| Error(format!("no Photo {id}")))?;
            rows.push((id, row));
        }
        let sources: BTreeSet<ShootId> = rows.iter().map(|(_, r)| r.0).collect();

        // The target, and the folder its copied originals live in.
        let (target, name, folder) = match to {
            MoveTo::Shoot(t) => {
                let name: String = self
                    .db
                    .query_row("SELECT name FROM shoots WHERE id = ?1", [t], |r| r.get(0))
                    .optional()
                    .map_err(db_err)?
                    .ok_or_else(|| Error(format!("no Shoot {t}")))?;
                let folder = self.own_originals_folder(t)?;
                (Some(t), name, folder)
            }
            MoveTo::New(title) => {
                if title.trim().is_empty() {
                    return Err(Error("a Shoot needs a name".into()));
                }
                let first = rows.iter().map(|(_, r)| r.3.as_str()).min();
                let name = naming::shoot_slug(&title, first);
                let base = self.root.join(ORIGINALS_DIR);
                let copied = rows
                    .iter()
                    .any(|(_, r)| r.1.starts_with(&format!("{ORIGINALS_DIR}/")));
                let folder = copied.then(|| {
                    let leaf = naming::unique_name(&name, |n| base.join(n).exists());
                    format!("{ORIGINALS_DIR}/{leaf}")
                });
                (None, name, folder)
            }
        };

        // Move copied originals (and their camera JPEGs) into the target's folder.
        let mut moved_files: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut new_paths: Vec<(PhotoId, String, Option<String>)> = Vec::new();
        let result = (|| -> Result<ShootId> {
            for (id, (from_shoot, path, jpeg, _)) in &rows {
                if Some(*from_shoot) == target {
                    new_paths.push((*id, path.clone(), jpeg.clone()));
                    continue;
                }
                let mut relocate = |stored: &str| -> Result<String> {
                    let Some(dir) = &folder else {
                        return Ok(stored.to_string());
                    };
                    if !stored.starts_with(&format!("{ORIGINALS_DIR}/")) {
                        return Ok(stored.to_string());
                    }
                    let from = self.root.join(stored);
                    let file = from
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let dest_dir = self.root.join(dir);
                    std::fs::create_dir_all(&dest_dir)
                        .map_err(|e| Error(format!("{}: {e}", dest_dir.display())))?;
                    let leaf = naming::unique_name(&file, |n| dest_dir.join(n).exists());
                    let to = dest_dir.join(&leaf);
                    if from.exists() {
                        std::fs::rename(&from, &to).map_err(|e| {
                            Error(format!("move {} to {}: {e}", from.display(), to.display()))
                        })?;
                        moved_files.push((from, to));
                    }
                    Ok(format!("{dir}/{leaf}"))
                };
                let p = relocate(path)?;
                let j = jpeg.as_deref().map(&mut relocate).transpose()?;
                new_paths.push((*id, p, j));
            }

            let tx = self.db.transaction().map_err(db_err)?;
            let target = match target {
                Some(t) => t,
                None => {
                    tx.execute(
                        "INSERT INTO shoots (name, source, imported_at) VALUES (?1, 'moved', ?2)",
                        params![name, super::time::now()],
                    )
                    .map_err(db_err)?;
                    tx.last_insert_rowid()
                }
            };
            for (id, path, jpeg) in &new_paths {
                tx.execute(
                    "UPDATE photos SET shoot_id = ?1, path = ?2, jpeg = ?3 WHERE id = ?4",
                    params![target, path, jpeg, id],
                )
                .map_err(db_err)?;
                tx.execute("DELETE FROM stack_members WHERE photo_id = ?1", [id])
                    .map_err(db_err)?;
                tx.execute(
                    "DELETE FROM best_photos WHERE photo_id = ?1 AND shoot_id != ?2",
                    params![id, target],
                )
                .map_err(db_err)?;
            }
            // A Stack of one isn't a Stack.
            tx.execute(
                "DELETE FROM stack_members WHERE stack_id IN
                 (SELECT stack_id FROM stack_members GROUP BY stack_id HAVING COUNT(*) < 2)",
                [],
            )
            .map_err(db_err)?;
            tx.execute(
                "DELETE FROM stacks WHERE id NOT IN (SELECT stack_id FROM stack_members)",
                [],
            )
            .map_err(db_err)?;
            tx.commit().map_err(db_err)?;
            Ok(target)
        })();
        let target = match result {
            Ok(t) => t,
            Err(e) => {
                for (from, to) in moved_files.iter().rev() {
                    let _ = std::fs::rename(to, from);
                }
                return Err(e);
            }
        };

        // Shoots the move emptied go, with their folder if nothing is left in it.
        let mut removed = Vec::new();
        for s in sources {
            if s == target {
                continue;
            }
            let left: i64 = self
                .db
                .query_row(
                    "SELECT COUNT(*) FROM photos WHERE shoot_id = ?1",
                    [s],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if left == 0 {
                let tx = self.db.transaction().map_err(db_err)?;
                for t in ["looks", "best_photos", "stacks", "shoots"] {
                    let col = if t == "shoots" { "id" } else { "shoot_id" };
                    tx.execute(&format!("DELETE FROM {t} WHERE {col} = ?1"), [s])
                        .map_err(db_err)?;
                }
                tx.commit().map_err(db_err)?;
                removed.push(s);
            }
        }
        for (from, _) in &moved_files {
            if let Some(dir) = from.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
        Ok(PhotoMoveReport {
            shoot_id: target,
            name,
            moved: rows.len(),
            removed_shoots: removed,
        })
    }
}
