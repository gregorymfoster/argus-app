//! Export tool.

use super::*;
use crate::library::ItemState;

pub(super) fn specs() -> Vec<ToolSpec> {
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "export_photos",
            description: "Export Photos into a folder on this Mac (created if missing), as Lightroom's Export does: JPEG, HEIC (macOS only) or TIFF rendered with each Photo's edits and Look, or the untouched original plus an XMP sidecar Lightroom reads. Names never contain spaces and nothing is overwritten (an existing name gets -2). JPEGs and HEICs keep capture metadata; location only with keep_location. Photos that fail are listed under failed; the rest still export.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "folder"], "properties": {
                "photo_ids": ids,
                "folder": { "type": "string", "description": "Export location: an absolute path, or starting with ~/" },
                "format": { "enum": ["jpeg", "heic", "tiff", "original"], "description": "Default jpeg; heic needs macOS" },
                "long_edge": { "type": "integer", "minimum": 64, "description": "Pixels (7680 for 8K); full size if omitted; never upscales" },
                "short_edge": { "type": "integer", "minimum": 64, "description": "Pixels on the short side instead (1080 for Instagram); never upscales" },
                "megapixels": { "type": "number", "exclusiveMinimum": 0, "description": "Size by megapixels instead; never upscales" },
                "sharpen": { "enum": ["low", "standard", "high"], "description": "Output sharpening for screen, as Lightroom's Export; off if omitted" },
                "quality": { "type": "integer", "minimum": 50, "maximum": 100, "description": "JPEG and HEIC quality, default 90" },
                "naming": { "enum": ["shoot", "sequence", "original"],
                    "description": "shoot (default): 2026-10-18_Boat-Trip_DSC08005; sequence: 2026-10-18_Boat-Trip_001; original: DSC08005" },
                "shoot_folder": { "type": "boolean", "description": "Put files in a folder named after their Shoot inside folder (default true)" },
                "sequence_start": { "type": "integer", "minimum": 1, "description": "Number of the first Photo, for sequence names (default 1)" },
                "sequence_total": { "type": "integer", "minimum": 1, "description": "Photos in the whole export, when it is split across calls (sets the digits)" },
                "color_space": { "enum": ["srgb", "display_p3"], "description": "srgb (default) or display_p3: wider colour for Apple screens, with the profile embedded; JPEG and HEIC only" },
                "watermark": { "type": "object", "required": ["text"], "description": "Text drawn on rendered files, as Lightroom's Watermark",
                    "properties": { "text": { "type": "string" },
                        "corner": { "enum": ["top_left", "top_right", "bottom_left", "bottom_right", "center"], "description": "Default bottom_right" },
                        "size": { "type": "number", "minimum": 0.5, "maximum": 20, "description": "Text height, percent of the short edge (default 3)" },
                        "opacity": { "type": "number", "minimum": 0, "maximum": 1, "description": "Default 0.6" } } },
                "learn": { "type": "boolean", "description": "false: don't queue learning from these edits after this call (for an export split across calls, all but the last)" },
                "keep_location": { "type": "boolean", "description": "Keep the original's GPS location in rendered files (default false: removed, as Lightroom's Remove Location Info)" },
                "record": { "type": "boolean", "description": "false keeps this export out of the usage log" },
                "run_id": { "type": "integer", "description": "The run from start_run: each file is recorded as it is written, and a Photo the run already finished comes back as exported (resumed) without writing it again" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "add_to_apple_photos",
            description: "Import exported files (JPEG, HEIC or TIFF from export_photos) into Apple Photos on this Mac, optionally into an album (made if missing). macOS only; the first time, macOS asks to let Argus control Photos.",
            input_schema: json!({ "type": "object", "required": ["paths"], "properties": {
                "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                "album": { "type": "string", "description": "Album name, e.g. the Shoot's" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "send_to_photos",
            description: "Send exported files (from export_photos) to the person's photo app in the background: apple_photos imports them into Apple Photos on this Mac, google_photos uploads them to Google Photos (needs the person to have connected it in Settings › Export). Either way they go into an album, by default named after the Shoot, made if missing. Returns the Queue job; progress shows in Work mode.",
            input_schema: json!({ "type": "object", "required": ["to", "paths"], "properties": {
                "to": { "enum": ["apple_photos", "google_photos"] },
                "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                "album": { "type": "string", "description": "Album name; the Shoot's name if omitted and shoot_id is given" },
                "shoot_id": { "type": "integer", "description": "The Shoot the files came from: names the album and labels the job" } } }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_exports(
        &self,
        name: &str,
        args: Value,
        lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "export_photos" => {
                let a: ExportPhotos = parse(args)?;
                export_photos(&lib, a)
            }
            "add_to_apple_photos" => {
                let a: AddToApplePhotos = parse(args)?;
                let paths: Vec<std::path::PathBuf> =
                    a.paths.iter().map(|p| export::expand_home(p)).collect();
                if let Some(p) = paths.iter().find(|p| !p.is_file()) {
                    return Err(Error(format!("no file at {}", p.display())));
                }
                export::add_to_apple_photos(&paths, a.album.as_deref())?;
                one(json!({ "added": paths.len() }))
            }
            "send_to_photos" => {
                let a: SendToPhotos = parse(args)?;
                let paths: Vec<std::path::PathBuf> =
                    a.paths.iter().map(|p| export::expand_home(p)).collect();
                if let Some(p) = paths.iter().find(|p| !p.is_file()) {
                    return Err(Error(format!("no file at {}", p.display())));
                }
                if a.to == "google_photos"
                    && crate::google_photos::status(&crate::google_photos::Config::from_env())
                        ["connected"]
                        != json!(true)
                {
                    return Err(Error(
                        "Google Photos isn't connected: connect it in Settings › Export".into(),
                    ));
                }
                let album = match (a.album, a.shoot_id) {
                    (Some(album), _) => Some(album),
                    (None, Some(s)) => lib
                        .shoots()?
                        .into_iter()
                        .find(|x| x.id == s)
                        .map(|x| x.name),
                    (None, None) => None,
                };
                let mut args = json!({ "to": a.to, "paths": paths, "album": album });
                if let Some(s) = a.shoot_id {
                    args["shoot_id"] = json!(s);
                }
                let mut lib = lib;
                let (job_id, created) =
                    crate::jobs::enqueue(&mut lib, "send_photos", &args, None, None)?;
                one(json!({ "job_id": job_id, "created": created, "album": album }))
            }
            other => Err(unknown_tool(other)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendToPhotos {
    to: String,
    paths: Vec<String>,
    album: Option<String>,
    shoot_id: Option<crate::library::ShootId>,
}

fn export_photos(lib: &Library, a: ExportPhotos) -> Result<Vec<Part>> {
    // The person is waiting on this: the Queue starts nothing new meanwhile.
    crate::compute::hold_for_export(lib.root(), 30);
    let opts = export::Options {
        folder: export::expand_home(&a.folder),
        shoot_folder: a.shoot_folder.unwrap_or(true),
        format: a.format.unwrap_or_default(),
        long_edge: a.long_edge,
        short_edge: a.short_edge,
        megapixels: a.megapixels,
        sharpen: a.sharpen,
        quality: a.quality.unwrap_or(90),
        naming: a.naming.unwrap_or_default(),
        sequence_start: a.sequence_start.unwrap_or(1).max(1),
        keep_location: a.keep_location.unwrap_or(false),
        primaries: match a.color_space.as_deref() {
            None | Some("srgb") => crate::pipeline::Primaries::Srgb,
            Some("display_p3") => crate::pipeline::Primaries::DisplayP3,
            Some(other) => return Err(Error(format!("unknown color_space {other}"))),
        },
        watermark: a.watermark.filter(|w| !w.text.trim().is_empty()),
    };
    let shoots: BTreeMap<ShootId, String> =
        lib.shoots()?.into_iter().map(|s| (s.id, s.name)).collect();
    let photos: Vec<(Photo, Recipe)> = a
        .photo_ids
        .iter()
        .map(|&id| {
            let photo = lib.find_photo(id)?;
            let recipe = lib.with_look(&photo, &lib.current_recipe(id)?)?;
            Ok((photo, recipe))
        })
        .collect::<Result<_>>()?;
    let total = a
        .sequence_total
        .unwrap_or(opts.sequence_start + photos.len() - 1);
    let (mut exported, mut failed) = (Vec::new(), Vec::new());
    for (i, (photo, recipe)) in photos.iter().enumerate() {
        let item = export::Item {
            photo,
            recipe,
            verdict: photo.verdict,
            shoot_name: shoots.get(&photo.shoot_id).map_or("", |s| s.as_str()),
            masks: render::masks_for(lib, photo, recipe),
            keywords: lib.kept_keywords(photo.id)?,
            caption: lib.kept_caption(photo.id)?,
        };
        // Resuming a run: a Photo it finished stays as it is (written once, never `-2`).
        let before = match a.run_id {
            Some(run) => lib
                .run_item(run, photo.id)?
                .and_then(|(p, done)| p.map(|p| (p, done))),
            None => None,
        };
        // A name only intended may hold a file that was there before: never taken as done.
        if let (Some(run), Some((path, state))) = (a.run_id, &before) {
            let done = *state == ItemState::Done;
            let whole = *state == ItemState::Claimed
                && export::finished(path)
                && (opts.format != export::Format::Original
                    || export::finished(&path.with_extension("xmp")));
            if done || whole {
                if !done {
                    lib.finish_run_item(run, photo.id, path)?;
                }
                exported.push(export::Exported {
                    photo_id: photo.id,
                    path: path.clone(),
                    sidecar: (opts.format == export::Format::Original)
                        .then(|| path.with_extension("xmp")),
                    width: photo.width,
                    height: photo.height,
                    bytes: std::fs::metadata(path).map_or(0, |m| m.len()),
                    resumed: true,
                });
                continue;
            }
        }
        let claimed = |p: &Path| match a.run_id {
            Some(run) => lib.claim_run_item(run, photo.id, p),
            None => Ok(()),
        };
        let intent = |p: &Path| match a.run_id {
            Some(run) => lib.intend_run_item(run, photo.id, p),
            None => Ok(()),
        };
        let slot = export::Slot {
            reuse: before.as_ref().map(|(p, state)| match state {
                ItemState::Intent => export::Reuse::Intent(p),
                _ => export::Reuse::Claimed(p),
            }),
            intent: &intent,
            claimed: &claimed,
        };
        // One Photo failing (its drive offline, say) doesn't stop the rest, as in Lightroom.
        match export::export_to(&item, &opts, opts.sequence_start + i, total, slot) {
            Ok(x) => {
                if a.record.unwrap_or(true) {
                    lib.log_usage(
                        photo.id,
                        "export",
                        json!({ "recipe": recipe.to_value(), "path": x.path, "format": opts.format,
                                "long_edge": opts.long_edge, "quality": opts.quality,
                                "naming": opts.naming, "verdict": photo.verdict.as_str(),
                                "stars": photo.stars }),
                    );
                }
                if let Some(run) = a.run_id {
                    lib.finish_run_item(run, photo.id, &x.path)?;
                }
                exported.push(x)
            }
            Err(e) => failed.push(json!({ "photo_id": photo.id, "error": e.to_string() })),
        }
    }
    crate::compute::hold_for_export(lib.root(), 10);
    // Every finished edit teaches the Library's own Edit head (best-effort, in the Queue),
    // once per export: the UI passes learn: false on all but its last call, or the lesson
    // re-ran between Photos and halved the export's speed (Greg, 2026-11-02).
    if !exported.is_empty() && a.record.unwrap_or(true) && a.learn.unwrap_or(true) {
        if let Ok(mut l) = Library::open(lib.root()) {
            let _ = jobs::enqueue(&mut l, "learn_edits", &json!({}), None, Some("learn_edits"));
        }
    }
    one(json!({ "exported": exported, "failed": failed }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportPhotos {
    photo_ids: Vec<PhotoId>,
    folder: String,
    format: Option<export::Format>,
    long_edge: Option<usize>,
    short_edge: Option<usize>,
    megapixels: Option<f64>,
    sharpen: Option<export::Sharpen>,
    quality: Option<u8>,
    naming: Option<export::Naming>,
    shoot_folder: Option<bool>,
    sequence_start: Option<usize>,
    sequence_total: Option<usize>,
    keep_location: Option<bool>,
    /// False skips queueing learn_edits after this call (all but the last of a split export).
    learn: Option<bool>,
    color_space: Option<String>,
    watermark: Option<crate::watermark::Watermark>,
    /// False keeps this export out of the usage log (bench exports of the log itself).
    record: Option<bool>,
    /// A resumable run's id ([`crate::library::Run`]).
    run_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddToApplePhotos {
    paths: Vec<String>,
    album: Option<String>,
}
