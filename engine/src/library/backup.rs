//! Library backups: whole copies of `library.db` (every Verdict, Recipe Version, Look and
//! batch) in the Library's own `backups/` folder, so they move with it. Originals and
//! previews are not copied: originals are never written, and previews can be rebuilt.
//! See ADR 0009.
//!
//! A backup is made with `VACUUM INTO`, a consistent snapshot even while other processes
//! write, into a temporary name that is renamed into place, so a crash never leaves a
//! half-written file that looks like a backup. Names are UTC times plus an optional tag:
//! `2026-10-18_130102.db` (daily), `2026-10-18_130102_pre-v4.db` (before a schema
//! upgrade), `2026-10-18_130102_before-restore.db`. Only untagged ones are pruned.

use super::{db_err, migrate, time, Library};
use crate::{Error, Result};
use rusqlite::{Connection, MAIN_DB};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const DIR: &str = "backups";
/// Daily backups kept; tagged ones (pre-upgrade, before-restore) are always kept.
pub const KEEP: usize = 14;
pub const EVERY: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Serialize)]
pub struct Backup {
    /// File name inside `backups/`, which is also how a restore names it.
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
    /// UTC, as in the name.
    pub at: String,
    /// "daily", or the tag ("pre-v4", "before-restore", "manual").
    pub kind: String,
}

fn io(what: &Path, e: std::io::Error) -> Error {
    Error(format!("{}: {e}", what.display()))
}

/// `2026-10-18T13:01:02Z` -> `2026-10-18_130102`.
fn stamp() -> String {
    let t = time::now();
    format!("{}_{}", &t[..10], t[11..19].replace(':', ""))
}

/// Copy the database into `backups/`, tagged (or not, for a daily one).
pub fn snapshot(db: &Connection, root: &Path, tag: &str) -> Result<PathBuf> {
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    let (stamp, tag) = (
        stamp(),
        if tag.is_empty() {
            String::new()
        } else {
            format!("_{tag}")
        },
    );
    let mut path = dir.join(format!("{stamp}{tag}.db"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stamp}-{n}{tag}.db"));
        n += 1;
    }
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&tmp);
    db.execute("VACUUM INTO ?1", [tmp.to_string_lossy()])
        .map_err(db_err)?;
    std::fs::File::open(&tmp)
        .and_then(|f| f.sync_all())
        .map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| io(&path, e))?;
    Ok(path)
}

fn parse(path: PathBuf) -> Option<Backup> {
    let name = path.file_name()?.to_str()?.to_string();
    let stem = name.strip_suffix(".db")?;
    let b = stem.as_bytes();
    let dated = b.len() >= 17
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'_'
        && stem[..10].bytes().filter(u8::is_ascii_digit).count() == 8
        && stem[11..17].bytes().all(|c| c.is_ascii_digit());
    if !dated {
        return None;
    }
    let rest = stem[17..].trim_start_matches(|c: char| c == '-' || c.is_ascii_digit());
    let kind = match rest.strip_prefix('_') {
        Some(tag) => tag.to_string(),
        None => "daily".into(),
    };
    let t = &stem[11..17];
    Some(Backup {
        at: format!("{}T{}:{}:{}Z", &stem[..10], &t[..2], &t[2..4], &t[4..6]),
        bytes: std::fs::metadata(&path).ok()?.len(),
        name,
        path,
        kind,
    })
}

impl Library {
    /// The Library's backups, newest first.
    pub fn backups(&self) -> Result<Vec<Backup>> {
        let dir = self.root.join(DIR);
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&dir, e)),
        };
        let mut all: Vec<Backup> = entries.filter_map(|e| parse(e.ok()?.path())).collect();
        all.sort_by(|a, b| b.name.cmp(&a.name));
        Ok(all)
    }

    /// Back up now, whatever the schedule, tagged "manual".
    pub fn backup_now(&self) -> Result<PathBuf> {
        self.check()?;
        snapshot(&self.db, &self.root, "manual")
    }

    /// Make the daily backup if the newest is older than a day (or there is none), then
    /// keep the newest [`KEEP`] daily ones. A database that fails its integrity check is
    /// never backed up, so it can't push good backups out.
    pub fn backup_if_due(&self) -> Result<Option<PathBuf>> {
        let daily: Vec<Backup> = self
            .backups()?
            .into_iter()
            .filter(|b| b.kind == "daily")
            .collect();
        let fresh = daily.first().is_some_and(|b| {
            std::fs::metadata(&b.path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age < EVERY)
        });
        if fresh {
            return Ok(None);
        }
        self.check()?;
        let made = snapshot(&self.db, &self.root, "")?;
        for old in self
            .backups()?
            .into_iter()
            .filter(|b| b.kind == "daily")
            .skip(KEEP)
        {
            let _ = std::fs::remove_file(&old.path);
        }
        Ok(Some(made))
    }

    /// SQLite's quick integrity check. Err names what is wrong.
    pub fn check(&self) -> Result<()> {
        let problems: Vec<String> = {
            let mut q = self.db.prepare("PRAGMA quick_check").map_err(db_err)?;
            let rows = q.query_map([], |r| r.get(0)).map_err(db_err)?;
            rows.collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        if problems.len() == 1 && problems[0] == "ok" {
            return Ok(());
        }
        Err(Error(format!(
            "the Library database is damaged ({}). Restore a backup with `argus restore`.",
            problems.join("; ")
        )))
    }

    /// Put a backup (by name, see [`Library::backups`]) back as the Library's database.
    /// The current database is backed up first ("before-restore"), so a restore can be
    /// undone the same way. Runs through SQLite's backup API, under its locks, so other
    /// open connections see the restored data rather than a torn file.
    pub fn restore_backup(&mut self, name: &str) -> Result<PathBuf> {
        let backup = self
            .backups()?
            .into_iter()
            .find(|b| b.name == name)
            .ok_or_else(|| {
                Error(format!(
                    "no backup named {name} in {}",
                    self.root.join(DIR).display()
                ))
            })?;
        let src = Connection::open(&backup.path).map_err(db_err)?;
        let problems: String = src
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(db_err)?;
        if problems != "ok" {
            return Err(Error(format!("backup {name} is damaged: {problems}")));
        }
        drop(src);
        let before = snapshot(&self.db, &self.root, "before-restore")?;
        self.db
            .restore(
                MAIN_DB,
                &backup.path,
                None::<fn(rusqlite::backup::Progress)>,
            )
            .map_err(db_err)?;
        migrate::run(&mut self.db, &self.root)?;
        Ok(before)
    }
}
