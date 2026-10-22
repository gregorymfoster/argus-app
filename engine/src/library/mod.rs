//! The Library: one self-contained folder holding `library.db` (SQLite), `previews/`,
//! `originals/` (Photos copied in on Import) and `cache/`. Paths inside it are stored
//! relative to it, so the folder can move to another disk or Mac and still open (ADR 0011).
//!
//! This is the M1 slice of the tool layer. Methods are named after the tools in
//! docs/spec.md (list_photos, set_verdict, read_recipe, write_recipe) so the CLI, the
//! app and the MCP server can all sit directly on top of it. Originals are only ever
//! read; everything Argus makes lives inside the Library folder.

mod backup;
mod facts;
mod import;
mod jobs;
mod keywords;
mod looks;
mod migrate;
mod people;
mod previews;
mod ratings;
mod remove;
mod scan;
mod search;
mod shoots;
mod storage;
mod suggestions;
pub(crate) mod time;
mod undo;
mod usage;

use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use backup::Backup;
pub(crate) use facts::FactRow;
pub(crate) use import::read_jpeg_upright;
pub use import::{ImportOptions, ImportReport};

/// Shutter, aperture, ISO, focal length and lens from a file's EXIF (the whole file's bytes),
/// as Import reads them.
pub fn exposure_of(data: &[u8]) -> Exposure {
    import::Exif::read(data).exposure
}
pub use jobs::{Job, JobId, JobStatus, QueueSummary};
pub use keywords::{Caption, KeywordCount, PhotoKeyword, TagReport, Tags};
pub use looks::Look;
pub use migrate::LATEST as SCHEMA_VERSION;
pub use people::{FaceName, FaceRef, Person, PersonId};
pub use previews::PreviewReport;
pub use ratings::{BestPhoto, CullState};
pub use remove::{Removal, RemoveReport, RestoreReport, TRASH_FILE};
pub use scan::{find_cards, is_on_card, Card, Scan, ScannedPhoto};
pub use search::{Album, Hit as SearchHit};
pub use shoots::RenameReport;
pub use storage::{
    move_library, move_library_via, ImportEstimate, MissingOriginal, MoveReport, RebuildReport,
    RelinkReport, Settings, Storage, Usage,
};
pub use suggestions::Suggestion;
pub use undo::UndoReport;
pub use usage::{UsageEvent, UsageSummary, RECORD_KEY as RECORD_USAGE_KEY};

pub const DB_FILE: &str = "library.db";
/// Where Import copies originals to, inside the Library folder.
pub const ORIGINALS_DIR: &str = "originals";
pub const PREVIEWS_DIR: &str = "previews";
/// Anything Argus can rebuild and may delete at any time.
pub const CACHE_DIR: &str = "cache";

pub type PhotoId = i64;
pub type ShootId = i64;
pub type BatchId = i64;

/// A Photo's cull outcome, as a Lightroom-style flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pick,
    Reject,
    Unflagged,
}

/// Who made a change: the person or the AI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Author {
    Person,
    Ai,
}

macro_rules! text_enum {
    ($t:ty { $($v:ident = $s:literal),* }) => {
        impl $t {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$v => $s),* }
            }
        }
        impl std::str::FromStr for $t {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self> {
                match s { $($s => Ok(Self::$v),)* _ => Err(Error(format!("unknown {}: {s}", stringify!($t)))) }
            }
        }
    };
}
text_enum!(Verdict { Pick = "pick", Reject = "reject", Unflagged = "unflagged" });
text_enum!(Author { Person = "person", Ai = "ai" });

