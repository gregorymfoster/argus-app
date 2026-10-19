//! Before an Import: find memory cards, and list what's on one without importing it.
//!
//! A scan reads only each file's size and the start of its EXIF, so a card of a few
//! hundred RAWs scans in seconds. A Photo counts as already imported when the Library
//! holds one with the same file name, size and capture time: card paths repeat between
//! cards, and hashing every file would mean reading the whole card. Import still dedupes
//! by content hash, so a miss here only means a duplicate is offered, never lost.

use super::import::{kind_of, pair_raw_jpeg, walk, Exif};
use super::{db_err, time, Library};
use crate::{Error, Result};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Enough of a file for its EXIF block (TIFF-based RAWs and JPEGs keep it near the start).
const EXIF_PREFIX: u64 = 256 * 1024;

/// A mounted memory card: a volume with a camera's DCIM folder.
#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScannedPhoto {
    pub path: PathBuf,
    pub file_name: String,
    pub kind: &'static str,
    pub bytes: u64,
    pub captured_at: String,
    pub camera: String,
    /// The Library already has this Photo (see the module docs for how that's judged).
    pub imported: bool,
    /// The camera's JPEG of the same frame, when it shot RAW+JPEG. Import keeps it with the
    /// RAW as one Photo (or skips it, `raw_only`).
    pub jpeg: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub folder: PathBuf,
    /// In capture order.
    pub photos: Vec<ScannedPhoto>,
    pub new: usize,
    pub imported: usize,
}

/// Folders whose children are mounted volumes: `ARGUS_VOLUMES` (`:`-separated) if set,
/// else `/Volumes` on macOS and the usual removable-media folders on Linux.
fn volume_roots() -> Vec<PathBuf> {
    if let Ok(v) = std::env::var("ARGUS_VOLUMES") {
        return std::env::split_paths(&v).collect();
    }
    if cfg!(target_os = "macos") {
        return vec![PathBuf::from("/Volumes")];
    }
    let user = std::env::var("USER").unwrap_or_default();
    vec![
        PathBuf::from(format!("/media/{user}")),
        PathBuf::from(format!("/run/media/{user}")),
        PathBuf::from("/media"),
    ]
}

/// Mounted memory cards, by name.
pub fn find_cards() -> Vec<Card> {
    let mut cards: Vec<Card> = volume_roots()
        .iter()
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flat_map(|d| d.flatten())
        .map(|e| e.path())
        .filter(|p| p.join("DCIM").is_dir())
        .map(|p| Card {
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: p,
        })
        .collect();
    cards.sort_by(|a, b| a.name.cmp(&b.name));
    cards.dedup_by(|a, b| a.path == b.path);
    cards
}

/// True when `folder` is on a mounted Card (or is one). Cards are removable, so Import
/// copies from them by default.
pub fn is_on_card(folder: &Path) -> bool {
    let folder = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    find_cards().iter().any(|c| {
        let card = c.path.canonicalize().unwrap_or_else(|_| c.path.clone());
        folder.starts_with(card)
    })
}

/// A file's EXIF, read from just the start of it.
pub(super) fn read_exif_prefix(path: &Path) -> Exif {
    read_prefix(path, EXIF_PREFIX)
        .map(|b| Exif::read(&b))
        .unwrap_or_default()
}

fn read_prefix(path: &Path, n: u64) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    std::fs::File::open(path)?.take(n).read_to_end(&mut buf)?;
    Ok(buf)
}

pub(super) fn run(lib: &Library, folder: &Path) -> Result<Scan> {
    let folder = folder
        .canonicalize()
        .map_err(|e| Error(format!("{}: {e}", folder.display())))?;
    let mut files = Vec::new();
    walk(&folder, &mut files)?;
    let files = pair_raw_jpeg(files);
    let task = crate::progress::Task::start("scan", "Finding photos", files.len());

    let known: HashSet<(String, i64, String)> = {
        let mut q = lib
            .db
            .prepare("SELECT file_name, bytes, captured_at FROM photos")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(db_err)?
    };

    let mut photos: Vec<ScannedPhoto> = files
        .par_iter()
        .filter_map(|(path, jpeg)| {
            task.tick();
            let meta = std::fs::metadata(path).ok()?;
            let exif = read_prefix(path, EXIF_PREFIX)
                .map(|b| Exif::read(&b))
                .unwrap_or_default();
            // Same fallback as Import, so the two agree on capture time.
            let captured_at = exif
                .captured_at
                .unwrap_or_else(|| meta.modified().map(time::from_system).unwrap_or_default());
            let file_name = path.file_name()?.to_string_lossy().into_owned();
            let imported =
                known.contains(&(file_name.clone(), meta.len() as i64, captured_at.clone()));
            Some(ScannedPhoto {
                path: path.clone(),
                file_name,
                kind: kind_of(path)?,
                bytes: meta.len(),
                captured_at,
                camera: exif.camera,
                imported,
                jpeg: jpeg.clone(),
            })
        })
        .collect();
    photos.sort_by(|a, b| (&a.captured_at, &a.file_name).cmp(&(&b.captured_at, &b.file_name)));
    let imported = photos.iter().filter(|p| p.imported).count();
    Ok(Scan {
        folder,
        new: photos.len() - imported,
        imported,
        photos,
    })
}
