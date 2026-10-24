//! Export tool.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "export_photos",
            description: "Export Photos into a folder on this Mac (created if missing), as Lightroom's Export does: JPEG, HEIC (macOS only) or TIFF rendered with each Photo's edits and Look, or the untouched original plus an XMP sidecar Lightroom reads. Names never contain spaces and nothing is overwritten (an existing name gets -2). JPEGs keep capture metadata but never location. Photos that fail are listed under failed; the rest still export.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "folder"], "properties": {
                "photo_ids": ids,
                "folder": { "type": "string", "description": "Export location: an absolute path, or starting with ~/" },
                "format": { "enum": ["jpeg", "heic", "tiff", "original"], "description": "Default jpeg; heic needs macOS" },
                "long_edge": { "type": "integer", "minimum": 64, "description": "Pixels; full size if omitted" },
                "short_edge": { "type": "integer", "minimum": 64, "description": "Pixels on the short side instead (1080 for Instagram); never upscales" },
                "megapixels": { "type": "number", "exclusiveMinimum": 0, "description": "Size by megapixels instead; never upscales" },
                "sharpen": { "enum": ["low", "standard", "high"], "description": "Output sharpening for screen, as Lightroom's Export; off if omitted" },
                "quality": { "type": "integer", "minimum": 50, "maximum": 100, "description": "JPEG and HEIC quality, default 90" },
                "naming": { "enum": ["shoot", "sequence", "original"],
                    "description": "shoot (default): 2026-10-18_Boat-Trip_DSC08005; sequence: 2026-10-18_Boat-Trip_001; original: DSC08005" },
                "shoot_folder": { "type": "boolean", "description": "Put files in a folder named after their Shoot inside folder (default true)" },
                "sequence_start": { "type": "integer", "minimum": 1, "description": "Number of the first Photo, for sequence names (default 1)" },
                "sequence_total": { "type": "integer", "minimum": 1, "description": "Photos in the whole export, when it is split across calls (sets the digits)" },
                "record": { "type": "boolean", "description": "false keeps this export out of the usage log" } } }),
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
            other => Err(unknown_tool(other)),
        }
    }
}

fn export_photos(lib: &Library, a: ExportPhotos) -> Result<Vec<Part>> {
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
    };
    let shoots: BTreeMap<ShootId, String> =
        lib.shoots()?.into_iter().map(|s| (s.id, s.name)).collect();
    let photos: Vec<(Photo, Recipe)> = a
        .photo_ids
        .iter()
        .map(|&id| {
            let photo = find_photo(lib, id)?;
            let recipe = with_look(lib, &photo, &current_recipe(lib, id)?)?;
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
        // One Photo failing (its drive offline, say) doesn't stop the rest, as in Lightroom.
        match export::export(&item, &opts, opts.sequence_start + i, total) {
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
                exported.push(x)
            }
            Err(e) => failed.push(json!({ "photo_id": photo.id, "error": e.to_string() })),
        }
    }
    // Every finished edit teaches the Library's own Edit head (best-effort, in the Queue).
    if !exported.is_empty() && a.record.unwrap_or(true) {
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
    /// False keeps this export out of the usage log (bench exports of the log itself).
    record: Option<bool>,
}