#[derive(Debug, Clone, Serialize)]
pub struct Shoot {
    pub id: ShootId,
    pub name: String,
    pub source: String,
    pub imported_at: String,
    pub photos: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Photo {
    pub id: PhotoId,
    pub shoot_id: ShootId,
    pub file_name: String,
    /// Absolute path of the original (resolved from the Library-relative path when the
    /// original lives inside the Library).
    pub path: String,
    /// The original was copied into the Library's `originals/` folder on Import.
    pub in_library: bool,
    pub kind: String,
    pub captured_at: String,
    pub width: u32,
    pub height: u32,
    pub camera: String,
    /// Shutter, aperture, ISO, focal length and lens, as the camera recorded them.
    #[serde(flatten)]
    pub exposure: Exposure,
    /// Absolute path of the Library's own preview JPEG.
    pub preview: PathBuf,
    pub verdict: Verdict,
    pub verdict_by: Option<Author>,
    pub reason: Option<String>,
    /// The Stack this Photo belongs to, and its rank in it (0 = best).
    pub stack: Option<i64>,
    pub stack_rank: Option<i64>,
    /// The camera's JPEG of this frame when it was shot RAW+JPEG (the RAW is the Photo).
    pub jpeg: Option<PathBuf>,
    /// Lightroom-style star Rating, 0 (unrated) to 5, and who set it.
    pub stars: u8,
    pub stars_by: Option<Author>,
    /// Rank in the Shoot's Best Photos (1 = best), if the AI put it there.
    pub best: Option<i64>,
}

/// The exposure settings in a Photo's EXIF, each missing when the camera didn't record it
/// (a manual lens reports no aperture or focal length, a JPEG from a phone app no lens).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Exposure {
    /// Shutter speed in seconds.
    pub shutter: Option<f64>,
    /// f-number.
    pub aperture: Option<f64>,
    pub iso: Option<u32>,
    /// Focal length in mm, as marked on the lens (not 35 mm equivalent).
    pub focal: Option<f64>,
    pub lens: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PhotoFilter {
    pub shoot: Option<ShootId>,
    pub verdict: Option<Verdict>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecipeVersion {
    pub version: i64,
    pub recipe: serde_json::Value,
    pub author: Author,
    pub batch_id: Option<BatchId>,
    pub at: String,
}

pub struct Library {
    root: PathBuf,
    db: Connection,
}

/// One saved Version of a Photo's Recipe, as the History list shows it.
#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub version: i64,
    pub author: Author,
    pub batch_id: Option<BatchId>,
    /// The batch's label ("Agent: warmer, less contrast"), if the Version was part of one.
    pub label: Option<String>,
    pub at: String,
    pub recipe: serde_json::Value,
}

fn db_err(e: rusqlite::Error) -> Error {
    Error(format!("library: {e}"))
}

/// How a path is kept in the Library: relative (with `/`) when it is inside `root`, so the
/// Library folder can move, and absolute otherwise.
pub(super) fn store_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) if path.is_absolute() => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        _ => path.to_string_lossy().into_owned(),
    }
}

