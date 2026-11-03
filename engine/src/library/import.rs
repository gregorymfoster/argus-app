//! Import: walk a folder, dedupe by content hash, read metadata, build previews in
//! parallel, and record everything as one new Shoot. Originals are only ever read.

use super::{db_err, store_path, time, Exposure, Library, ShootId, PREVIEWS_DIR};
use crate::naming;
use crate::{decode, develop, DevelopOptions, Error, Orientation, Result};
use rusqlite::params;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Instant;

const RAW_EXTENSIONS: &[&str] = &["arw", "cr2", "cr3", "nef", "raf", "dng", "orf", "rw2"];
const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg"];
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Defaults to the folder's name.
    pub shoot_name: Option<String>,
    /// Import only these files (each inside the folder) instead of everything in it, e.g.
    /// the Photos picked from a card's [`super::Scan`].
    pub files: Option<Vec<PathBuf>>,
    /// For RAW+JPEG frames, import the RAW and skip the camera's JPEG. By default the
    /// JPEG is kept with its RAW as one Photo (Greg, 2026-10-18).
    pub raw_only: bool,
    /// Copy each original into the Library's `originals/<date>_<shoot>/` folder, verified
    /// by hash, instead of reading it where it is. A Card is removable, so Card imports
    /// copy (ADR 0011); the tool layer picks that default with [`super::is_on_card`].
    pub copy: bool,
    /// Longest edge of the Library's own preview JPEGs; None uses the Library's Settings.
    pub preview_long_edge: Option<usize>,
    /// For a RAW, write the camera's embedded JPEG as the preview (fast: the grid fills at
    /// card-read speed) and leave Argus's own render to [`Library::render_previews`]. Off
    /// renders every preview during the Import.
    pub camera_previews: bool,
    /// The resumable run recording this Import ([`super::Run`]): it learns the Shoot and the
    /// copy folder as soon as they exist, so a resumed Import finishes into them.
    pub run: Option<super::RunId>,
    /// Carry on an interrupted Import: into this Shoot (and copy folder), whose Photos are
    /// skipped as already there.
    pub resume: Option<ImportResume>,
    /// Photos decoded at once. Each full-resolution RAW holds roughly 0.5 GB while it
    /// develops, so this caps peak memory; develop itself still uses every core.
    pub max_in_flight: usize,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            shoot_name: None,
            files: None,
            raw_only: false,
            copy: false,
            preview_long_edge: None,
            camera_previews: true,
            run: None,
            resume: None,
            max_in_flight: 4,
        }
    }
}

