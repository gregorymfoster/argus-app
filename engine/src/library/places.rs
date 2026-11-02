//! Backup places: copies of the whole Library kept somewhere else (an external disk, the
//! Google Drive or iCloud Drive app's folder, a NAS), so losing the Mac's disk loses
//! nothing. Every Backup place is just a folder: Argus writes files and the Drive or iCloud
//! app uploads them, so there is no sign-in and no API key. See ADR 0017.
//!
//! A Backup place's copy lives in `<folder>/Argus-Backup/<Library folder name>/` and has
//! the Library's own layout: `library.db` (the newest copy of the database), `backups/`
//! (dated older copies, which the Library's own restore lists), `originals/`, optionally
//! `previews/`, and `elsewhere/` for originals the Library reads where they are. So the
//! copy opens as a working Library on any Mac. `argus-backup.json` beside them records
//! each file's size and hash.
//!
//! A run never deletes or overwrites anything in the copy except `library.db` and the
//! manifest: originals and previews never change once written (originals are never
//! written at all, previews are named by content), so a file already in the manifest at the
//! same size is skipped. Every new file goes to a temporary name, is read back and checked
//! against the hash of what was read from the Library, and only then renamed into place.

use super::{backup, db_err, resolve_path, time, Library, PhotoId, ShootId, PREVIEWS_DIR};

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// The folder Argus makes inside a Backup place's folder.
pub const FOLDER: &str = "Argus-Backup";
pub const MANIFEST: &str = "argus-backup.json";
/// Where originals the Library reads in place go inside a copy.
pub const ELSEWHERE: &str = "elsewhere";
const PLACES_KEY: &str = "backup_places";
const STATE_KEY: &str = "backup_state:";
const ID_KEY: &str = "library_id";
/// Runs kept in a Backup place's history.
const HISTORY: usize = 50;
/// Database copies are dated at most once a day in the copy's `backups/`.
const SNAPSHOT_EVERY: Duration = Duration::from_secs(20 * 3600);

/// What a Backup place copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Copies {
    /// The database only: every Verdict, Recipe and Look. Small.
    Edits,
    /// Originals and the database: everything that can't be rebuilt.
    #[default]
    Originals,
    /// Previews too, so the copy opens instantly on another Mac.
    Everything,
}

/// When a Backup place runs on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum When {
    /// After each Import (that Shoot only) and on Finish Shoot.
    #[default]
    AfterImport,
    /// Once a day.
    Daily,
    /// Whenever its disk is connected and the last run is a day old.
    OnConnect,
    /// Only when the person asks.
    Manual,
}

/// A Backup place, as the person set it up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
    /// The folder the person chose; the copy is inside it, see [`Place::copy_dir`].
    pub folder: PathBuf,
    #[serde(default)]
    pub copies: Copies,
    /// Also copy originals the Library reads where they are (outside its folder).
    #[serde(default)]
    pub include_elsewhere: bool,
    #[serde(default)]
    pub when: When,
    /// Dated database copies kept in the copy's `backups/` (0 keeps only `library.db`).
    #[serde(default = "default_keep")]
    pub keep_snapshots: usize,
    /// Re-read every file once a month and check it against its hash.
    #[serde(default = "yes")]
    pub verify_monthly: bool,
    /// The copy's folder name inside `<folder>/Argus-Backup/`, fixed when the place is added.
    #[serde(default)]
    pub copy_name: String,
}

fn default_keep() -> usize {
    30
}
fn yes() -> bool {
    true
}

impl Place {
    /// Where this place's copy of the Library lives.
    pub fn copy_dir(&self) -> PathBuf {
        self.folder.join(FOLDER).join(&self.copy_name)
    }
    /// Whether the place's folder is there now (its disk connected).
    pub fn available(&self) -> bool {
        self.folder.is_dir()
    }
}

/// One run, for the history list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// UTC.
    pub at: String,
    /// "Import · 2026-11-02_Harbour-Walk", "Daily", "By hand", "Monthly check".
    pub why: String,
    pub files: u64,
    pub bytes: u64,
    pub secs: f64,
    /// "verified", "all match", "3 differ", "stopped at 40%", or the error.
    pub check: String,
    pub ok: bool,
}

