//! Disk space and portability: what the Library folder holds and how big each part is,
//! its Settings (preview size and quality), clearing what can be rebuilt, finding and
//! relinking originals that moved, estimating an Import, and moving the whole Library.
//! See ADR 0011.

use super::import::{frames, kind_of, walk, write_preview, ImportFailure};
use super::{
    db_err, store_path, Library, PhotoId, CACHE_DIR, DB_FILE, ORIGINALS_DIR, PREVIEWS_DIR,
};
use crate::{Error, Result};
use rusqlite::params;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Per-Library settings, kept inside the Library so they travel with it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Longest edge of the preview JPEGs Import makes (pixels).
    pub preview_long_edge: u32,
    /// JPEG quality of those previews.
    pub preview_quality: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            preview_long_edge: 2048,
            preview_quality: 85,
        }
    }
}

pub const PREVIEW_EDGES: std::ops::RangeInclusive<u32> = 512..=4096;
pub const PREVIEW_QUALITIES: std::ops::RangeInclusive<u8> = 50..=95;

/// Bytes and files under one part of the Library.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Usage {
    pub bytes: u64,
    pub files: u64,
}

impl Usage {
    fn add(&mut self, other: Usage) {
        self.bytes += other.bytes;
        self.files += other.files;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Storage {
    pub library: PathBuf,
    pub photos: i64,
    /// Originals copied into the Library's `originals/` folder.
    pub originals_in_library: Usage,
    /// Originals the Library reads where they are (outside its folder). Not part of the
    /// Library's size on disk, and offline when their disk is.
    pub originals_elsewhere: Usage,
    /// Photos whose original can't be found right now (moved, deleted, or on a disk that
    /// isn't connected). Previews still work; relink with `relink_originals`.
    pub missing: i64,
    pub previews: Usage,
    /// Safe to clear at any time: rebuildable files and previews no Photo uses.
    pub cache: Usage,
    pub database: Usage,
    /// Automatic copies of the database (Verdicts, Recipes, Looks; never photos).
    pub backups: Usage,
    /// Everything inside the Library folder.
    pub total: Usage,
    /// Free space on the disk the Library is on.
    pub free_bytes: Option<u64>,
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissingOriginal {
    pub photo_id: PhotoId,
    pub file_name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelinkReport {
    pub relinked: usize,
    pub still_missing: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RebuildReport {
    pub rebuilt: usize,
    /// Photos skipped because their original is missing; they keep their old preview.
    pub offline: usize,
    pub failed: Vec<ImportFailure>,
    pub previews: Usage,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportEstimate {
    pub photos: usize,
    /// Size of the originals being imported.
    pub original_bytes: u64,
    /// Whether this Import copies the originals into the Library.
    pub copy: bool,
    /// Bytes the copies take (0 when importing in place).
    pub copy_bytes: u64,
    /// Estimated bytes of the new previews.
    pub preview_bytes: u64,
    /// What the Library's disk needs: copies plus previews.
    pub needed_bytes: u64,
    /// Free space on the Library's disk.
    pub free_bytes: Option<u64>,
    pub library: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoveReport {
    pub from: PathBuf,
    pub to: PathBuf,
    pub moved: Usage,
    /// False when the move was a rename on the same disk; true when every file was copied
    /// to another disk, verified by hash, and the old folder removed.
    pub copied: bool,
}

/// Size of everything under `dir` (0 when it doesn't exist).
fn usage(dir: &Path) -> Usage {
    let mut u = Usage::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return u;
    };
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            u.add(usage(&e.path()));
        } else {
            u.add(Usage {
                bytes: meta.len(),
                files: 1,
            });
        }
    }
    u
}

/// Free bytes on the disk holding `path`.
pub fn free_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let mut p = path.to_path_buf();
        while !p.exists() {
            p = p.parent()?.to_path_buf();
        }
        let c = CString::new(p.as_os_str().as_bytes()).ok()?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is a valid NUL-terminated path and `st` a properly sized out-param.
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        #[allow(clippy::unnecessary_cast)]
        Some(st.f_bavail as u64 * st.f_frsize as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

impl Library {
    pub fn settings(&self) -> Result<Settings> {
        let mut q = self
            .db
            .prepare("SELECT key, value FROM settings")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        let mut map = serde_json::Map::new();
        for row in rows {
            let (k, v) = row.map_err(db_err)?;
            if let Ok(v) = serde_json::from_str(&v) {
                map.insert(k, v);
            }
        }
        let mut s = serde_json::to_value(Settings::default()).expect("Settings serialize");
        for (k, v) in map {
            s[k] = v;
        }
        Ok(serde_json::from_value(s).unwrap_or_default())
    }

    /// Save Settings. Out-of-range values are an error and nothing changes.
    pub fn set_settings(&mut self, s: &Settings) -> Result<()> {
        if !PREVIEW_EDGES.contains(&s.preview_long_edge) {
            return Err(Error(format!(
                "preview_long_edge must be {}-{} pixels",
                PREVIEW_EDGES.start(),
                PREVIEW_EDGES.end()
            )));
        }
        if !PREVIEW_QUALITIES.contains(&s.preview_quality) {
            return Err(Error(format!(
                "preview_quality must be {}-{}",
                PREVIEW_QUALITIES.start(),
                PREVIEW_QUALITIES.end()
            )));
        }
        let tx = self.db.transaction().map_err(db_err)?;
        let v = serde_json::to_value(s).expect("Settings serialize");
        for (k, v) in v.as_object().expect("Settings is an object") {
            tx.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                params![k, v.to_string()],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    /// Every Photo's stored original path and preview.
    fn originals(&self) -> Result<Vec<(PhotoId, String, String, String)>> {
        let mut q = self
            .db
            .prepare("SELECT id, path, file_name, preview FROM photos ORDER BY id")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Previews no Photo refers to any more (left by an interrupted Import or rebuild).
    fn orphan_previews(&self) -> Result<Vec<PathBuf>> {
        let used: HashSet<PathBuf> = self
            .originals()?
            .into_iter()
            .map(|(.., preview)| self.root.join(preview))
            .collect();
        let mut files = Vec::new();
        list_files(&self.root.join(PREVIEWS_DIR), &mut files);
        Ok(files.into_iter().filter(|f| !used.contains(f)).collect())
    }

    /// How much space each part of the Library takes, and what's missing.
    pub fn storage(&self) -> Result<Storage> {
        let mut elsewhere = Usage::default();
        let mut missing = 0;
        let mut q = self
            .db
            .prepare("SELECT path, bytes FROM photos")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(db_err)?;
        let mut photos = 0;
        for row in rows {
            let (stored, bytes) = row.map_err(db_err)?;
            photos += 1;
            let path = self.resolve(&stored);
            if Path::new(&stored).is_absolute() {
                elsewhere.add(Usage {
                    bytes: bytes as u64,
                    files: 1,
                });
            }
            if !path.is_file() {
                missing += 1;
            }
        }
        let mut cache = usage(&self.root.join(CACHE_DIR));
        let mut previews = usage(&self.root.join(PREVIEWS_DIR));
        for orphan in self.orphan_previews()? {
            let bytes = std::fs::metadata(&orphan).map(|m| m.len()).unwrap_or(0);
            let u = Usage { bytes, files: 1 };
            cache.add(u);
            previews.bytes -= u.bytes.min(previews.bytes);
            previews.files -= 1.min(previews.files);
        }
        let mut database = Usage::default();
        for suffix in ["", "-wal", "-shm"] {
            if let Ok(m) = std::fs::metadata(self.root.join(format!("{DB_FILE}{suffix}"))) {
                database.add(Usage {
                    bytes: m.len(),
                    files: 1,
                });
            }
        }
        Ok(Storage {
            library: self.root.clone(),
            photos,
            originals_in_library: usage(&self.root.join(ORIGINALS_DIR)),
            originals_elsewhere: elsewhere,
            missing,
            previews,
            cache,
            database,
            backups: usage(&self.root.join(super::backup::DIR)),
            total: usage(&self.root),
            free_bytes: free_bytes(&self.root),
            settings: self.settings()?,
        })
    }

    /// Delete everything in `cache/` and any preview no Photo uses. Returns what it freed.
    pub fn clear_cache(&self) -> Result<Usage> {
        let mut freed = usage(&self.root.join(CACHE_DIR));
        let cache = self.root.join(CACHE_DIR);
        if cache.exists() {
            std::fs::remove_dir_all(&cache).map_err(|e| Error(format!("clear cache: {e}")))?;
        }
        self.clear_facts()?;
        for orphan in self.orphan_previews()? {
            let bytes = std::fs::metadata(&orphan).map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(&orphan).is_ok() {
                freed.add(Usage { bytes, files: 1 });
            }
        }
        Ok(freed)
    }

    /// Photos whose original isn't where the Library expects it.
    pub fn missing_originals(&self) -> Result<Vec<MissingOriginal>> {
        Ok(self
            .originals()?
            .into_iter()
            .filter(|(_, stored, ..)| !self.resolve(stored).is_file())
            .map(|(id, stored, file_name, _)| MissingOriginal {
                photo_id: id,
                file_name,
                path: self.resolve(&stored).to_string_lossy().into_owned(),
            })
            .collect())
    }

    /// Find missing originals under `folder` (recursively) by name and size, confirm each
    /// by content hash, and point the Library at them. Nothing is moved or written there.
    pub fn relink(&mut self, folder: &Path) -> Result<RelinkReport> {
        let missing = self.missing_originals()?;
        if missing.is_empty() {
            return Ok(RelinkReport {
                relinked: 0,
                still_missing: 0,
            });
        }
        let folder = folder
            .canonicalize()
            .map_err(|e| Error(format!("{}: {e}", folder.display())))?;
        let mut files = Vec::new();
        walk(&folder, &mut files)?;
        let mut by_key: HashMap<(String, u64), Vec<PathBuf>> = HashMap::new();
        for f in files {
            let (Some(name), Ok(meta)) = (f.file_name(), std::fs::metadata(&f)) else {
                continue;
            };
            by_key
                .entry((name.to_string_lossy().to_lowercase(), meta.len()))
                .or_default()
                .push(f);
        }
        let mut relinked = 0;
        for m in &missing {
            let (bytes, hash): (i64, String) = self
                .db
                .query_row(
                    "SELECT bytes, hash FROM photos WHERE id = ?1",
                    [m.photo_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(db_err)?;
            let key = (m.file_name.to_lowercase(), bytes as u64);
            let found = by_key.get(&key).into_iter().flatten().find(|f| {
                std::fs::read(f)
                    .map(|d| blake3::hash(&d).to_hex().as_str() == hash)
                    .unwrap_or(false)
            });
            if let Some(f) = found {
                self.db
                    .execute(
                        "UPDATE photos SET path = ?1 WHERE id = ?2",
                        params![store_path(&self.root, f), m.photo_id],
                    )
                    .map_err(db_err)?;
                relinked += 1;
            }
        }
        Ok(RelinkReport {
            relinked,
            still_missing: missing.len() - relinked,
        })
    }

    /// Re-make previews at the current Settings (after changing preview size or quality).
    /// About a second per RAW; Photos whose original is missing keep their old preview.
    pub fn rebuild_previews(&self, max_in_flight: usize) -> Result<RebuildReport> {
        let settings = self.settings()?;
        let photos = self.originals()?;
        let next = AtomicUsize::new(0);
        let (rebuilt, offline) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let failed = Mutex::new(Vec::new());
        let root = &self.root;
        std::thread::scope(|s| {
            for _ in 0..max_in_flight.clamp(1, photos.len().max(1)) {
                s.spawn(|| {
                    while let Some((_, stored, _, preview)) =
                        photos.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        let path = super::resolve_path(root, stored);
                        let Ok(data) = std::fs::read(&path) else {
                            offline.fetch_add(1, Ordering::Relaxed);
                            continue;
                        };
                        let kind = kind_of(&path).unwrap_or("jpeg");
                        match write_preview(
                            &path,
                            &data,
                            kind,
                            settings.preview_long_edge as usize,
                            settings.preview_quality,
                            &root.join(preview),
                        ) {
                            Ok(_) => {
                                rebuilt.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => failed.lock().unwrap().push(ImportFailure {
                                file: path.to_string_lossy().into_owned(),
                                error: e.0,
                            }),
                        }
                    }
                });
            }
        });
        Ok(RebuildReport {
            rebuilt: rebuilt.into_inner(),
            offline: offline.into_inner(),
            failed: failed.into_inner().unwrap(),
            previews: usage(&self.root.join(PREVIEWS_DIR)),
        })
    }

    /// How much disk an Import of `folder` (or just `files` in it) would take, from file
    /// sizes alone. Previews are estimated from this Library's own average, or from the
    /// preview size when it has none yet.
    pub fn estimate_import(
        &self,
        folder: &Path,
        files: Option<&[PathBuf]>,
        copy: bool,
        raw_only: bool,
    ) -> Result<ImportEstimate> {
        let files = match files {
            Some(f) => f.to_vec(),
            None => {
                let mut all = Vec::new();
                walk(folder, &mut all)?;
                all
            }
        };
        // The same frames Import makes, so a RAW+JPEG frame counts its JPEG unless RAW only.
        let frames = frames(files, raw_only);
        let files: Vec<&PathBuf> = frames.iter().map(|(f, _)| f).collect();
        let original_bytes: u64 = frames
            .iter()
            .flat_map(|(f, j)| std::iter::once(f).chain(j))
            .map(|f| {
                std::fs::metadata(f)
                    .map(|m| m.len())
                    .map_err(|e| Error(format!("{}: {e}", f.display())))
            })
            .sum::<Result<u64>>()?;
        let settings = self.settings()?;
        let existing = usage(&self.root.join(PREVIEWS_DIR));
        let per_preview = if existing.files >= 5 {
            existing.bytes / existing.files
        } else {
            // A 3:2 frame at about 0.3 bytes per pixel, typical of q85 photos.
            let edge = settings.preview_long_edge as f64;
            (edge * edge * 2.0 / 3.0 * 0.3 * settings.preview_quality as f64 / 85.0) as u64
        };
        let copy_bytes = if copy { original_bytes } else { 0 };
        let preview_bytes = per_preview * files.len() as u64;
        Ok(ImportEstimate {
            photos: files.len(),
            original_bytes,
            copy,
            copy_bytes,
            preview_bytes,
            needed_bytes: copy_bytes + preview_bytes,
            free_bytes: free_bytes(&self.root),
            library: self.root.clone(),
        })
    }
}

fn list_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            list_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Move the Library folder `from` to `to` (a new path, or an empty folder). On the same
/// disk it is a rename. Across disks every file is copied, checked by size and hash, and
/// only then is the old folder removed; any mismatch removes the copy and keeps the old.
/// Close the Library everywhere first: nothing may write to it while it moves.
pub fn move_library(from: &Path, to: &Path) -> Result<MoveReport> {
    move_library_via(from, to, true)
}

/// [`move_library`], optionally never renaming, so tests can take the other-disk path.
#[doc(hidden)]
pub fn move_library_via(from: &Path, to: &Path, rename: bool) -> Result<MoveReport> {
    if !Library::is_library(from) {
        return Err(Error(format!("{} is not an Argus Library", from.display())));
    }
    let from = from
        .canonicalize()
        .map_err(|e| Error(format!("{}: {e}", from.display())))?;
    let parent = to
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| Error(format!("{} has no parent folder", to.display())))?;
    let parent = parent
        .canonicalize()
        .map_err(|e| Error(format!("{}: {e}", parent.display())))?;
    let to = parent.join(to.file_name().unwrap_or_default());
    if to.starts_with(&from) {
        return Err(Error("can't move a Library inside itself".into()));
    }
    if to.exists() {
        // macOS drops .DS_Store into folders the person merely looked at in Finder.
        let empty = std::fs::read_dir(&to)
            .map(|mut d| d.all(|e| e.is_ok_and(|e| e.file_name() == ".DS_Store")))
            .unwrap_or(false);
        if !empty {
            return Err(Error(format!(
                "{} already exists and isn't empty; pick a new or empty folder",
                to.display()
            )));
        }
        std::fs::remove_dir_all(&to).map_err(|e| Error(format!("{}: {e}", to.display())))?;
    }
    // Fold the write-ahead log into library.db so the folder is complete on its own.
    {
        let db = rusqlite::Connection::open(from.join(DB_FILE)).map_err(db_err)?;
        // Background work writes into the folder; let it finish first (ADR 0016).
        let running: Option<String> = db
            .query_row(
                "SELECT label FROM jobs WHERE status = 'running' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if let Some(label) = running {
            return Err(Error(format!(
                "Argus is still working in this Library ({label}); pause the background work or wait for it, then move"
            )));
        }
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(db_err)?;
    }
    let moved = usage(&from);
    if rename && std::fs::rename(&from, &to).is_ok() {
        return Ok(MoveReport {
            from,
            to,
            moved,
            copied: false,
        });
    }
    let copied = copy_tree(&from, &to).and_then(|_| verify_tree(&from, &to));
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&to);
        return Err(Error(format!(
            "move failed, the Library is still at {}: {}",
            from.display(),
            e.0
        )));
    }
    std::fs::remove_dir_all(&from).map_err(|e| {
        Error(format!(
            "the Library was copied to {} but the old folder couldn't be removed: {e}",
            to.display()
        ))
    })?;
    Ok(MoveReport {
        from,
        to,
        moved,
        copied: true,
    })
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|e| Error(format!("{}: {e}", to.display())))?;
    let entries = std::fs::read_dir(from).map_err(|e| Error(format!("{}: {e}", from.display())))?;
    for e in entries.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| Error(format!("{}: {e}", src.display())))?;
        }
    }
    Ok(())
}

fn verify_tree(from: &Path, to: &Path) -> Result<()> {
    let mut files = Vec::new();
    list_files(from, &mut files);
    for src in files {
        let dst = to.join(src.strip_prefix(from).expect("listed under from"));
        let hash = |p: &Path| std::fs::read(p).map(|d| blake3::hash(&d));
        match (hash(&src), hash(&dst)) {
            (Ok(a), Ok(b)) if a == b => {}
            _ => return Err(Error(format!("{} did not copy intact", dst.display()))),
        }
    }
    Ok(())
}