/// Where an interrupted Import carries on ([`ImportOptions::resume`]).
#[derive(Debug, Clone)]
pub struct ImportResume {
    pub shoot_id: ShootId,
    pub copy_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportFailure {
    pub file: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    /// None when nothing new was found (no empty Shoot is created).
    pub shoot_id: Option<ShootId>,
    pub shoot_name: String,
    pub found: usize,
    pub imported: usize,
    pub duplicates: usize,
    /// Where the originals were copied to (relative to the Library), when they were.
    pub copied_to: Option<String>,
    pub failed: Vec<ImportFailure>,
    /// The folder went away part way (a card ejected, or a reader that unmounts while the
    /// Mac sleeps): the Import stopped there and resumes when it is back.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub card_gone: bool,
    /// Photos an interrupted Import had already brought in, when this one resumed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_with: Option<usize>,
    /// Photos showing the camera's preview until [`Library::render_previews`] runs.
    pub camera_previews: usize,
    pub scan_ms: f64,
    pub process_ms: f64,
    pub total_ms: f64,
}

struct Item {
    /// The original as the Library will read it: the copy when Import copied it.
    path: PathBuf,
    file_name: String,
    hash: String,
    bytes: u64,
    kind: &'static str,
    captured_at: String,
    width: u32,
    height: u32,
    camera: String,
    exposure: Exposure,
    preview: String,
    jpeg: Option<PathBuf>,
}

enum Outcome {
    Imported(Box<Item>),
    Duplicate,
    Failed(ImportFailure),
}

/// How often a running Import commits, so the Photos it has finished show up.
const FLUSH_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

fn ms(t: Instant) -> f64 {
    (t.elapsed().as_secs_f64() * 10_000.0).round() / 10.0
}

pub(super) fn run(lib: &mut Library, folder: &Path, opts: &ImportOptions) -> Result<ImportReport> {
    let t0 = Instant::now();
    let folder = folder
        .canonicalize()
        .map_err(|e| Error(format!("{}: {e}", folder.display())))?;
    let mut files = Vec::new();
    match &opts.files {
        Some(only) => {
            for f in only {
                let f = f
                    .canonicalize()
                    .map_err(|e| Error(format!("{}: {e}", f.display())))?;
                if !f.starts_with(&folder) || kind_of(&f).is_none() {
                    return Err(Error(format!(
                        "{} is not a photo in {}",
                        f.display(),
                        folder.display()
                    )));
                }
                files.push(f);
            }
            files.sort();
            files.dedup();
        }
        None => walk(&folder, &mut files)?,
    }
    let files = frames(files, opts.raw_only);
    let task = crate::progress::Task::start("import", "Importing", files.len());
    let scan_ms = ms(t0);

    let shoot_name = opts.shoot_name.clone().unwrap_or_else(|| {
        folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Shoot".into())
    });
    let known: HashSet<String> = {
        let mut q = lib.db.prepare("SELECT hash FROM photos").map_err(db_err)?;
        let rows = q.query_map([], |r| r.get(0)).map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(db_err)?
    };
    let settings = lib.settings()?;
    let copy_dir = match (opts.copy, files.first(), &opts.resume) {
        (
            true,
            Some(_),
            Some(ImportResume {
                copy_dir: Some(d), ..
            }),
        ) => {
            std::fs::create_dir_all(d).map_err(|e| Error(format!("{}: {e}", d.display())))?;
            clear_tmp(d);
            Some(d.clone())
        }
        (true, Some(_), _) => Some(originals_dir(&lib.root, &files, &shoot_name)?),
        _ => None,
    };
    let ctx = Ctx {
        root: lib.root.clone(),
        long_edge: opts
            .preview_long_edge
            .unwrap_or(settings.preview_long_edge as usize),
        quality: settings.preview_quality,
        camera_previews: opts.camera_previews,
        copy_dir: copy_dir.clone(),
        seen: Mutex::new(known),
    };

    let t1 = Instant::now();
    let now = time::now();
    // The Shoot and its Photos are committed as they land, so the app can open the Shoot
    // and fill its grid while the Import runs. A killed Import keeps what it finished;
    // importing again skips those as duplicates.
    let db = &lib.db;
    let (shoot_id, already, earliest) = match &opts.resume {
        Some(r) => {
            let (n, first): (i64, Option<String>) = db
                .query_row(
                    "SELECT COUNT(*), MIN(captured_at) FROM photos WHERE shoot_id = ?1",
                    [r.shoot_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(db_err)?;
            (r.shoot_id, n as usize, first)
        }
        None => {
            db.execute(
                "INSERT INTO shoots (name, source, imported_at) VALUES (?1, ?2, ?3)",
                params![
                    crate::naming::shoot_slug(&shoot_name, None),
                    folder.to_string_lossy(),
                    now
                ],
            )
            .map_err(db_err)?;
            (db.last_insert_rowid(), 0, None)
        }
    };
    if let Some(run) = opts.run {
        let mut args = lib.run(run)?.args;
        args["shoot_id"] = serde_json::json!(shoot_id);
        if let Some(d) = &copy_dir {
            args["copy_dir"] = serde_json::json!(store_path(&lib.root, d));
        }
        lib.set_run_args(run, &args)?;
    }
    let mut tx = Some(db.unchecked_transaction().map_err(db_err)?);
    let mut flushed = Instant::now();

    let (send, recv) = mpsc::channel();
    let next = AtomicUsize::new(0);
    let (mut imported, mut duplicates, mut failed) = (0, 0usize, Vec::new());
    let mut camera_previews = 0;
    let mut first_capture: Option<String> = earliest;
    let mut card_gone = false;
    // Cancel (`cancel_import`) stops handing out files; what landed stays imported.
    let stop = task.stop_flag();
    std::thread::scope(|s| -> Result<()> {
        // A few worker threads pull Photos from a shared cursor. Bounding the number in
        // flight keeps memory flat; each develop fans out over rayon's pool on its own.
        for _ in 0..opts.max_in_flight.clamp(1, files.len().max(1)) {
            let send = send.clone();
            let (next, files, ctx, stop) = (&next, &files, &ctx, &stop);
            s.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let Some((f, jpeg)) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        break;
                    };
                    let _ = send.send(process(f, jpeg.as_deref(), ctx));
                }
            });
        }
        drop(send);
        // Insert on this thread as results arrive, so a long import makes steady progress.
        // Commit the first Photo at once (the app opens the Shoot on it), then twice a
        // second, even while a slow file holds up the rest.
        let mut unsaved = 0;
        loop {
            let outcome = match recv.recv_timeout(FLUSH_EVERY) {
                Ok(o) => Some(o),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if unsaved > 0 && (imported == 1 || flushed.elapsed() >= FLUSH_EVERY) {
                tx.take().unwrap().commit().map_err(db_err)?;
                tx = Some(db.unchecked_transaction().map_err(db_err)?);
                flushed = Instant::now();
                unsaved = 0;
            }
            let Some(outcome) = outcome else { continue };
            task.tick();
            match outcome {
                Outcome::Imported(it) => {
                    tx.as_ref().unwrap().execute(
                        "INSERT INTO photos (shoot_id, path, file_name, hash, bytes, kind, captured_at,
                                             width, height, camera, preview, imported_at, jpeg,
                                             shutter, aperture, iso, focal, lens)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                                 ?14, ?15, ?16, ?17, ?18)",
                        params![
                            shoot_id,
                            store_path(&ctx.root, &it.path),
                            it.file_name,
                            it.hash,
                            it.bytes as i64,
                            it.kind,
                            it.captured_at,
                            it.width,
                            it.height,
                            it.camera,
                            it.preview,
                            now,
                            it.jpeg.as_ref().map(|j| store_path(&ctx.root, j)),
                            it.exposure.shutter,
                            it.exposure.aperture,
                            it.exposure.iso,
                            it.exposure.focal,
                            it.exposure.lens,
                        ],
                    )
                    .map_err(db_err)?;
                    imported += 1;
                    unsaved += 1;
                    camera_previews += it.preview.ends_with(CAMERA_SUFFIX) as usize;
                    if first_capture.as_ref().is_none_or(|f| it.captured_at < *f) {
                        first_capture = Some(it.captured_at.clone());
                    }
                }
                Outcome::Duplicate => duplicates += 1,
                // A read that failed because the folder is gone (the card ejected, or a
                // reader unmounted by sleep) stops here: the rest waits for the card,
                // rather than every remaining file failing.
                Outcome::Failed(_) if !folder.is_dir() => {
                    card_gone = true;
                    stop.store(true, Ordering::Relaxed);
                }
                Outcome::Failed(f) => failed.push(f),
            }
        }
        Ok(())
    })?;
    // Greg's naming rule: the Shoot's date first, no spaces (`2026-10-18_Boat-Trip`).
    let shoot_name = crate::naming::shoot_slug(&shoot_name, first_capture.as_deref());
    let tx = tx.unwrap();
    if imported + already == 0 {
        tx.execute("DELETE FROM shoots WHERE id = ?1", [shoot_id])
            .map_err(db_err)?;
    } else {
        tx.execute(
            "UPDATE shoots SET name = ?1 WHERE id = ?2",
            params![shoot_name, shoot_id],
        )
        .map_err(db_err)?;
    }
    // A copy folder left empty (every file a duplicate or failed) is clutter.
    if let Some(dir) = &copy_dir {
        let _ = std::fs::remove_dir(dir);
    }
    tx.commit().map_err(db_err)?;
    failed.sort_by(|a: &ImportFailure, b| a.file.cmp(&b.file));
    let mut copied_to = copy_dir
        .filter(|d| d.is_dir())
        .map(|d| store_path(&ctx.root, &d));
    // The copy folder was named before Import knew which files were new, from every file's
    // date. Run again after a cancelled Import, the earlier frames are duplicates and the
    // Shoot is dated by the rest, so its folder follows the Shoot's name (a no-op otherwise).
    if imported + already > 0 && copied_to.is_some() {
        copied_to = lib.rename_shoot(shoot_id, &shoot_name)?.originals;
    }

    Ok(ImportReport {
        shoot_id: (imported + already > 0).then_some(shoot_id),
        shoot_name,
        found: files.len(),
        imported,
        camera_previews,
        duplicates: duplicates.saturating_sub(already),
        copied_to,
        failed,
        card_gone,
        resumed_with: opts.resume.is_some().then_some(already),
        scan_ms,
        process_ms: ms(t1),
        total_ms: ms(t0),
    })
}