/// What Argus knows about a place's copy, kept in the Library so it shows while the disk
/// is away. Written only by runs; the person's settings are in [`Place`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub last_ok: Option<String>,
    pub last_verified: Option<String>,
    pub last_error: Option<String>,
    /// Photos whose original is in the copy, as inclusive id ranges.
    pub covered: Vec<(PhotoId, PhotoId)>,
    /// Bytes and files in the copy, from its manifest.
    pub bytes: u64,
    pub files: u64,
    pub history: Vec<Run>,
}

/// A place with its live status, for Settings.
#[derive(Debug, Clone, Serialize)]
pub struct PlaceStatus {
    #[serde(flatten)]
    pub place: Place,
    pub copy_dir: PathBuf,
    pub kind: Kind,
    pub available: bool,
    pub free_bytes: Option<u64>,
    #[serde(flatten)]
    pub state: State,
    /// Photos whose original is in the copy.
    pub photos_in: u64,
    /// Photos this place should hold that it doesn't yet (0 for an Edits-only place).
    pub waiting: u64,
    /// "up to date", "backing up", "waiting", "not connected", "failed", "never run".
    pub status: String,
}

/// What kind of place a folder is, for its icon and its speed estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    GoogleDrive,
    Icloud,
    Dropbox,
    OneDrive,
    ExternalDisk,
    NetworkDisk,
    Folder,
}

impl Kind {
    pub fn of(path: &Path) -> Kind {
        let s = path.to_string_lossy();
        if s.contains("/Library/CloudStorage/GoogleDrive-") {
            Kind::GoogleDrive
        } else if s.contains("/Library/Mobile Documents/com~apple~CloudDocs") {
            Kind::Icloud
        } else if s.contains("/Library/CloudStorage/Dropbox") || s.contains("/Dropbox") {
            Kind::Dropbox
        } else if s.contains("/Library/CloudStorage/OneDrive") {
            Kind::OneDrive
        } else if s.starts_with("/Volumes/") {
            if network_volume(path) {
                Kind::NetworkDisk
            } else {
                Kind::ExternalDisk
            }
        } else {
            Kind::Folder
        }
    }

    /// Bytes per second Argus can expect to write there (cloud folders are local disk; the
    /// app uploads afterwards).
    fn write_rate(self) -> f64 {
        match self {
            Kind::NetworkDisk => 40e6,
            Kind::ExternalDisk => 150e6,
            _ => 400e6,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::GoogleDrive => "Google Drive",
            Kind::Icloud => "iCloud Drive",
            Kind::Dropbox => "Dropbox",
            Kind::OneDrive => "OneDrive",
            Kind::ExternalDisk => "External disk",
            Kind::NetworkDisk => "Network disk",
            Kind::Folder => "Folder",
        }
    }
}

#[cfg(target_os = "macos")]
fn network_volume(path: &Path) -> bool {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a properly sized out-param.
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return false;
    }
    // SAFETY: f_fstypename is NUL-terminated by the kernel.
    let ty = unsafe { CStr::from_ptr(st.f_fstypename.as_ptr()) }.to_string_lossy();
    matches!(ty.as_ref(), "smbfs" | "afpfs" | "nfs" | "webdav")
}

#[cfg(not(target_os = "macos"))]
fn network_volume(_: &Path) -> bool {
    false
}

/// A folder Argus found that would make a good Backup place.
#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub name: String,
    pub folder: PathBuf,
    pub kind: Kind,
    pub free_bytes: Option<u64>,
    /// Already a Backup place of this Library.
    pub in_use: bool,
    /// Something to know first, such as iCloud's Optimize Mac Storage.
    pub note: Option<String>,
}

/// How big a first backup to a folder would be.
#[derive(Debug, Clone, Serialize)]
pub struct Estimate {
    pub files: u64,
    pub bytes: u64,
    pub free_bytes: Option<u64>,
    /// Seconds to write it, roughly. A cloud folder's upload comes after, at the app's pace.
    pub secs: f64,
    pub fits: bool,
}

/// What a run did.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub place: String,
    pub copy_dir: PathBuf,
    pub copied_files: u64,
    pub copied_bytes: u64,
    /// Files already in the copy.
    pub skipped: u64,
    /// Files that could not be copied (unreadable, or a different file already there).
    pub failed: Vec<String>,
    /// False when the time budget ran out first; the next run carries on.
    pub done: bool,
    /// With `verify`: files whose bytes no longer match their hash.
    pub differ: Vec<String>,
    pub verified_files: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Manifest {
    library_id: String,
    #[serde(default)]
    library: String,
    #[serde(default)]
    updated: String,
    #[serde(default)]
    files: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    bytes: u64,
    hash: String,
}

