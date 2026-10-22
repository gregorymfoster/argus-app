//! Who the person named (the People view, [`crate::people`]): people, which face of which
//! Photo is whom, faces hidden as not someone to name, and "not this person" answers. A
//! named face keeps the Keyword `People|<name>` on its Photo, so search, Albums and the
//! XMP sidecar see it. Only the person names anyone; nothing here guesses.

use super::{db_err, time, Library, PhotoId};
use crate::naming::Keyword;
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

pub type PersonId = i64;

/// One face: the Photo and its number in that Photo's `faces` Fact, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FaceRef {
    pub photo_id: PhotoId,
    pub face: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    pub id: PersonId,
    pub name: String,
}

/// What the person said about one face: whose it is, or None when they hid it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceName {
    pub face: FaceRef,
    pub person: Option<PersonId>,
}

impl Library {
    /// Everyone named, by name.
    pub fn people(&self) -> Result<Vec<Person>> {
        let mut q = self
            .db
            .prepare_cached("SELECT id, name FROM people ORDER BY name COLLATE NOCASE")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| {
                Ok(Person {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    pub fn person(&self, id: PersonId) -> Result<Person> {
        self.db
            .query_row("SELECT id, name FROM people WHERE id = ?1", [id], |r| {
                Ok(Person {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| Error(format!("no person with id {id}")))
    }

    /// Every face the person named or hid.
    pub fn face_names(&self) -> Result<Vec<FaceName>> {
        let mut q = self
            .db
            .prepare_cached("SELECT photo_id, face, person_id FROM face_names")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| {
                Ok(FaceName {
                    face: FaceRef {
                        photo_id: r.get(0)?,
                        face: r.get::<_, i64>(1)? as usize,
                    },
                    person: r.get(2)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Every "not this person" answer.
    pub fn faces_not(&self) -> Result<Vec<(FaceRef, PersonId)>> {
        let mut q = self
            .db
            .prepare_cached("SELECT photo_id, face, person_id FROM face_not")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    FaceRef {
                        photo_id: r.get(0)?,
                        face: r.get::<_, i64>(1)? as usize,
                    },
                    r.get(2)?,
                ))
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Name faces as the person: the person called `name` (made if new, matched without
    /// regard to case) now owns them, and their Photos keep `People|<name>`.
    pub fn name_faces(&mut self, faces: &[FaceRef], name: &str) -> Result<Person> {
        let keyword = Keyword::person(name).ok_or_else(|| Error("a name needs a word".into()))?;
        let name = keyword.leaf().to_string();
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        let id = person_called(&tx, &name, &at)?;
        for f in faces {
            check_photo(&tx, f.photo_id)?;
            let before = owner(&tx, *f)?;
            tx.execute(
                "INSERT INTO face_names (photo_id, face, person_id, at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (photo_id, face) DO UPDATE SET person_id = ?3, at = ?4",
                params![f.photo_id, f.face as i64, id, at],
            )
            .map_err(db_err)?;
            tx.execute(
                "DELETE FROM face_not WHERE photo_id = ?1 AND face = ?2 AND person_id = ?3",
                params![f.photo_id, f.face as i64, id],
            )
            .map_err(db_err)?;
            if let Some(old) = before.filter(|&o| o != id) {
                sync_keyword(&tx, f.photo_id, old, &at)?;
            }
            sync_keyword(&tx, f.photo_id, id, &at)?;
        }
        drop_unused(&tx)?;
        tx.commit().map_err(db_err)?;
        self.person(id)
    }

    /// Hide faces (not someone to name), or with `hide` false forget what was said about
    /// them, so they're back among the faces to name.
    pub fn hide_faces(&mut self, faces: &[FaceRef], hide: bool) -> Result<usize> {
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        let mut changed = 0;
        for f in faces {
            let before = owner(&tx, *f)?;
            changed += if hide {
                tx.execute(
                    "INSERT INTO face_names (photo_id, face, person_id, at) VALUES (?1, ?2, NULL, ?3)
                     ON CONFLICT (photo_id, face) DO UPDATE SET person_id = NULL, at = ?3",
                    params![f.photo_id, f.face as i64, at],
                )
            } else {
                tx.execute(
                    "DELETE FROM face_names WHERE photo_id = ?1 AND face = ?2",
                    params![f.photo_id, f.face as i64],
                )
            }
            .map_err(db_err)?;
            if let Some(old) = before {
                sync_keyword(&tx, f.photo_id, old, &at)?;
            }
        }
        drop_unused(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(changed)
    }

    /// The person says these faces aren't `person`: they're unnamed if they were, and
    /// never offered as that person again.
    pub fn not_person(&mut self, faces: &[FaceRef], person: PersonId) -> Result<usize> {
        self.person(person)?;
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        let mut changed = 0;
        for f in faces {
            check_photo(&tx, f.photo_id)?;
            changed += tx
                .execute(
                    "INSERT OR IGNORE INTO face_not (photo_id, face, person_id) VALUES (?1, ?2, ?3)",
                    params![f.photo_id, f.face as i64, person],
                )
                .map_err(db_err)?;
            if owner(&tx, *f)? == Some(person) {
                tx.execute(
                    "DELETE FROM face_names WHERE photo_id = ?1 AND face = ?2",
                    params![f.photo_id, f.face as i64],
                )
                .map_err(db_err)?;
                sync_keyword(&tx, f.photo_id, person, &at)?;
            }
        }
        drop_unused(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(changed)
    }

    /// Rename a person, and the Keyword on their Photos. A name someone else already has
    /// merges the two into that one.
    pub fn rename_person(&mut self, id: PersonId, name: &str) -> Result<Person> {
        let keyword = Keyword::person(name).ok_or_else(|| Error("a name needs a word".into()))?;
        let name = keyword.leaf().to_string();
        let old = self.person(id)?;
        let at = time::now();
        let tx = self.db.transaction().map_err(db_err)?;
        let other: Option<PersonId> = tx
            .query_row(
                "SELECT id FROM people WHERE name = ?1 AND id != ?2",
                params![name, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let into = match other {
            Some(into) => {
                let photos = photos_of(&tx, id)?;
                tx.execute(
                    "UPDATE face_names SET person_id = ?1 WHERE person_id = ?2",
                    params![into, id],
                )
                .map_err(db_err)?;
                tx.execute(
                    "INSERT OR IGNORE INTO face_not (photo_id, face, person_id)
                     SELECT photo_id, face, ?1 FROM face_not WHERE person_id = ?2",
                    params![into, id],
                )
                .map_err(db_err)?;
                tx.execute("DELETE FROM face_not WHERE person_id = ?1", [id])
                    .map_err(db_err)?;
                for p in photos {
                    remove_keyword(&tx, p, &old.name)?;
                    sync_keyword(&tx, p, into, &at)?;
                }
                tx.execute("DELETE FROM people WHERE id = ?1", [id])
                    .map_err(db_err)?;
                into
            }
            None => {
                tx.execute(
                    "UPDATE people SET name = ?1 WHERE id = ?2",
                    params![name, id],
                )
                .map_err(db_err)?;
                for p in photos_of(&tx, id)? {
                    remove_keyword(&tx, p, &old.name)?;
                    sync_keyword(&tx, p, id, &at)?;
                }
                id
            }
        };
        tx.commit().map_err(db_err)?;
        self.person(into)
    }
}

/// The person called `name`, made if new.
fn person_called(tx: &Transaction, name: &str, at: &str) -> Result<PersonId> {
    if let Some(id) = tx
        .query_row("SELECT id FROM people WHERE name = ?1", [name], |r| {
            r.get(0)
        })
        .optional()
        .map_err(db_err)?
    {
        return Ok(id);
    }
    tx.execute(
        "INSERT INTO people (name, created_at) VALUES (?1, ?2)",
        params![name, at],
    )
    .map_err(db_err)?;
    Ok(tx.last_insert_rowid())
}

fn owner(tx: &Transaction, f: FaceRef) -> Result<Option<PersonId>> {
    Ok(tx
        .query_row(
            "SELECT person_id FROM face_names WHERE photo_id = ?1 AND face = ?2",
            params![f.photo_id, f.face as i64],
            |r| r.get::<_, Option<PersonId>>(0),
        )
        .optional()
        .map_err(db_err)?
        .flatten())
}

fn photos_of(tx: &Transaction, person: PersonId) -> Result<Vec<PhotoId>> {
    let mut q = tx
        .prepare("SELECT DISTINCT photo_id FROM face_names WHERE person_id = ?1")
        .map_err(db_err)?;
    let rows = q.query_map([person], |r| r.get(0)).map_err(db_err)?;
    rows.collect::<std::result::Result<_, _>>().map_err(db_err)
}

/// Keep `People|<name>` on the Photo while one of its faces is that person, and take the
/// person's own copy off when none is.
fn sync_keyword(tx: &Transaction, photo: PhotoId, person: PersonId, at: &str) -> Result<()> {
    let name: String = tx
        .query_row("SELECT name FROM people WHERE id = ?1", [person], |r| {
            r.get(0)
        })
        .map_err(db_err)?;
    let named = tx
        .query_row(
            "SELECT 1 FROM face_names WHERE photo_id = ?1 AND person_id = ?2",
            params![photo, person],
            |_| Ok(()),
        )
        .optional()
        .map_err(db_err)?
        .is_some();
    if !named {
        return remove_keyword(tx, photo, &name);
    }
    let path = Keyword::person(&name)
        .expect("a stored name has a word")
        .path();
    tx.execute(
        "INSERT INTO photo_keywords (photo_id, keyword, author, state, at)
         VALUES (?1, ?2, 'person', 'kept', ?3)
         ON CONFLICT (photo_id, keyword) DO UPDATE SET state = 'kept', at = ?3 WHERE state != 'kept'",
        params![photo, path, at],
    )
    .map_err(db_err)?;
    Ok(())
}

fn remove_keyword(tx: &Transaction, photo: PhotoId, name: &str) -> Result<()> {
    let Some(k) = Keyword::person(name) else {
        return Ok(());
    };
    tx.execute(
        "DELETE FROM photo_keywords WHERE photo_id = ?1 AND keyword = ?2 AND author = 'person'",
        params![photo, k.path()],
    )
    .map_err(db_err)?;
    Ok(())
}

/// People with no faces left are forgotten (their "not this person" answers too).
fn drop_unused(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "DELETE FROM face_not WHERE person_id NOT IN (SELECT person_id FROM face_names WHERE person_id IS NOT NULL);
         DELETE FROM people WHERE id NOT IN (SELECT person_id FROM face_names WHERE person_id IS NOT NULL);",
    )
    .map_err(db_err)
}

fn check_photo(tx: &Transaction, id: PhotoId) -> Result<()> {
    tx.query_row("SELECT 1 FROM photos WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map_err(db_err)?
        .ok_or_else(|| Error(format!("no Photo with id {id}")))
}