pub(super) fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
    for e in entries.flatten() {
        let p = e.path();
        let hidden = p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if p.is_dir() {
            walk(&p, out)?;
        } else if kind_of(&p).is_some() {
            out.push(p);
        }
    }
    Ok(())
}

/// A camera shooting RAW+JPEG writes each frame twice (`DSC07745.ARW`, `DSC07745.JPG`).
/// Like Lightroom, they are one Photo, the RAW: a JPEG with a RAW of the same name in the
/// same folder leaves the list and comes back beside its RAW. A lone JPEG stays a Photo.
pub(super) fn pair_raw_jpeg(files: Vec<PathBuf>) -> Vec<(PathBuf, Option<PathBuf>)> {
    let key = |p: &Path| p.with_extension("").to_string_lossy().to_lowercase();
    let mut jpegs: std::collections::HashMap<String, PathBuf> = files
        .iter()
        .filter(|p| kind_of(p) == Some("jpeg"))
        .map(|p| (key(p), p.clone()))
        .collect();
    let raws: Vec<(PathBuf, Option<PathBuf>)> = files
        .iter()
        .filter(|p| kind_of(p) == Some("raw"))
        .map(|p| (p.clone(), jpegs.remove(&key(p))))
        .collect();
    let mut out = raws;
    out.extend(jpegs.into_values().map(|j| (j, None)));
    out.sort();
    out
}