fn io(what: &Path, e: std::io::Error) -> Error {
    Error(format!("{}: {e}", what.display()))
}

/// Inclusive id ranges from a set of ids.
fn ranges(ids: &mut [PhotoId]) -> Vec<(PhotoId, PhotoId)> {
    ids.sort_unstable();
    let mut out: Vec<(PhotoId, PhotoId)> = Vec::new();
    for &id in ids.iter() {
        match out.last_mut() {
            Some((_, hi)) if id <= *hi + 1 => *hi = (*hi).max(id),
            _ => out.push((id, id)),
        }
    }
    out
}

fn in_ranges(r: &[(PhotoId, PhotoId)], id: PhotoId) -> bool {
    r.binary_search_by(|&(lo, hi)| {
        if hi < id {
            std::cmp::Ordering::Less
        } else if lo > id {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    })
    .is_ok()
}

/// A file name that is safe and readable: the folder's last part, or a fallback.
fn slug(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "place".into()
    } else {
        s
    }
}

/// One file to copy: where it is in the Library and where it goes in the copy.
struct Item {
    src: PathBuf,
    rel: String,
    bytes: u64,
    photo: Option<PhotoId>,
}

impl Library {
    /// This Library's id, made the first time it's asked for. A copy remembers it, so two
    /// Libraries with the same folder name never share one.
    pub fn library_id(&self) -> Result<String> {
        if let Some(Value::String(id)) = self.setting(ID_KEY)? {
            return Ok(id);
        }
        let seed = format!(
            "{}:{}:{:?}",
            self.root.display(),
            std::process::id(),
            SystemTime::now()
        );
        let id = blake3::hash(seed.as_bytes()).to_hex()[..16].to_string();
        self.set_setting(ID_KEY, &json!(id))?;
        Ok(id)
    }

    /// The Library's Backup places, as the person set them up.
    pub fn backup_places(&self) -> Result<Vec<Place>> {
        Ok(self
            .setting(PLACES_KEY)?
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default())
    }

    fn save_places(&self, places: &[Place]) -> Result<()> {
        self.set_setting(
            PLACES_KEY,
            &serde_json::to_value(places).expect("places serialize"),
        )
    }

