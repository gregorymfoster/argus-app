//! Looks: a Shoot's shared creative styles. Each save appends a row, so tuning a Look
//! is undoable and every Photo whose Recipe names it follows the newest version.

use super::{db_err, time, BatchId, Library, ShootId};
use crate::recipe::Adjust;
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

/// A shared style: sliders applied under every Photo's own Recipe (prototype: `Look`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Look {
    /// Stable within its Shoot (`warm-film`); a Recipe's `look` names it.
    pub id: String,
    pub name: String,
    pub adjust: Adjust,
}

impl Library {
    /// The current version of each of a Shoot's Looks, oldest Look first.
    pub fn looks(&self, shoot: ShootId) -> Result<Vec<Look>> {
        let mut stmt = self
            .db
            .prepare("SELECT look FROM looks WHERE shoot_id = ?1 ORDER BY id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([shoot], |r| r.get::<_, String>(0))
            .map_err(db_err)?;
        let mut out: Vec<Look> = Vec::new();
        for json in rows {
            let look: Look = serde_json::from_str(&json.map_err(db_err)?)
                .map_err(|e| Error(format!("look: {e}")))?;
            match out.iter_mut().find(|l| l.id == look.id) {
                Some(slot) => *slot = look,
                None => out.push(look),
            }
        }
        Ok(out)
    }

    /// The current version of one Look, or None.
    pub fn look(&self, shoot: ShootId, id: &str) -> Result<Option<Look>> {
        Ok(self.looks(shoot)?.into_iter().find(|l| l.id == id))
    }

    /// Save a new version of a Look (creating it if its id is new).
    pub fn save_look(&mut self, shoot: ShootId, look: &Look, batch: Option<BatchId>) -> Result<()> {
        let exists = self
            .db
            .query_row("SELECT 1 FROM shoots WHERE id = ?1", [shoot], |_| Ok(()))
            .optional()
            .map_err(db_err)?;
        if exists.is_none() {
            return Err(Error(format!("no Shoot with id {shoot}")));
        }
        let json = serde_json::to_string(look).expect("Look serialises");
        self.db
            .execute(
                "INSERT INTO looks (shoot_id, look, at, batch_id) VALUES (?1, ?2, ?3, ?4)",
                params![shoot, json, time::now(), batch],
            )
            .map_err(db_err)?;
        Ok(())
    }
}