/// One Photo per frame: a RAW and the camera's JPEG of it (found in `files`, or beside the
/// RAW on disk) become one Photo; with `raw_only` the JPEG is left out.
pub(super) fn frames(files: Vec<PathBuf>, raw_only: bool) -> Vec<(PathBuf, Option<PathBuf>)> {
    let mut frames = pair_raw_jpeg(files);
    for (main, jpeg) in &mut frames {
        if raw_only {
            *jpeg = None;
        } else if jpeg.is_none() && kind_of(main) == Some("raw") {
            *jpeg = sibling_jpeg(main);
        }
    }
    frames
}

/// The camera's JPEG next to a RAW (`DSC07745.ARW` -> `DSC07745.JPG`), if there is one.
pub(super) fn sibling_jpeg(raw: &Path) -> Option<PathBuf> {
    JPEG_EXTENSIONS
        .iter()
        .flat_map(|e| [e.to_string(), e.to_ascii_uppercase()])
        .map(|e| raw.with_extension(e))
        .find(|p| p.is_file())
}

pub(super) fn kind_of(p: &Path) -> Option<&'static str> {
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    if RAW_EXTENSIONS.contains(&ext.as_str()) {
        Some("raw")
    } else if JPEG_EXTENSIONS.contains(&ext.as_str()) {
        Some("jpeg")
    } else {
        None
    }
}

/// What every Import worker shares.
struct Ctx {
    root: PathBuf,
    long_edge: usize,
    quality: u8,
    camera_previews: bool,
    copy_dir: Option<PathBuf>,
    seen: Mutex<HashSet<String>>,
}