    pub fn backup_place(&self, id: &str) -> Result<Place> {
        self.backup_places()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error(format!("no Backup place {id:?}")))
    }

    fn place_state(&self, id: &str) -> Result<State> {
        Ok(self
            .setting(&format!("{STATE_KEY}{id}"))?
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default())
    }

    fn save_state(&self, id: &str, s: &State) -> Result<()> {
        self.set_setting(
            &format!("{STATE_KEY}{id}"),
            &serde_json::to_value(s).expect("state serializes"),
        )
    }

    /// Add a Backup place. The folder must exist; the copy goes in `Argus-Backup/` inside
    /// it. A folder that already holds another Library's copy of the same name gets `-2`.
    pub fn add_backup_place(&self, mut place: Place) -> Result<Place> {
        if !place.folder.is_absolute() {
            return Err(Error("a Backup place needs a full folder path".into()));
        }
        if !place.folder.is_dir() {
            return Err(Error(format!(
                "{} isn't there (is its disk connected?)",
                place.folder.display()
            )));
        }
        let root = self.root.canonicalize().unwrap_or(self.root.clone());
        let folder = place.folder.canonicalize().unwrap_or(place.folder.clone());
        if folder.starts_with(&root) || root.starts_with(folder.join(FOLDER)) {
            return Err(Error(
                "a Backup place can't be inside the Library it backs up".into(),
            ));
        }
        let mut places = self.backup_places()?;
        if places.iter().any(|p| p.folder == place.folder) {
            return Err(Error(format!(
                "{} is already a Backup place",
                place.folder.display()
            )));
        }
        if place.name.trim().is_empty() {
            place.name = default_name(&place.folder);
        }
        let mut id = slug(&place.name).to_lowercase();
        let base = id.clone();
        let mut n = 2;
        while places.iter().any(|p| p.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        place.id = id;
        let me = self.library_id()?;
        let base = slug(
            &self
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "library".into()),
        );
        let mut name = base.clone();
        let mut n = 2;
        loop {
            let dir = place.folder.join(FOLDER).join(&name);
            match read_manifest(&dir) {
                Some(m) if m.library_id != me => {
                    name = format!("{base}-{n}");
                    n += 1;
                }
                _ => break,
            }
        }
        place.copy_name = name;
        places.push(place.clone());
        self.save_places(&places)?;
        Ok(place)
    }

    /// Change a Backup place's settings (not its folder or id).
    pub fn update_backup_place(&self, place: &Place) -> Result<Place> {
        let mut places = self.backup_places()?;
        let p = places
            .iter_mut()
            .find(|p| p.id == place.id)
            .ok_or_else(|| Error(format!("no Backup place {:?}", place.id)))?;
        p.name = place.name.clone();
        p.copies = place.copies;
        p.include_elsewhere = place.include_elsewhere;
        p.when = place.when;
        p.keep_snapshots = place.keep_snapshots;
        p.verify_monthly = place.verify_monthly;
        let out = p.clone();
        self.save_places(&places)?;
        Ok(out)
    }

    /// Stop backing up to a place. Its copy stays where it is: Argus never deletes a backup.
    pub fn remove_backup_place(&self, id: &str) -> Result<Place> {
        let mut places = self.backup_places()?;
        let i = places
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| Error(format!("no Backup place {id:?}")))?;
        let p = places.remove(i);
        self.save_places(&places)?;
        self.db
            .execute(
                "DELETE FROM settings WHERE key = ?1",
                [format!("{STATE_KEY}{id}")],
            )
            .map_err(db_err)?;
        Ok(p)
    }

    /// Every Backup place with its live status: connected, free space, how many Photos
    /// are in it and how many wait. `running` names places with a backup running now.
    pub fn backup_status(&self, running: &[String]) -> Result<Vec<PlaceStatus>> {
        let photos = self.photo_paths(None)?;
        let mut out = Vec::new();
        for place in self.backup_places()? {
            let state = self.place_state(&place.id)?;
            let available = place.available();
            let photos_in = photos
                .iter()
                .filter(|(id, ..)| in_ranges(&state.covered, *id))
                .count() as u64;
            let waiting = if place.copies == Copies::Edits {
                0
            } else {
                photos
                    .iter()
                    .filter(|(id, path, _)| {
                        (place.include_elsewhere || !Path::new(path).is_absolute())
                            && !in_ranges(&state.covered, *id)
                    })
                    .count() as u64
            };
            let status = if running.contains(&place.id) {
                "backing up"
            } else if !available {
                "not connected"
            } else if state.last_error.is_some() {
                "failed"
            } else if state.last_ok.is_none() {
                "never run"
            } else if waiting > 0 {
                "waiting"
            } else {
                "up to date"
            };
            out.push(PlaceStatus {
                copy_dir: place.copy_dir(),
                kind: Kind::of(&place.folder),
                free_bytes: if available {
                    super::storage::free_bytes(&place.folder)
                } else {
                    None
                },
                available,
                photos_in,
                waiting,
                status: status.into(),
                place,
                state,
            });
        }
        Ok(out)
    }

    pub fn photo_count(&self) -> Result<u64> {
        self.db
            .query_row("SELECT COUNT(*) FROM photos", [], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)
            .map_err(db_err)
    }

    /// Photos with at least `n` Backup places holding their original.
    pub fn photos_backed_up(&self, n: usize) -> Result<u64> {
        let states: Vec<State> = self
            .backup_places()?
            .iter()
            .filter(|p| p.copies != Copies::Edits)
            .map(|p| self.place_state(&p.id))
            .collect::<Result<_>>()?;
        Ok(self
            .photo_paths(None)?
            .iter()
            .filter(|(id, ..)| states.iter().filter(|s| in_ranges(&s.covered, *id)).count() >= n)
            .count() as u64)
    }

    /// (id, stored path, bytes) of every Photo, or one Shoot's.
    fn photo_paths(&self, shoot: Option<ShootId>) -> Result<Vec<(PhotoId, String, u64)>> {
        let mut q = self
            .db
            .prepare(
                "SELECT id, path, bytes FROM photos WHERE ?1 IS NULL OR shoot_id = ?1 ORDER BY id",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map([shoot], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)?.max(0) as u64))
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// The files a place should hold: originals (and previews), with where each goes.
    fn backup_items(&self, place: &Place, shoot: Option<ShootId>) -> Result<Vec<Item>> {
        let mut items = Vec::new();
        if place.copies == Copies::Edits {
            return Ok(items);
        }
        for (id, stored, bytes) in self.photo_paths(shoot)? {
            let p = Path::new(&stored);
            let rel = if p.is_absolute() {
                if !place.include_elsewhere {
                    continue;
                }
                format!("{ELSEWHERE}/{}", stored.trim_start_matches('/'))
            } else {
                stored.clone()
            };
            items.push(Item {
                src: resolve_path(&self.root, &stored),
                rel,
                bytes,
                photo: Some(id),
            });
        }
        if place.copies == Copies::Everything && shoot.is_none() {
            let mut files = Vec::new();
            super::storage::list_files(&self.root.join(PREVIEWS_DIR), &mut files);
            for f in files {
                let Ok(rel) = f.strip_prefix(&self.root) else {
                    continue;
                };
                let rel = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                if rel.split('/').any(|s| s.starts_with('.')) {
                    continue;
                }
                let bytes = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                items.push(Item {
                    src: f,
                    rel,
                    bytes,
                    photo: None,
                });
            }
        }
        Ok(items)
    }

    /// How big a first backup of this Library to `folder` would be.
    pub fn estimate_backup(
        &self,
        folder: &Path,
        copies: Copies,
        include_elsewhere: bool,
    ) -> Result<Estimate> {
        let place = Place {
            id: String::new(),
            name: String::new(),
            folder: folder.to_path_buf(),
            copies,
            include_elsewhere,
            when: When::Manual,
            keep_snapshots: 0,
            verify_monthly: false,
            copy_name: String::new(),
        };
        let items = self.backup_items(&place, None)?;
        let db = std::fs::metadata(self.root.join(super::DB_FILE))
            .map(|m| m.len())
            .unwrap_or(0);
        let bytes = items.iter().map(|i| i.bytes).sum::<u64>() + db;
        let free = super::storage::free_bytes(folder);
        Ok(Estimate {
            files: items.len() as u64 + 1,
            bytes,
            free_bytes: free,
            secs: bytes as f64 / Kind::of(folder).write_rate(),
            fits: free.is_none_or(|f| f > bytes),
        })
    }

    /// Back the Library up to a place: a fresh copy of the database, then every original
    /// (and preview) the copy doesn't hold yet, each verified by hash. With `shoot`, only
    /// that Shoot's originals (plus the database). With `budget`, stops when it runs out and
    /// says so (`done: false`); running again carries on. With `verify`, re-reads every file
    /// in the copy and checks it against its hash instead of copying.
    /// `progress(done_bytes, total_bytes)`.
    pub fn run_backup(
        &self,
        id: &str,
        shoot: Option<ShootId>,
        verify: bool,
        why: &str,
        budget: Option<Duration>,
        progress: &dyn Fn(u64, u64),
    ) -> Result<Report> {
        let place = self.backup_place(id)?;
        let started = Instant::now();
        let mut state = self.place_state(id)?;
        let result = if verify {
            self.verify_copy(&place, progress)
        } else {
            self.copy_to(&place, shoot, budget, progress)
        };
        let secs = started.elapsed().as_secs_f64();
        let now = time::now();
        let run = match &result {
            Ok(r) => {
                let check = if verify {
                    if r.differ.is_empty() {
                        "all match".to_string()
                    } else {
                        format!("{} differ", r.differ.len())
                    }
                } else if !r.failed.is_empty() {
                    format!("{} failed", r.failed.len())
                } else if !r.done {
                    "carries on next run".to_string()
                } else {
                    "verified".to_string()
                };
                Run {
                    at: now.clone(),
                    why: why.to_string(),
                    files: if verify {
                        r.verified_files
                    } else {
                        r.copied_files
                    },
                    bytes: r.copied_bytes,
                    secs,
                    ok: r.failed.is_empty() && r.differ.is_empty(),
                    check,
                }
            }
            Err(e) => Run {
                at: now.clone(),
                why: why.to_string(),
                files: 0,
                bytes: 0,
                secs,
                check: e.0.clone(),
                ok: false,
            },
        };
        // Fold this run into what we know about the copy.
        if let Some(m) = read_manifest(&place.copy_dir()) {
            state.bytes = m.files.values().map(|e| e.bytes).sum();
            state.files = m.files.len() as u64;
            let mut ids: Vec<PhotoId> = self
                .backup_items(
                    &Place {
                        copies: Copies::Originals,
                        include_elsewhere: true,
                        ..place.clone()
                    },
                    None,
                )?
                .into_iter()
                .filter(|i| m.files.contains_key(&i.rel))
                .filter_map(|i| i.photo)
                .collect();
            state.covered = ranges(&mut ids);
        }
        match &result {
            Ok(r) if r.failed.is_empty() && r.differ.is_empty() => {
                state.last_error = None;
                if verify {
                    state.last_verified = Some(now);
                } else if r.done {
                    state.last_ok = Some(now);
                }
            }
            Ok(_) => state.last_error = Some(run.check.clone()),
            Err(e) => state.last_error = Some(e.0.clone()),
        }
        state.history.insert(0, run);
        state.history.truncate(HISTORY);
        self.save_state(id, &state)?;
        result
    }

    fn copy_to(
        &self,
        place: &Place,
        shoot: Option<ShootId>,
        budget: Option<Duration>,
        progress: &dyn Fn(u64, u64),
    ) -> Result<Report> {
        if !place.available() {
            return Err(Error(format!(
                "{} isn't connected ({} not found)",
                place.name,
                place.folder.display()
            )));
        }
        let started = Instant::now();
        let dir = place.copy_dir();
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let me = self.library_id()?;
        let mut manifest = read_manifest(&dir).unwrap_or_default();
        if !manifest.library_id.is_empty() && manifest.library_id != me {
            return Err(Error(format!(
                "{} holds another Library's backup; add the place again to use a new folder",
                dir.display()
            )));
        }
        manifest.library_id = me;
        manifest.library = self.root.display().to_string();

        let items: Vec<Item> = self
            .backup_items(place, shoot)?
            .into_iter()
            .filter(|i| {
                manifest
                    .files
                    .get(&i.rel)
                    .is_none_or(|e| e.bytes != i.bytes)
            })
            .collect();
        let skipped = self.backup_items(place, shoot)?.len() as u64 - items.len() as u64;
        let db_bytes = std::fs::metadata(self.root.join(super::DB_FILE))
            .map(|m| m.len())
            .unwrap_or(0);
        let total = items.iter().map(|i| i.bytes).sum::<u64>() + db_bytes;
        let mut done_bytes = 0u64;
        progress(0, total);

        // The database first: it's small, and it's what holds the edits.
        self.copy_database(place, &dir)?;
        done_bytes += db_bytes;
        progress(done_bytes, total);

        let mut report = Report {
            place: place.id.clone(),
            copy_dir: dir.clone(),
            copied_files: 0,
            copied_bytes: 0,
            skipped,
            failed: Vec::new(),
            done: true,
            differ: Vec::new(),
            verified_files: 0,
        };
        let mut last_save = Instant::now();
        for item in items {
            if budget.is_some_and(|b| started.elapsed() >= b) {
                report.done = false;
                break;
            }
            match copy_one(&item.src, &dir, &item.rel) {
                Ok(entry) => {
                    report.copied_files += 1;
                    report.copied_bytes += entry.bytes;
                    manifest.files.insert(item.rel.clone(), entry);
                }
                Err(e) => report.failed.push(format!("{}: {}", item.rel, e.0)),
            }
            done_bytes += item.bytes;
            progress(done_bytes.min(total), total);
            if last_save.elapsed() > Duration::from_secs(30) {
                write_manifest(&dir, &mut manifest)?;
                last_save = Instant::now();
            }
        }
        write_manifest(&dir, &mut manifest)?;
        Ok(report)
    }

    /// `library.db` in the copy is always the newest; a dated copy goes in its `backups/`
    /// at most once a day, keeping `keep_snapshots`.
    fn copy_database(&self, place: &Place, dir: &Path) -> Result<()> {
        let tmp = dir.join(".library.db.tmp");
        let _ = std::fs::remove_file(&tmp);
        self.db
            .execute("VACUUM INTO ?1", [tmp.to_string_lossy()])
            .map_err(db_err)?;
        std::fs::File::open(&tmp)
            .and_then(|f| f.sync_all())
            .map_err(|e| io(&tmp, e))?;
        let db = dir.join(super::DB_FILE);
        std::fs::rename(&tmp, &db).map_err(|e| io(&db, e))?;
        if place.keep_snapshots == 0 {
            return Ok(());
        }
        let snaps = dir.join(backup::DIR);
        let newest = std::fs::read_dir(&snaps)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.metadata().ok()?.modified().ok())
            .max();
        let due = newest
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_none_or(|age| age >= SNAPSHOT_EVERY);
        if due {
            let conn = rusqlite::Connection::open(&db).map_err(db_err)?;
            backup::snapshot(&conn, dir, "")?;
            drop(conn);
            let mut dated: Vec<PathBuf> = std::fs::read_dir(&snaps)
                .map_err(|e| io(&snaps, e))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "db"))
                .collect();
            dated.sort();
            dated.reverse();
            for old in dated.into_iter().skip(place.keep_snapshots) {
                let _ = std::fs::remove_file(old);
            }
        }
        Ok(())
    }

    fn verify_copy(&self, place: &Place, progress: &dyn Fn(u64, u64)) -> Result<Report> {
        if !place.available() {
            return Err(Error(format!("{} isn't connected", place.name)));
        }
        let dir = place.copy_dir();
        let manifest = read_manifest(&dir)
            .ok_or_else(|| Error(format!("{} has no backup to check yet", place.name)))?;
        let total: u64 = manifest.files.values().map(|e| e.bytes).sum();
        let mut done = 0;
        let mut report = Report {
            place: place.id.clone(),
            copy_dir: dir.clone(),
            copied_files: 0,
            copied_bytes: 0,
            skipped: 0,
            failed: Vec::new(),
            done: true,
            differ: Vec::new(),
            verified_files: 0,
        };
        progress(0, total);
        for (rel, entry) in &manifest.files {
            match hash_file(&dir.join(rel)) {
                Ok((h, _)) if h == entry.hash => {}
                _ => report.differ.push(rel.clone()),
            }
            report.verified_files += 1;
            done += entry.bytes;
            progress(done, total);
        }
        Ok(report)
    }

    /// Places due to run on their own now (Daily, On connect, or the monthly check), as
    /// (place id, verify).
    pub fn backups_due(&self) -> Result<Vec<(String, bool)>> {
        let mut out = Vec::new();
        for p in self.backup_places()? {
            if !p.available() {
                continue;
            }
            let s = self.place_state(&p.id)?;
            let older =
                |t: &Option<String>, d: Duration| t.as_deref().and_then(age).is_none_or(|a| a >= d);
            let day = Duration::from_secs(24 * 3600);
            // A run that just failed waits an hour before the next try.
            if s.history.first().is_some_and(|r| !r.ok)
                && !older(
                    &s.history.first().map(|r| r.at.clone()),
                    Duration::from_secs(3600),
                )
            {
                continue;
            }
            let run = match p.when {
                When::Daily | When::OnConnect => older(&s.last_ok, day),
                When::AfterImport | When::Manual => false,
            };
            if run {
                out.push((p.id.clone(), false));
            } else if p.verify_monthly
                && s.last_ok.is_some()
                && older(&s.last_verified, Duration::from_secs(30 * 24 * 3600))
            {
                out.push((p.id.clone(), true));
            }
        }
        Ok(out)
    }
}

