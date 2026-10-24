//! Library tools: Shoots, Photos, Import, storage and settings, originals and cards, previews,
//! Verdicts and star Ratings.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    let verdict = json!({ "type": "string", "enum": ["pick", "reject", "unflagged"] });
    vec![
        ToolSpec {
            name: "list_shoots",
            description: "List the Shoots in the Library (one per Import) with their Photo counts.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "rename_shoot",
            description: "Rename a Shoot. The name is stored by the naming rules (date first, words joined by hyphens, no spaces: \"Boat tour\" becomes 2026-10-10_Boat-tour); the Shoot keeps its date unless the name starts with a new one. Its originals folder inside the Library is renamed to match.",
            input_schema: json!({ "type": "object", "required": ["shoot_id", "name"], "properties": {
                "shoot_id": id, "name": { "type": "string", "minLength": 1 } } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_photos",
            description: "List Photos in capture order with their Verdict (pick, reject, unflagged), who set it and why, their star Rating (0-5, 0 = unrated) and who set it, their Stack, and `best`: their rank in the Shoot's Best Photos. Narrow by Shoot, Verdict and/or least stars.",
            input_schema: json!({ "type": "object", "properties": {
                "shoot_id": id, "verdict": verdict,
                "min_stars": { "type": "integer", "minimum": 0, "maximum": 5 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "import_folder",
            description: "Import a folder on this Mac (recursively; RAW and JPEG) into the Library as one new Shoot, or only the `files` you name from it. Duplicates are skipped and originals are never changed; from a memory card they are copied into the Library (see `copy`). Takes about a second per RAW. Photos appear in the Shoot as they land, so `photos` and `progress` can follow a running Import.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path" },
                "shoot_name": { "type": "string", "description": "Defaults to the folder's name" },
                "files": { "type": "array", "items": { "type": "string" },
                    "description": "Absolute paths of the files to import, all inside folder (e.g. from scan_folder); default everything" },
                "raw_only": { "type": "boolean",
                    "description": "For RAW+JPEG frames, skip the camera's JPEG. By default it is kept with its RAW as one Photo" },
                "copy": { "type": "boolean",
                    "description": "Copy the originals into the Library's originals folder (verified) instead of reading them in place. Default: true from a memory card, false from a folder" },
                "cull": { "type": "boolean",
                    "description": "Then queue an AI cull, edit Suggestions, and suggested captions and Keywords for the new Shoot" },
                "auto": { "type": "boolean",
                    "description": "Then run Auto (as auto_edit) on every new Photo, as one undoable batch" },
                "provider": { "type": "string", "description": "ollama, claude-code or claude-api; default ARGUS_PROVIDER, else ollama" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "estimate_import",
            description: "How much disk an Import would take, before running it: the originals' size, whether they'd be copied into the Library (by default yes from a memory card, no from a folder), the estimated previews, the total needed and the free space on the Library's disk. Reads file sizes only.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path" },
                "files": { "type": "array", "items": { "type": "string" },
                    "description": "Only these files (e.g. the new ones from scan_folder); default everything" },
                "copy": { "type": "boolean", "description": "Copy into the Library; default true for a memory card, false otherwise" },
                "raw_only": { "type": "boolean", "description": "As import_folder: leave out the camera JPEGs of RAW+JPEG frames" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "library_storage",
            description: "Where the Library folder is and how much disk each part takes: originals copied into it, originals read from elsewhere, previews, cache, database and backups, plus how many originals are missing (moved or on a disconnected disk) and the free space on its disk.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "learn_edit_head",
            description: "Learn this Library's own Edit head from every RAW the person has exported: what Auto makes of each against the sliders they finished it with, on top of the head Argus ships. Auto uses it when the Edit head is on. Returns how many edits it learned from and the average offset from Auto it now adds.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: false,
        },
        ToolSpec {
            name: "clear_cache",
            description: "Delete what the Library can rebuild (its cache folder and previews no Photo uses). Never touches originals, edits or the previews in use. Returns the bytes freed.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: false,
        },
        ToolSpec {
            name: "library_settings",
            description: "Read the Library's settings, or change them by passing values: preview_long_edge (512-4096 px, default 2048) and preview_quality (50-95, default 85) for the previews Import makes. Smaller previews save disk; run rebuild_previews to apply to existing Photos.",
            input_schema: json!({ "type": "object", "properties": {
                "preview_long_edge": { "type": "integer" },
                "preview_quality": { "type": "integer" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "rebuild_previews",
            description: "Re-make every Photo's preview at the current library_settings. Slow: about a second per RAW. Photos whose original is missing keep their old preview.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: false,
        },
        ToolSpec {
            name: "find_missing_originals",
            description: "List Photos whose original file can't be found where the Library expects it (moved, renamed folder, or a disk that isn't connected).",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "relink_originals",
            description: "Look for missing originals under a folder (recursively), match them by name and size, confirm each by content hash, and point the Library at them. Nothing in the folder is changed.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path to search" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "find_cards",
            description: "List the memory cards mounted on this Mac (volumes with a camera DCIM folder), with their paths.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "scan_folder",
            description: "List the photos in a folder or on a card without importing them: path, name, size, capture time, camera, and whether the Library already has each one (same name, size and capture time). A RAW+JPEG frame is one entry, the RAW, with its JPEG beside it, as Import makes it one Photo. Fast: reads only file headers.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path, e.g. a card from find_cards" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "get_preview",
            description: "Look at one Photo: its details plus a JPEG preview (long edge 1024 by default, at most 1536).",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id,
                "long_edge": { "type": "integer", "minimum": 64, "maximum": MAX_PREVIEW_EDGE } } }),
            read_only: true,
        },
        ToolSpec {
            name: "get_contact_sheet",
            description: "See a whole Shoot at once, as you need to before judging any one Photo: a grid image of up to 30 numbered thumbnails per page, in capture order, plus a legend mapping each number to its Photo id, Verdict and Stack. Ask for further pages while `pages` is larger.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": {
                "shoot_id": id, "page": { "type": "integer", "minimum": 1 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "set_verdict",
            description: "Set one Verdict on many Photos. By default it is recorded as the AI's suggestion with your reason, and Photos the person flagged themselves are left alone (reported as kept). Use as=person only when the person told you to flag those exact Photos.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "verdict"], "properties": {
                "photo_ids": ids, "verdict": verdict,
                "reason": { "type": "string", "description": "Short, for the person: why" },
                "as": { "type": "string", "enum": ["ai", "person"] } } }),
            read_only: false,
        },
        ToolSpec {
            name: "set_rating",
            description: "Set one star Rating (Lightroom stars: 0 = none, 1 = reject-worthy, 3 = solid keeper, 5 = the best of the Shoot) on many Photos. By default it is recorded as the AI's suggestion, and Photos the person rated themselves are left alone (reported as kept). Use as=person only when the person told you to rate those exact Photos.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "stars"], "properties": {
                "photo_ids": ids,
                "stars": { "type": "integer", "minimum": 0, "maximum": 5 },
                "as": { "type": "string", "enum": ["ai", "person"] } } }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_library(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "list_shoots" => one(lib.shoots()?),
            "rename_shoot" => {
                let a: RenameShoot = parse(args)?;
                one(lib.rename_shoot(a.shoot_id, &a.name)?)
            }
            "list_photos" => {
                let a: ListPhotos = parse(args)?;
                let photos = lib.list_photos(&PhotoFilter {
                    shoot: a.shoot_id,
                    verdict: a.verdict,
                })?;
                let min = a.min_stars.unwrap_or(0);
                one(photos
                    .iter()
                    .filter(|p| p.stars >= min)
                    .map(PhotoView::from)
                    .collect::<Vec<_>>())
            }
            "import_folder" => {
                let a: ImportFolder = parse(args)?;
                let folder = Path::new(&a.folder);
                let report = lib.import(
                    folder,
                    &ImportOptions {
                        shoot_name: a.shoot_name,
                        files: a.files.map(|f| f.into_iter().map(PathBuf::from).collect()),
                        raw_only: a.raw_only,
                        copy: a.copy.unwrap_or_else(|| is_on_card(folder)),
                        ..Default::default()
                    },
                )?;
                // RAWs come in showing the camera's preview; Argus's own render follows on
                // the background Queue at the Compute level.
                if let (Some(shoot), 1..) = (report.shoot_id, report.camera_previews) {
                    jobs::enqueue(
                        &mut lib,
                        "render_previews",
                        &json!({ "shoot_id": shoot }),
                        None,
                        None,
                    )?;
                }
                // The specialist models' Facts, after the previews they're read from.
                if let Some(shoot) = report.shoot_id {
                    crate::analyzers::enqueue(&mut lib, Some(shoot), None)?;
                }
                // Auto first, so the cull and Suggestions see the toned Photos. A failed
                // Auto leaves the Import as it is.
                if let (Some(shoot), true) = (report.shoot_id, a.auto) {
                    let auto = lib
                        .begin_batch(Author::Person, "Auto on import")
                        .and_then(|b| {
                            let a = AutoEdit {
                                photo_ids: None,
                                shoot_id: Some(shoot),
                                compute: None,
                            };
                            auto_edit(&mut lib, a, Some(b))
                        });
                    if let Err(e) = auto {
                        eprintln!("argus import: Auto failed: {e}");
                    }
                }
                // Queued here rather than by the caller, so the app can leave the dialog
                // (and reload into the new Shoot) while the Import still runs.
                if let Some(shoot) = report.shoot_id {
                    let kinds = [
                        // The rules' cull runs at once; a model's waits like other model work.
                        (
                            match cull::check(None) {
                                cull::Check::Rules => "check_shoot",
                                cull::Check::Model => "cull_shoot",
                            },
                            a.cull,
                        ),
                        ("suggest_shoot", a.suggest.unwrap_or(a.cull)),
                        ("tag_photos", a.tag.unwrap_or(a.cull)),
                    ];
                    let mut args = json!({ "shoot_id": shoot });
                    if let Some(p) = &a.provider {
                        args["provider"] = json!(p);
                    }
                    for (kind, _) in kinds.iter().filter(|(_, on)| *on) {
                        jobs::enqueue(&mut lib, kind, &args, None, None)?;
                    }
                }
                one(report)
            }
            "estimate_import" => {
                let a: EstimateImport = parse(args)?;
                let folder = Path::new(&a.folder);
                let files: Option<Vec<PathBuf>> =
                    a.files.map(|f| f.into_iter().map(PathBuf::from).collect());
                let copy = a.copy.unwrap_or_else(|| is_on_card(folder));
                one(lib.estimate_import(folder, files.as_deref(), copy, a.raw_only)?)
            }
            "library_storage" => one(lib.storage()?),
            "learn_edit_head" => one(crate::edit_head::learn_library(&lib)?),
            "clear_cache" => one(lib.clear_cache()?),
            "library_settings" => {
                let a: SettingsArgs = parse(args)?;
                let mut s = lib.settings()?;
                if let Some(v) = a.preview_long_edge {
                    s.preview_long_edge = v;
                }
                if let Some(v) = a.preview_quality {
                    s.preview_quality = v;
                }
                if a.preview_long_edge.is_some() || a.preview_quality.is_some() {
                    lib.set_settings(&s)?;
                }
                one(s)
            }
            "rebuild_previews" => one(lib.rebuild_previews(4)?),
            "find_missing_originals" => one(lib.missing_originals()?),
            "relink_originals" => {
                let a: ScanFolder = parse(args)?;
                one(lib.relink(Path::new(&a.folder))?)
            }
            "find_cards" => one(crate::library::find_cards()),
            "scan_folder" => {
                let a: ScanFolder = parse(args)?;
                one(lib.scan(Path::new(&a.folder))?)
            }
            "get_preview" => {
                let a: GetPreview = parse(args)?;
                let photo = find_photo(&lib, a.photo_id)?;
                let edge = a.long_edge.unwrap_or(1024).clamp(64, MAX_PREVIEW_EDGE);
                let img = open_preview(&photo)?.thumbnail(edge, edge).to_rgb8();
                Ok(vec![
                    Part::Json(to_json(PhotoView::from(&photo))?),
                    Part::Jpeg(encode_jpeg(&img)?),
                ])
            }
            "get_contact_sheet" => {
                let a: GetContactSheet = parse(args)?;
                contact_sheet(&lib, a.shoot_id, a.page.unwrap_or(1))
            }
            "set_verdict" => {
                let a: SetVerdict = parse(args)?;
                set_verdict(&mut lib, a, batch)
            }
            "set_rating" => {
                let a: SetRating = parse(args)?;
                set_rating(&mut lib, a, batch)
            }
            other => Err(unknown_tool(other)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListPhotos {
    shoot_id: Option<ShootId>,
    verdict: Option<Verdict>,
    min_stars: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetRating {
    photo_ids: Vec<PhotoId>,
    stars: u8,
    #[serde(rename = "as")]
    author: Option<Author>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameShoot {
    shoot_id: ShootId,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportFolder {
    folder: String,
    shoot_name: Option<String>,
    files: Option<Vec<String>>,
    #[serde(default)]
    raw_only: bool,
    copy: Option<bool>,
    /// Queue an AI cull once the Photos are in.
    #[serde(default)]
    cull: bool,
    /// Queue edit Suggestions once the Photos are in (default: as `cull`).
    suggest: Option<bool>,
    /// Queue the AI's captions and Keywords once the Photos are in (default: as `cull`).
    tag: Option<bool>,
    /// Lightroom's Auto on every new Photo, as one batch of the person's (⌘Z undoes it).
    #[serde(default)]
    auto: bool,
    /// The model the queued cull and Suggestions use (default: ARGUS_PROVIDER, else Ollama).
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EstimateImport {
    folder: String,
    files: Option<Vec<String>>,
    copy: Option<bool>,
    #[serde(default)]
    raw_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsArgs {
    preview_long_edge: Option<u32>,
    preview_quality: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanFolder {
    folder: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetPreview {
    photo_id: PhotoId,
    long_edge: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetContactSheet {
    shoot_id: ShootId,
    page: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetVerdict {
    photo_ids: Vec<PhotoId>,
    verdict: Verdict,
    reason: Option<String>,
    #[serde(rename = "as")]
    author: Option<Author>,
}

fn set_verdict(lib: &mut Library, a: SetVerdict, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let author = a.author.unwrap_or(Author::Ai);
    let all = lib.list_photos(&PhotoFilter::default())?;
    let mut changed = Vec::new();
    let mut kept = Vec::new();
    for &id in &a.photo_ids {
        let p = all
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error(format!("no Photo with id {id}")))?;
        if author == Author::Ai && p.verdict_by == Some(Author::Person) {
            kept.push(id);
        } else {
            changed.push(id);
        }
    }
    let batch = match batch {
        Some(b) => b,
        None => lib.begin_batch(author, "set_verdict")?,
    };
    lib.set_verdict(
        &changed,
        a.verdict,
        author,
        a.reason.as_deref(),
        Some(batch),
    )?;
    one(json!({ "verdict": a.verdict, "changed": changed, "kept_person_verdicts": kept }))
}

fn set_rating(lib: &mut Library, a: SetRating, batch: Option<BatchId>) -> Result<Vec<Part>> {
    if a.stars > 5 {
        return Err(Error(format!("stars must be 0 to 5, not {}", a.stars)));
    }
    let author = a.author.unwrap_or(Author::Ai);
    let all = lib.list_photos(&PhotoFilter::default())?;
    let mut changed = Vec::new();
    let mut kept = Vec::new();
    for &id in &a.photo_ids {
        let p = all
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error(format!("no Photo with id {id}")))?;
        if author == Author::Ai && p.stars_by == Some(Author::Person) {
            kept.push(id);
        } else {
            changed.push(id);
        }
    }
    let batch = match batch {
        Some(b) => b,
        None => lib.begin_batch(author, "set_rating")?,
    };
    lib.set_rating(&changed, a.stars, author, Some(batch))?;
    one(json!({ "stars": a.stars, "changed": changed, "kept_person_ratings": kept }))
}

/// One page of a Shoot's Contact sheet: a grid JPEG with each cell numbered (1-based
/// position in capture order across the whole Shoot) and a legend for those numbers.
fn contact_sheet(lib: &Library, shoot: ShootId, page: usize) -> Result<Vec<Part>> {
    let photos = shoot_photos(lib, shoot)?;
    let pages = photos.len().div_ceil(SHEET_PER_PAGE).max(1);
    if page == 0 || page > pages {
        return Err(Error(format!("page {page} is out of range 1..={pages}")));
    }
    let first = (page - 1) * SHEET_PER_PAGE;
    let on_page: Vec<(usize, &Photo)> = photos
        .iter()
        .enumerate()
        .skip(first)
        .take(SHEET_PER_PAGE)
        .map(|(i, p)| (i + 1, p))
        .collect();

    let legend: Vec<Value> = on_page
        .iter()
        .map(|(n, p)| {
            json!({ "n": n, "photo_id": p.id, "file_name": p.file_name, "captured_at": p.captured_at,
                    "verdict": p.verdict, "verdict_by": p.verdict_by, "stars": p.stars, "stack": p.stack, "stack_rank": p.stack_rank, "best": p.best })
        })
        .collect();
    let info = json!({ "shoot_id": shoot, "page": page, "pages": pages,
                       "photos_in_shoot": photos.len(), "photos": legend });
    if on_page.is_empty() {
        return Ok(vec![Part::Json(info)]);
    }

    let thumbs = on_page
        .par_iter()
        .map(|(_, p)| {
            Ok(open_preview(p)?
                .thumbnail(SHEET_CELL - 8, SHEET_CELL - 8)
                .to_rgb8())
        })
        .collect::<Result<Vec<_>>>()?;
    let cols = SHEET_COLUMNS.min(on_page.len() as u32);
    let rows = (on_page.len() as u32).div_ceil(SHEET_COLUMNS);
    let mut sheet = image::RgbImage::from_pixel(
        cols * SHEET_CELL,
        rows * SHEET_CELL,
        image::Rgb([24, 24, 24]),
    );
    for (i, ((n, _), t)) in on_page.iter().zip(&thumbs).enumerate() {
        let (cx, cy) = (
            (i as u32 % SHEET_COLUMNS) * SHEET_CELL,
            (i as u32 / SHEET_COLUMNS) * SHEET_CELL,
        );
        let (x, y) = (
            cx + (SHEET_CELL - t.width()) / 2,
            cy + (SHEET_CELL - t.height()) / 2,
        );
        image::imageops::replace(&mut sheet, t, x as i64, y as i64);
        draw_number(&mut sheet, cx + 4, cy + 4, *n);
    }
    Ok(vec![Part::Json(info), Part::Jpeg(encode_jpeg(&sheet)?)])
}