/// `originals/<Shoot name>` (`originals/2026-10-18_Boat-Trip`, docs/naming.md), the Shoot
/// named as Import will name it: dated by the earliest capture among `files`. A Shoot of
/// the same name already there gets `-2`, `-3`...
fn originals_dir(
    root: &Path,
    files: &[(PathBuf, Option<PathBuf>)],
    shoot_name: &str,
) -> Result<PathBuf> {
    let first_capture = files
        .iter()
        .filter_map(|(f, _)| {
            super::scan::read_exif_prefix(f).captured_at.or_else(|| {
                std::fs::metadata(f)
                    .and_then(|m| m.modified())
                    .map(time::from_system)
                    .ok()
            })
        })
        .min();
    let folder =
        naming::originals_folder(&naming::shoot_slug(shoot_name, first_capture.as_deref()));
    let base = root.join(folder.parent().expect("originals/<name>"));
    let name = naming::unique_name(
        &folder.file_name().unwrap_or_default().to_string_lossy(),
        |n| base.join(n).exists(),
    );
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).map_err(|e| Error(format!("create {}: {e}", dir.display())))?;
    Ok(dir)
}

fn process(path: &Path, jpeg: Option<&Path>, ctx: &Ctx) -> Outcome {
    match try_process(path, jpeg, ctx) {
        Ok(Some(it)) => Outcome::Imported(Box::new(it)),
        Ok(None) => Outcome::Duplicate,
        Err(e) => Outcome::Failed(ImportFailure {
            file: path.to_string_lossy().into_owned(),
            error: e.0,
        }),
    }
}

fn try_process(path: &Path, jpeg: Option<&Path>, ctx: &Ctx) -> Result<Option<Item>> {
    let kind = kind_of(path).expect("walk only yields known kinds");
    let data = std::fs::read(path).map_err(|e| Error(format!("read: {e}")))?;
    let hash = blake3::hash(&data).to_hex().to_string();
    if !ctx.seen.lock().unwrap().insert(hash.clone()) {
        return Ok(None);
    }
    // A corrupt header would abort rawler later (previews, Edit): fail this file here instead.
    if kind == "raw" {
        crate::rawcheck::check(path)?;
    }
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let original = match &ctx.copy_dir {
        Some(dir) => copy_verified(&data, &hash, dir, &file_name)?,
        None => path.to_path_buf(),
    };
    // The camera's JPEG of a RAW+JPEG frame travels with its RAW.
    let jpeg = match (jpeg, &ctx.copy_dir) {
        (Some(j), Some(dir)) => {
            let d = std::fs::read(j).map_err(|e| Error(format!("read {}: {e}", j.display())))?;
            let h = blake3::hash(&d).to_hex().to_string();
            let name = j.file_name().unwrap_or_default().to_string_lossy();
            Some(copy_verified(&d, &h, dir, &name)?)
        }
        (j, _) => j.map(Path::to_path_buf),
    };
    let exif = Exif::read(&data);
    let camera = kind == "raw" && ctx.camera_previews;
    let camera_rel = camera_preview_rel(&hash);
    let camera = match camera {
        true => write_camera_preview(
            &original,
            &data,
            ctx.long_edge,
            ctx.quality,
            &ctx.root.join(&camera_rel),
        )?,
        false => None,
    };
    let (rel, (width, height)) = match camera {
        Some(size) => (camera_rel, size),
        None => {
            let rel = preview_rel(&hash);
            let size = write_preview(
                &original,
                &data,
                kind,
                ctx.long_edge,
                ctx.quality,
                &ctx.root.join(&rel),
            )?;
            (rel, size)
        }
    };

    let captured_at = exif.captured_at.unwrap_or_else(|| {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map(time::from_system)
            .unwrap_or_default()
    });
    Ok(Some(Item {
        path: original,
        file_name,
        hash,
        bytes: data.len() as u64,
        kind,
        captured_at,
        width,
        height,
        camera: exif.camera,
        exposure: exif.exposure,
        preview: rel,
        jpeg,
    }))
}

/// A Photo's preview path, relative to the Library, by content hash.
pub(super) fn preview_rel(hash: &str) -> String {
    format!("{PREVIEWS_DIR}/{}/{hash}.jpg", &hash[..2])
}

/// Where a RAW's camera preview waits for Argus's own render. The name marks the Photos
/// [`Library::render_previews`] still has to do, and the new path tells the UI to reload.
pub(super) fn camera_preview_rel(hash: &str) -> String {
    format!("{PREVIEWS_DIR}/{}/{hash}{CAMERA_SUFFIX}", &hash[..2])
}
pub(super) const CAMERA_SUFFIX: &str = ".camera.jpg";