/// Seconds since a UTC time Argus wrote (`2026-11-02T16:52:41Z`).
fn age(t: &str) -> Option<Duration> {
    let s = time::seconds_between(t, &time::now())?;
    Some(Duration::from_secs_f64(s.max(0.0)))
}

fn default_name(folder: &Path) -> String {
    let kind = Kind::of(folder);
    match kind {
        Kind::GoogleDrive | Kind::Icloud | Kind::Dropbox | Kind::OneDrive => {
            kind.label().to_string()
        }
        _ => folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| kind.label().to_string()),
    }
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    let s = std::fs::read_to_string(dir.join(MANIFEST)).ok()?;
    serde_json::from_str(&s).ok()
}

fn write_manifest(dir: &Path, m: &mut Manifest) -> Result<()> {
    m.updated = time::now();
    let path = dir.join(MANIFEST);
    let tmp = dir.join(format!(".{MANIFEST}.tmp"));
    let data = serde_json::to_vec(m).map_err(|e| Error(format!("manifest: {e}")))?;
    let mut f = std::fs::File::create(&tmp).map_err(|e| io(&tmp, e))?;
    f.write_all(&data)
        .and_then(|_| f.sync_all())
        .map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| io(&path, e))
}

fn hash_file(path: &Path) -> std::result::Result<(String, u64), std::io::Error> {
    let mut f = std::fs::File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((h.finalize().to_hex().to_string(), n))
}