fn resolve_path(root: &Path, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

impl Library {
    /// Open the Library folder at `root`, creating it and its schema if needed.
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root.join(PREVIEWS_DIR))
            .map_err(|e| Error(format!("create {}: {e}", root.display())))?;
        let root = root
            .canonicalize()
            .map_err(|e| Error(format!("{}: {e}", root.display())))?;
        let mut db = Connection::open(root.join(DB_FILE)).map_err(db_err)?;
        // WAL lets the app, `argus serve`, MCP and tool calls read while one writes;
        // synchronous=FULL makes every commit durable, not only crash-consistent; the
        // busy timeout makes a second writer wait its turn instead of failing (ADR 0009).
        db.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 10000;",
        )
        .map_err(db_err)?;
        // Every transaction here writes, so take the write lock when it begins: a deferred
        // one that reads first can't wait for the lock under WAL and fails with "database
        // is locked" at once when another writer committed meanwhile (fuzz 2026-10-22).
        db.set_transaction_behavior(TransactionBehavior::Immediate);
        migrate::run(&mut db, &root)?;
        Ok(Self { root, db })
    }

    /// True when `dir` holds a Library (its `library.db`).
    pub fn is_library(dir: &Path) -> bool {
        dir.join(DB_FILE).is_file()
    }

    /// The absolute path of an original as stored in `photos.path`.
    pub fn resolve(&self, stored: &str) -> PathBuf {
        resolve_path(&self.root, stored)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Import a folder (recursively) as one new Shoot. See [`import`].
    pub fn import(&mut self, folder: &Path, opts: &ImportOptions) -> Result<ImportReport> {
        import::run(self, folder, opts)
    }

    /// List the Photos in a folder (or on a card) and which the Library already has,
    /// without importing anything. See [`scan`].
    pub fn scan(&self, folder: &Path) -> Result<Scan> {
        scan::run(self, folder)
    }

    pub fn shoots(&self) -> Result<Vec<Shoot>> {
        let mut q = self
            .db
            .prepare(
                "SELECT s.id, s.name, s.source, s.imported_at, COUNT(p.id)
                 FROM shoots s LEFT JOIN photos p ON p.shoot_id = s.id
                 GROUP BY s.id ORDER BY s.id",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| {
                Ok(Shoot {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    source: r.get(2)?,
                    imported_at: r.get(3)?,
                    photos: r.get(4)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Photos in capture order, optionally narrowed to one Shoot or one Verdict.
    pub fn list_photos(&self, filter: &PhotoFilter) -> Result<Vec<Photo>> {
        let mut q = self
            .db
            .prepare(
                "SELECT p.id, p.shoot_id, p.file_name, p.path, p.kind, p.captured_at, p.width,
                        p.height, p.camera, p.preview, COALESCE(v.verdict, 'unflagged'), v.author, v.reason,
                        m.stack_id, m.rank, p.jpeg, COALESCE(r.stars, 0), r.author, b.rank,
                        p.shutter, p.aperture, p.iso, p.focal, p.lens
                 FROM photos p LEFT JOIN verdicts v ON v.photo_id = p.id
                 LEFT JOIN stack_members m ON m.photo_id = p.id
                 LEFT JOIN ratings r ON r.photo_id = p.id
                 LEFT JOIN best_photos b ON b.photo_id = p.id
                 WHERE (?1 IS NULL OR p.shoot_id = ?1)
                   AND (?2 IS NULL OR COALESCE(v.verdict, 'unflagged') = ?2)
                 ORDER BY p.captured_at, p.file_name",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map(
                params![filter.shoot, filter.verdict.map(Verdict::as_str)],
                |r| {
                    let verdict: String = r.get(10)?;
                    let by: Option<String> = r.get(11)?;
                    let preview: String = r.get(9)?;
                    let stored: String = r.get(3)?;
                    Ok(Photo {
                        id: r.get(0)?,
                        shoot_id: r.get(1)?,
                        file_name: r.get(2)?,
                        path: self.resolve(&stored).to_string_lossy().into_owned(),
                        in_library: Path::new(&stored).is_relative(),
                        kind: r.get(4)?,
                        captured_at: r.get(5)?,
                        width: r.get(6)?,
                        height: r.get(7)?,
                        camera: r.get(8)?,
                        exposure: Exposure {
                            shutter: r.get(19)?,
                            aperture: r.get(20)?,
                            iso: r.get(21)?,
                            focal: r.get(22)?,
                            lens: r.get(23)?,
                        },
                        preview: self.root.join(preview),
                        verdict: verdict.parse().unwrap_or(Verdict::Unflagged),
                        verdict_by: by.and_then(|b| b.parse().ok()),
                        reason: r.get(12)?,
                        stack: r.get(13)?,
                        stack_rank: r.get(14)?,
                        jpeg: r.get::<_, Option<String>>(15)?.map(|j| self.resolve(&j)),
                        stars: r.get(16)?,
                        stars_by: r.get::<_, Option<String>>(17)?.and_then(|b| b.parse().ok()),
                        best: r.get(18)?,
                    })
                },
            )
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Start a batch so a group of changes can be undone together.
    pub fn begin_batch(&mut self, author: Author, label: &str) -> Result<BatchId> {
        self.db
            .execute(
                "INSERT INTO batches (author, label, at) VALUES (?1, ?2, ?3)",
                params![author.as_str(), label, time::now()],
            )
            .map_err(db_err)?;
        Ok(self.db.last_insert_rowid())
    }

    /// Who started a batch: its changes are recorded under the same author.
    pub fn batch_author(&self, batch: BatchId) -> Result<Author> {
        let a: String = self
            .db
            .query_row("SELECT author FROM batches WHERE id = ?1", [batch], |r| {
                r.get(0)
            })
            .map_err(db_err)?;
        a.parse()
    }

    /// Set one Verdict on many Photos. A person overruling an AI Verdict is logged as an
    /// Override. Unknown Photo ids are an error and nothing is changed.
    pub fn set_verdict(
        &mut self,
        photos: &[PhotoId],
        verdict: Verdict,
        author: Author,
        reason: Option<&str>,
        batch: Option<BatchId>,
    ) -> Result<()> {
        let at = time::now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        for &id in photos {
            let exists: bool = tx
                .query_row("SELECT 1 FROM photos WHERE id = ?1", [id], |_| Ok(true))
                .optional()
                .map_err(db_err)?
                .is_some();
            if !exists {
                return Err(Error(format!("no Photo with id {id}")));
            }
            let prev: Option<(String, String)> = tx
                .query_row(
                    "SELECT verdict, author FROM verdicts WHERE photo_id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            if let Some((ai_value, prev_author)) = &prev {
                if author == Author::Person && prev_author == "ai" && ai_value != verdict.as_str() {
                    tx.execute(
                        "INSERT INTO overrides (photo_id, field, ai_value, person_value, at)
                         VALUES (?1, 'verdict', ?2, ?3, ?4)",
                        params![id, ai_value, verdict.as_str(), at],
                    )
                    .map_err(db_err)?;
                }
            }
            for table in ["verdicts", "verdict_log"] {
                let verb = if table == "verdicts" {
                    "INSERT OR REPLACE"
                } else {
                    "INSERT"
                };
                tx.execute(
                    &format!(
                        "{verb} INTO {table} (photo_id, verdict, author, reason, batch_id, at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
                    ),
                    params![id, verdict.as_str(), author.as_str(), reason, batch, at],
                )
                .map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)?;
        for &id in photos {
            self.log_usage(
                id,
                "verdict",
                serde_json::json!({ "verdict": verdict.as_str(), "author": author.as_str(),
                                    "reason": reason, "batch_id": batch }),
            );
        }
        Ok(())
    }

    /// Replace a Shoot's Stacks. Each inner list is one Stack, best frame first; Photos
    /// not in the Shoot are an error and nothing changes.
    pub fn replace_stacks(&mut self, shoot: ShootId, stacks: &[Vec<PhotoId>]) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        tx.execute(
            "DELETE FROM stack_members WHERE stack_id IN (SELECT id FROM stacks WHERE shoot_id = ?1)",
            [shoot],
        )
        .map_err(db_err)?;
        tx.execute("DELETE FROM stacks WHERE shoot_id = ?1", [shoot])
            .map_err(db_err)?;
        for members in stacks.iter().filter(|m| m.len() > 1) {
            tx.execute("INSERT INTO stacks (shoot_id) VALUES (?1)", [shoot])
                .map_err(db_err)?;
            let stack = tx.last_insert_rowid();
            for (rank, &photo) in members.iter().enumerate() {
                let in_shoot: i64 = tx
                    .query_row(
                        "SELECT COUNT(*) FROM photos WHERE id = ?1 AND shoot_id = ?2",
                        [photo, shoot],
                        |r| r.get(0),
                    )
                    .map_err(db_err)?;
                if in_shoot == 0 {
                    return Err(Error(format!("Photo {photo} is not in Shoot {shoot}")));
                }
                tx.execute(
                    "INSERT OR REPLACE INTO stack_members (stack_id, photo_id, rank) VALUES (?1, ?2, ?3)",
                    params![stack, photo, rank as i64],
                )
                .map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)
    }

    /// Number of logged Overrides (person overruling the AI), for tracking cull quality.
    pub fn override_count(&self) -> Result<i64> {
        self.db
            .query_row("SELECT COUNT(*) FROM overrides", [], |r| r.get(0))
            .map_err(db_err)
    }

    /// A stamp per edited Photo (`version@at` of its current Recipe) that changes whenever
    /// its Recipe does, undo included, so the grid knows which thumbnails to re-render.
    pub fn recipe_stamps(&self) -> Result<std::collections::HashMap<PhotoId, String>> {
        let mut q = self
            .db
            .prepare(
                "SELECT photo_id, version || '@' || at FROM recipe_versions v
                 WHERE version = (SELECT MAX(version) FROM recipe_versions WHERE photo_id = v.photo_id)",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// The current Recipe Version, or None if the Photo has never been edited.
    pub fn read_recipe(&self, photo: PhotoId) -> Result<Option<RecipeVersion>> {
        self.db
            .query_row(
                "SELECT version, recipe, author, batch_id, at FROM recipe_versions
                 WHERE photo_id = ?1 ORDER BY version DESC LIMIT 1",
                [photo],
                |r| {
                    let json: String = r.get(1)?;
                    let author: String = r.get(2)?;
                    Ok(RecipeVersion {
                        version: r.get(0)?,
                        recipe: serde_json::from_str(&json).unwrap_or_default(),
                        author: author.parse().unwrap_or(Author::Person),
                        batch_id: r.get(3)?,
                        at: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(db_err)
    }

    /// Append a new Recipe Version and return its number (1 for the first edit).
    pub fn write_recipe(
        &mut self,
        photo: PhotoId,
        recipe: &serde_json::Value,
        author: Author,
        batch: Option<BatchId>,
    ) -> Result<i64> {
        // Read the next number and write it under one write lock, so two processes
        // editing the same Photo can't both claim it.
        self.atomic(|lib| {
            let next: i64 = lib
                .db
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) + 1 FROM recipe_versions WHERE photo_id = ?1",
                    [photo],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            lib.log_start(photo);
            lib.db
                .execute(
                    "INSERT INTO recipe_versions (photo_id, version, recipe, author, batch_id, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        photo,
                        next,
                        recipe.to_string(),
                        author.as_str(),
                        batch,
                        time::now()
                    ],
                )
                .map_err(db_err)?;
            lib.log_recipe(photo, next, recipe, author.as_str(), batch);
            crate::denoise::queue_if_needed(lib, photo, recipe, author)?;
            Ok(next)
        })
    }

    /// Run `f` as one write transaction (BEGIN IMMEDIATE), or as part of the caller's if
    /// one is open: all of it lands or none of it does, even if the process dies midway.
    pub(crate) fn atomic<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if !self.db.is_autocommit() {
            return f(self);
        }
        self.db.execute_batch("BEGIN IMMEDIATE").map_err(db_err)?;
        match f(self) {
            Ok(v) => {
                self.db.execute_batch("COMMIT").map_err(db_err)?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.db.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}