/// Embedded previews smaller than this are thumbnails, too small to stand in for a preview.
const CAMERA_PREVIEW_MIN_EDGE: usize = 1024;

/// Write a RAW's embedded camera JPEG, upright and no bigger than `long_edge`, to `out`.
/// Returns the RAW's full upright size, or None when it has no usable embedded JPEG.
fn write_camera_preview(
    path: &Path,
    data: &[u8],
    long_edge: usize,
    quality: u8,
    out: &Path,
) -> Result<Option<(u32, u32)>> {
    let Some(jpeg) = crate::embedded_preview_at_least(data, CAMERA_PREVIEW_MIN_EDGE) else {
        return Ok(None);
    };
    let Ok(img) = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg) else {
        return Ok(None);
    };
    let (width, height, orientation) = crate::raw_size(path, data)?;
    let img = match orientation {
        Orientation::Rotate90 => img.rotate90(),
        Orientation::Rotate180 => img.rotate180(),
        Orientation::Rotate270 => img.rotate270(),
        Orientation::Normal => img,
    };
    let edge = long_edge as u32;
    let img = if img.width().max(img.height()) > edge {
        img.resize(edge, edge, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    write_jpeg(&rgb.into_raw(), w, h, quality, out)?;
    Ok(Some((width as u32, height as u32)))
}

/// Write `data` into `dir` under its own name (`-2`, `-3`... on a clash), read it back and
/// check its hash, so a bad copy fails the Photo instead of standing in for its original.
/// It is written and checked under a hidden temporary name, then renamed, so a kill mid-copy
/// (a quit, a crash) never leaves half an original under its real name.
fn copy_verified(data: &[u8], hash: &str, dir: &Path, file_name: &str) -> Result<PathBuf> {
    let safe = naming::safe_file_name(file_name);
    let tmp = dir.join(format!(
        ".{safe}.{}{TMP_SUFFIX}",
        &hash[..hash.len().min(12)]
    ));
    let fail = |e: String| {
        let _ = std::fs::remove_file(&tmp);
        Error(e)
    };
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|e| fail(format!("copy: {e}")))?;
        f.write_all(data)
            .and_then(|_| f.sync_all())
            .map_err(|e| fail(format!("copy to {}: {e}", tmp.display())))?;
    }
    let back = std::fs::read(&tmp).map_err(|e| fail(format!("verify copy: {e}")))?;
    if blake3::hash(&back).to_hex().as_str() != hash {
        return Err(fail(format!("copy of {file_name} did not verify")));
    }
    // One name picked and taken at a time, so parallel workers never take the same one.
    static NAMING: Mutex<()> = Mutex::new(());
    // lock-lint: ok, held only to pick a free name and rename into it.
    let _one = NAMING.lock().unwrap_or_else(|e| e.into_inner());
    let dest = dir.join(naming::unique_name(&safe, |n| dir.join(n).exists()));
    std::fs::rename(&tmp, &dest).map_err(|e| fail(format!("copy to {}: {e}", dest.display())))?;
    Ok(dest)
}

/// What a copy interrupted part way leaves in a copy folder.
const TMP_SUFFIX: &str = ".argus-tmp";