/// Copy one file into the copy at `rel`: hash it as it's read, write to a temporary name,
/// read that back, and rename it into place only if the hashes agree. A different file
/// already at `rel` is never overwritten.
fn copy_one(src: &Path, dir: &Path, rel: &str) -> Result<Entry> {
    if rel.split('/').any(|s| s == ".." || s.is_empty()) {
        return Err(Error(format!("bad path in the Library: {rel}")));
    }
    let dest = dir.join(rel);
    let parent = dest.parent().expect("rel has a file name");
    std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    if dest.exists() {
        // Already there (an earlier run whose manifest was lost): adopt it only if equal.
        let (theirs, n) = hash_file(&dest).map_err(|e| io(&dest, e))?;
        let (ours, _) = hash_file(src).map_err(|e| io(src, e))?;
        if theirs == ours {
            return Ok(Entry {
                bytes: n,
                hash: ours,
            });
        }
        return Err(Error(
            "a different file is already there; left it as it is".into(),
        ));
    }
    let tmp = parent.join(format!(
        ".{}.argus-tmp",
        dest.file_name().unwrap().to_string_lossy()
    ));
    let mut from = std::fs::File::open(src).map_err(|e| io(src, e))?;
    let mut to = std::fs::File::create(&tmp).map_err(|e| io(&tmp, e))?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let k = from.read(&mut buf).map_err(|e| io(src, e))?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        to.write_all(&buf[..k]).map_err(|e| io(&tmp, e))?;
        n += k as u64;
    }
    to.sync_all().map_err(|e| io(&tmp, e))?;
    drop(to);
    let hash = h.finalize().to_hex().to_string();
    let (back, _) = hash_file(&tmp).map_err(|e| io(&tmp, e))?;
    if back != hash {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error("the copy read back differently".into()));
    }
    std::fs::rename(&tmp, &dest).map_err(|e| io(&dest, e))?;
    Ok(Entry { bytes: n, hash })
}