/// Remove the temporary files an interrupted copy left in `dir`.
fn clear_tmp(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && name.ends_with(TMP_SUFFIX) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Develop (RAW) or scale (JPEG) a Photo to a preview JPEG at `out`; returns the
/// original's full upright size.
pub(super) fn write_preview(
    path: &Path,
    data: &[u8],
    kind: &str,
    long_edge: usize,
    quality: u8,
    out: &Path,
) -> Result<(u32, u32)> {
    let (rgb, pw, ph, width, height) = if kind == "raw" {
        let raw = decode(path)?;
        let (cw, ch) = raw.crop_size();
        let (width, height) = match raw.orientation {
            Orientation::Rotate90 | Orientation::Rotate270 => (ch, cw),
            _ => (cw, ch),
        };
        let img = develop(
            &raw,
            &DevelopOptions {
                long_edge: Some(long_edge),
                ..Default::default()
            },
        );
        (img.data, img.width, img.height, width as u32, height as u32)
    } else {
        let img = upright_jpeg(data, Exif::read(data).orientation)?;
        let (width, height) = (img.width(), img.height());
        let edge = long_edge as u32;
        let img = if width.max(height) > edge {
            img.resize(edge, edge, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        let rgb = img.to_rgb8();
        let (pw, ph) = (rgb.width() as usize, rgb.height() as usize);
        (rgb.into_raw(), pw, ph, width, height)
    };
    write_jpeg(&rgb, pw, ph, quality, out)?;
    Ok((width, height))
}

fn write_jpeg(rgb: &[u8], w: usize, h: usize, quality: u8, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| Error(format!("previews: {e}")))?;
    // Write beside and rename, so a preview being rebuilt is never half there.
    let tmp = out.with_extension("jpg.part");
    let mut enc = jpeg_encoder::Encoder::new_file(&tmp, quality)
        .map_err(|e| Error(format!("preview: {e}")))?;
    enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2);
    enc.encode(rgb, w as u16, h as u16, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| Error(format!("preview encode: {e}")))?;
    std::fs::rename(&tmp, out).map_err(|e| Error(format!("preview: {e}")))?;
    Ok(())
}

/// Decode a JPEG and rotate it upright by its EXIF orientation.
fn upright_jpeg(data: &[u8], orientation: u32) -> Result<image::DynamicImage> {
    let img = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
        .map_err(|e| Error(format!("jpeg decode: {e}")))?;
    Ok(match orientation {
        6 => img.rotate90(),
        3 => img.rotate180(),
        8 => img.rotate270(),
        _ => img,
    })
}

/// Read a JPEG original upright, as import sees it.
pub(crate) fn read_jpeg_upright(path: &Path) -> Result<image::DynamicImage> {
    let data = std::fs::read(path).map_err(|e| Error(format!("read {}: {e}", path.display())))?;
    upright_jpeg(&data, Exif::read(&data).orientation)
}

#[derive(Default)]
pub(super) struct Exif {
    pub captured_at: Option<String>,
    pub camera: String,
    pub exposure: Exposure,
    orientation: u32,
}

impl Exif {
    pub(super) fn read(data: &[u8]) -> Self {
        let Ok(ex) = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(data))
        else {
            return Self::default();
        };
        let text = |tag| {
            ex.get_field(tag, exif::In::PRIMARY)
                .and_then(|f| match &f.value {
                    exif::Value::Ascii(v) => v.first().map(|b| {
                        String::from_utf8_lossy(b)
                            .trim()
                            .trim_end_matches('\0')
                            .to_string()
                    }),
                    _ => None,
                })
        };
        let make = text(exif::Tag::Make).unwrap_or_default();
        let model = text(exif::Tag::Model).unwrap_or_default();
        let camera = if model.starts_with(&make) || make.is_empty() {
            model
        } else {
            format!("{make} {model}")
        };
        let ratio = |tag| {
            ex.get_field(tag, exif::In::PRIMARY)
                .and_then(|f| match &f.value {
                    exif::Value::Rational(v) => v.first().map(|r| r.to_f64()),
                    _ => None,
                })
                .filter(|v| v.is_finite() && *v > 0.0)
        };
        let exposure = Exposure {
            shutter: ratio(exif::Tag::ExposureTime),
            aperture: ratio(exif::Tag::FNumber),
            iso: ex
                .get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
                .filter(|&v| v > 0),
            focal: ratio(exif::Tag::FocalLength),
            // Sony writes "----" for a lens with no electronic contacts.
            lens: text(exif::Tag::LensModel).filter(|l| !l.is_empty() && !l.starts_with("--")),
        };
        Self {
            exposure,
            captured_at: text(exif::Tag::DateTimeOriginal)
                .or_else(|| text(exif::Tag::DateTime))
                .and_then(|s| time::from_exif(&s)),
            camera,
            orientation: ex
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
                .unwrap_or(1),
        }
    }
}