/// Folders on this Mac that would make good Backup places: the Google Drive, Dropbox and
/// OneDrive app folders, iCloud Drive, and every disk under /Volumes but the startup one.
pub fn find_places(home: &Path, volumes: &Path, in_use: &[PathBuf]) -> Vec<Found> {
    let mut found = Vec::new();
    let used = |p: &Path| in_use.iter().any(|u| u == p || u.starts_with(p));
    let mut add = |name: String, folder: PathBuf, note: Option<String>| {
        if folder.is_dir() {
            found.push(Found {
                kind: Kind::of(&folder),
                free_bytes: super::storage::free_bytes(&folder),
                in_use: used(&folder),
                name,
                folder,
                note,
            });
        }
    };
    if let Ok(entries) = std::fs::read_dir(home.join("Library/CloudStorage")) {
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let n = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if n.starts_with("GoogleDrive-") {
                let account = n.trim_start_matches("GoogleDrive-").to_string();
                let my = p.join("My Drive");
                add(
                    format!("Google Drive ({account})"),
                    if my.is_dir() { my } else { p },
                    Some("Set the Google Drive app to Mirror files to keep a copy on this Mac too, or Stream files to save space here.".into()),
                );
            } else if n.starts_with("Dropbox") {
                add("Dropbox".into(), p, None);
            } else if n.starts_with("OneDrive") {
                add("OneDrive".into(), p, None);
            }
        }
    }
    add(
        "iCloud Drive".into(),
        home.join("Library/Mobile Documents/com~apple~CloudDocs"),
        Some("Turn off Optimize Mac Storage, or iCloud may remove the local copies.".into()),
    );
    let root_dev = dev_of(Path::new("/"));
    if let Ok(entries) = std::fs::read_dir(volumes) {
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if root_dev.is_some() && dev_of(&p) == root_dev {
                continue; // the startup disk's alias
            }
            let n = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if n.starts_with('.') {
                continue;
            }
            add(n, p, None);
        }
    }
    found
}

#[cfg(unix)]
fn dev_of(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
fn dev_of(_: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_merge_and_find() {
        let r = ranges(&mut [5, 1, 2, 3, 9, 10, 2]);
        assert_eq!(r, vec![(1, 3), (5, 5), (9, 10)]);
        assert!(in_ranges(&r, 2) && in_ranges(&r, 10) && !in_ranges(&r, 4));
    }

    #[test]
    fn kinds_by_path() {
        assert_eq!(
            Kind::of(Path::new(
                "/Users/g/Library/CloudStorage/GoogleDrive-g@x.com/My Drive"
            )),
            Kind::GoogleDrive
        );
        assert_eq!(
            Kind::of(Path::new(
                "/Users/g/Library/Mobile Documents/com~apple~CloudDocs"
            )),
            Kind::Icloud
        );
        assert_eq!(Kind::of(Path::new("/tmp/x")), Kind::Folder);
    }
}
