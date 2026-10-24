//! Auto, white balance, version history and undo, Looks and Paste: edits across Photos.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "auto_edit",
            description: "Lightroom's Auto on one or many Photos (photo_ids, or every Photo of shoot_id), as one undoable batch: sets Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Temp, Tint and Vibrance from the Photo's own light, deterministically and in well under a second per Photo (no model). Everything else in the Recipe (Look, HSL, Masks, crop) is kept. A good first step before finer edits.",
            input_schema: json!({ "type": "object", "properties": {
                "photo_ids": ids, "shoot_id": id,
                "compute": { "type": "string", "enum": ["gentle", "balanced", "full"] } } }),
            read_only: false,
        },
        ToolSpec {
            name: "white_balance",
            description: WHITE_BALANCE_DOC,
            input_schema: json!({ "type": "object", "required": ["photo_ids"], "properties": {
                "photo_ids": ids,
                "preset": { "type": "string", "enum": ["as_shot", "auto", "daylight", "cloudy", "shade", "tungsten", "fluorescent", "flash"] },
                "at": { "type": "object", "required": ["x", "y"], "description": "Eyedropper: a point that should be neutral grey, x and y 0..1 of the frame as render_photo shows it",
                    "properties": { "x": { "type": "number" }, "y": { "type": "number" } } },
                "dry_run": { "type": "boolean", "description": "Work out temp and tint without saving them" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_versions",
            description: "A Photo's Recipe history, oldest first: every saved Version with who made it (person or ai), its batch and label, and the Recipe.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "undo_batch",
            description: "Undo one batch of changes (an Agent turn, a cull, a Look change): every Recipe, Verdict, Rating and Look it touched goes back to how it was before. Without batch_id, undoes the newest AI batch. The undo is itself a new batch.",
            input_schema: json!({ "type": "object", "properties": { "batch_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_looks",
            description: "List a Shoot's Looks (shared styles applied under each Photo's own sliders), each with its id, name and sliders.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "set_look",
            description: SET_LOOK_DOC,
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": {
                "shoot_id": id,
                "look_id": { "type": "string", "description": "An existing Look to tune; omit to create one (or tune the Shoot's only Look)" },
                "name": { "type": "string", "description": "For a new Look, e.g. \"Warm film\"" },
                "set": { "type": "object", "additionalProperties": { "type": ["number", "array"] },
                    "description": "Setting path -> value, as in edit_recipe (no correction.*)" },
                "relative": { "type": "boolean", "description": "Add to the Look's current values (default false)" },
                "photo_ids": { "type": "array", "items": { "type": "integer" },
                    "description": "Photos to put under this Look; a new Look defaults to the Shoot's Picks" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "remove_look",
            description: "Take a Look off every Photo under it, leaving each Photo's own sliders. The Look itself is kept, so undo puts the Photos back under it.",
            input_schema: json!({ "type": "object", "required": ["shoot_id", "look_id"], "properties": {
                "shoot_id": id, "look_id": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "sync_to_look",
            description: "Lightroom's Sync: fold one Photo's own sliders into its Shoot's Look so every Photo under that Look gets them, and clear them on that Photo. Creates a Look for the Shoot's Picks if there is none.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "paste_settings",
            description: "Lightroom's Copy and Paste settings, and Sync settings: put groups of one Photo's settings on other Photos as one undoable batch, leaving their other settings as they are. A slider the source never moved is reset in a pasted group. Groups: wb, light, color, effects, detail, optics, calibration, retouch, masks, spots, crop; by default all but masks, spots and crop. When the source is under a different Look than a target, the target gets the settings as they render (Look included) and leaves its Look.",
            input_schema: json!({ "type": "object", "required": ["from", "photo_ids"], "properties": {
                "from": { "type": "integer", "description": "The Photo to copy from" },
                "photo_ids": ids,
                "groups": { "type": "array", "items": { "enum": ["wb", "light", "color", "effects", "detail", "optics", "calibration", "retouch", "masks", "spots", "crop"] } } } }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_looks(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "auto_edit" => auto_edit(&mut lib, parse(args)?, batch),
            "white_balance" => white_balance(&mut lib, parse(args)?, batch),
            "list_versions" => {
                let a: PhotoArg = parse(args)?;
                find_photo(&lib, a.photo_id)?;
                one(lib.versions(a.photo_id)?)
            }
            "undo_batch" => {
                let a: UndoBatch = parse(args)?;
                let id = match a.batch_id {
                    Some(b) => b,
                    None => {
                        lib.last_batch(Some(Author::Ai))?
                            .ok_or_else(|| Error("there is no AI change to undo".into()))?
                            .0
                    }
                };
                one(lib.undo_batch(id, Author::Person)?)
            }
            "list_looks" => {
                let a: ShootArg = parse(args)?;
                shoot_photos(&lib, a.shoot_id)?;
                one(lib.looks(a.shoot_id)?)
            }
            "set_look" => {
                let a: SetLook = parse(args)?;
                set_look(&mut lib, a, batch)
            }
            "remove_look" => {
                let a: RemoveLook = parse(args)?;
                let under = photos_under(&lib, a.shoot_id, &a.look_id)?;
                let batch = batch_or(&mut lib, batch, "Remove Look")?;
                for &id in &under {
                    let mut r = current_recipe(&lib, id)?;
                    r.look = None;
                    lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
                }
                one(json!({ "batch_id": batch, "removed_from": under }))
            }
            "sync_to_look" => {
                let a: SyncToLook = parse(args)?;
                sync_to_look(&mut lib, a, batch)
            }
            "paste_settings" => paste_settings(&mut lib, parse(args)?, batch),
            other => Err(unknown_tool(other)),
        }
    }
}

const SET_LOOK_DOC: &str = "Create or tune one of a Shoot's Looks: a shared style (colour grade, contrast, mood) \
under each Photo's own sliders, so a set reads as one shoot. Tuning a Look changes every Photo under it at once. \
Sliders as in edit_recipe; for consistency across frames shot in different light, pair it with per-Photo \
correction.exposure and correction.wbK through edit_recipe. One undoable batch.";

const WHITE_BALANCE_DOC: &str = "Lightroom's white balance menu and eyedropper on one or many Photos, \
as one undoable batch: sets temp and tint (relative to as shot). preset as_shot (the camera's), auto (from the Photo's own light), \
or a Kelvin preset for RAW Photos: daylight 5500 K, cloudy 6500 K, shade 7500 K, tungsten 2850 K, fluorescent 3800 K, flash 5500 K. \
Or `at` {x, y}: the eyedropper, making that spot neutral grey (pick something that should be white or grey, not a highlight). \
Returns each Photo's temp and tint.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WhiteBalance {
    photo_ids: Vec<PhotoId>,
    preset: Option<String>,
    at: Option<At>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Deserialize)]
struct At {
    x: f64,
    y: f64,
}

/// White balance ([`crate::white_balance`]): global temp and tint for a preset or an
/// eyedropper point, net of the Photo's Correction, so the Photo ends up there.
fn white_balance(lib: &mut Library, a: WhiteBalance, batch: Option<BatchId>) -> Result<Vec<Part>> {
    use crate::white_balance as wb;
    if a.photo_ids.is_empty() {
        return Err(Error("pass photo_ids".into()));
    }
    let mut next = Vec::new();
    for &id in &a.photo_ids {
        let photo = find_photo(lib, id)?;
        let own = current_recipe(lib, id)?;
        // The Correction's own white balance shift, in Temp steps.
        let corr = own.correction.wb_k / 25.0;
        let with = |temp: f64, tint: f64| {
            let mut r = own.clone();
            r.global.temp = temp;
            r.global.tint = tint;
            r
        };
        let render = |temp: f64, tint: f64| -> Result<crate::RgbImage> {
            let r = with_look(lib, &photo, &with(temp, tint))?;
            render::render_in(lib, &photo, &r, Some(crate::auto::MEASURE_EDGE))
        };
        let (temp, tint) = match (&a.at, a.preset.as_deref()) {
            (Some(p), None) => {
                wb::neutral_at(p.x, p.y, (own.global.temp, own.global.tint), render)?
            }
            (None, Some("as_shot")) => (-corr, 0.0),
            (None, Some("auto")) => {
                let (t, n) = crate::auto::white_balance(&render(-corr, 0.0)?)
                    .ok_or_else(|| Error(format!("Photo {id} has nothing to balance on")))?;
                (t - corr, n)
            }
            (None, Some(name)) => {
                if photo.kind != "raw" {
                    return Err(Error(format!(
                        "Photo {id} is a JPEG: Kelvin presets need a RAW; use as_shot, auto or the eyedropper"
                    )));
                }
                let raw = crate::decode(std::path::Path::new(&photo.path))?;
                let k = wb::as_shot_kelvin(raw.wb, raw.xyz_to_cam).ok_or_else(|| {
                    Error(format!(
                        "can't read the camera's white balance for Photo {id}"
                    ))
                })?;
                let (t, n) = wb::preset(name, k)?;
                (t - corr, n)
            }
            _ => return Err(Error("pass either preset or at".into())),
        };
        next.push((
            id,
            with(temp.clamp(-100.0, 100.0), tint.clamp(-100.0, 100.0)),
        ));
    }
    let out: BTreeMap<PhotoId, Value> = next
        .iter()
        .map(|(id, r)| (*id, json!({ "temp": r.global.temp, "tint": r.global.tint })))
        .collect();
    if a.dry_run {
        return one(json!({ "photos": out }));
    }
    let batch = batch_or(lib, batch, "White balance")?;
    for (id, r) in next {
        lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
    }
    one(json!({ "batch_id": batch, "photos": out }))
}

/// Put Photos under a Look by saving a Recipe Version that names it.
fn assign_look(lib: &mut Library, ids: &[PhotoId], look: &str, batch: BatchId) -> Result<()> {
    for &id in ids {
        let mut r = current_recipe(lib, id)?;
        if r.look.as_deref() != Some(look) {
            r.look = Some(look.to_string());
            lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
        }
    }
    Ok(())
}

fn picks_or_all(lib: &Library, shoot: ShootId) -> Result<Vec<PhotoId>> {
    let photos = shoot_photos(lib, shoot)?;
    let picks: Vec<PhotoId> = photos
        .iter()
        .filter(|p| p.verdict == Verdict::Pick)
        .map(|p| p.id)
        .collect();
    Ok(if picks.is_empty() {
        photos.iter().map(|p| p.id).collect()
    } else {
        picks
    })
}

fn set_look(lib: &mut Library, a: SetLook, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let photos = shoot_photos(lib, a.shoot_id)?;
    let looks = lib.looks(a.shoot_id)?;
    let existing = match &a.look_id {
        Some(id) => Some(looks.iter().find(|l| &l.id == id).cloned().ok_or_else(|| {
            Error(format!(
                "Shoot {} has no Look {id:?}; omit look_id to create one",
                a.shoot_id
            ))
        })?),
        None if a.name.is_none() && looks.len() == 1 => looks.first().cloned(),
        None => None,
    };
    let created = existing.is_none();
    let mut look = existing.unwrap_or_else(|| {
        let name = a.name.clone().unwrap_or_else(|| "Shoot Look".into());
        let mut id = crate::naming::id_slug(&name, "look");
        while looks.iter().any(|l| l.id == id) {
            id = format!("{id}-2");
        }
        Look {
            id,
            name,
            adjust: Default::default(),
        }
    });
    if let (false, Some(name)) = (created, &a.name) {
        look.name = name.clone();
    }
    let mut tmp = Recipe {
        global: look.adjust.clone(),
        ..Recipe::default()
    };
    for (path, v) in &a.set {
        if path.starts_with("correction.") {
            return Err(Error(format!(
                "{path}: Corrections are per Photo; set them with edit_recipe"
            )));
        }
        tmp.set_value(path, v, a.relative)?;
    }
    look.adjust = tmp.global;
    let targets = match a.photo_ids {
        Some(ids) => {
            for id in &ids {
                if !photos.iter().any(|p| p.id == *id) {
                    return Err(Error(format!("Photo {id} is not in Shoot {}", a.shoot_id)));
                }
            }
            ids
        }
        None if created => picks_or_all(lib, a.shoot_id)?,
        None => Vec::new(),
    };
    let batch = batch_or(lib, batch, &format!("Look: {}", look.name))?;
    lib.save_look(a.shoot_id, &look, Some(batch))?;
    assign_look(lib, &targets, &look.id, batch)?;
    let under = photos_under(lib, a.shoot_id, &look.id)?;
    one(
        json!({ "batch_id": batch, "look": look, "created": created, "applied_to": targets, "photos_under_look": under }),
    )
}

fn photos_under(lib: &Library, shoot: ShootId, look: &str) -> Result<Vec<PhotoId>> {
    let mut out = Vec::new();
    for p in shoot_photos(lib, shoot)? {
        if current_recipe(lib, p.id)?.look.as_deref() == Some(look) {
            out.push(p.id);
        }
    }
    Ok(out)
}

fn sync_to_look(lib: &mut Library, a: SyncToLook, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let photo = find_photo(lib, a.photo_id)?;
    let mut r = current_recipe(lib, photo.id)?;
    let looks = lib.looks(photo.shoot_id)?;
    let current = r
        .look
        .as_ref()
        .and_then(|id| looks.iter().find(|l| &l.id == id))
        .or(looks.first())
        .cloned();
    let created = current.is_none();
    let mut look = current.unwrap_or_else(|| Look {
        id: "shoot-look".into(),
        name: "Shoot Look".into(),
        adjust: Default::default(),
    });
    look.adjust = look.adjust.plus(&r.global).clamped();
    let batch = batch_or(lib, batch, &format!("Sync to {}", look.name))?;
    lib.save_look(photo.shoot_id, &look, Some(batch))?;
    r.global = Default::default();
    r.look = Some(look.id.clone());
    lib.write_recipe(photo.id, &r.to_value(), Author::Person, Some(batch))?;
    if created {
        let others: Vec<PhotoId> = picks_or_all(lib, photo.shoot_id)?
            .into_iter()
            .filter(|&id| id != photo.id)
            .collect();
        assign_look(lib, &others, &look.id, batch)?;
    }
    let under = photos_under(lib, photo.shoot_id, &look.id)?;
    one(json!({ "batch_id": batch, "look": look, "created": created, "photos_under_look": under }))
}

/// Copy and paste settings ([`crate::paste`]). Under the same Look (or none) the source's
/// own sliders are pasted and the target keeps its Look; otherwise both are taken as they
/// render, so the target looks like the source and stops following its old Look.
fn paste_settings(
    lib: &mut Library,
    a: PasteSettings,
    batch: Option<BatchId>,
) -> Result<Vec<Part>> {
    let groups = crate::paste::groups(a.groups.as_deref())?;
    if groups.is_empty() {
        return Err(Error("choose at least one settings group".into()));
    }
    let src_photo = find_photo(lib, a.from)?;
    let src = current_recipe(lib, src_photo.id)?;
    let mut next = Vec::new();
    for &id in a.photo_ids.iter().filter(|&&id| id != a.from) {
        let photo = find_photo(lib, id)?;
        let own = current_recipe(lib, id)?;
        let same_look = src.look == own.look && src_photo.shoot_id == photo.shoot_id;
        let r = if same_look || (src.look.is_none() && own.look.is_none()) {
            crate::paste::paste(&src, &own, &groups)?
        } else {
            let mut flat = with_look(lib, &photo, &own)?;
            flat.look = None;
            crate::paste::paste(&with_look(lib, &src_photo, &src)?, &flat, &groups)?
        };
        next.push((id, r));
    }
    if next.is_empty() {
        return Err(Error(
            "paste onto at least one Photo other than the source".into(),
        ));
    }
    let batch = batch_or(
        lib,
        batch,
        &format!("Paste settings from {}", src_photo.file_name),
    )?;
    let author = lib.batch_author(batch)?;
    let mut versions = BTreeMap::new();
    for (id, mut r) in next {
        r.global = r.global.clamped();
        versions.insert(
            id,
            lib.write_recipe(id, &r.to_value(), author, Some(batch))?,
        );
    }
    one(json!({ "batch_id": batch, "versions": versions, "groups": groups }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UndoBatch {
    batch_id: Option<BatchId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetLook {
    shoot_id: ShootId,
    look_id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    set: BTreeMap<String, Value>,
    #[serde(default)]
    relative: bool,
    photo_ids: Option<Vec<PhotoId>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncToLook {
    photo_id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasteSettings {
    from: PhotoId,
    photo_ids: Vec<PhotoId>,
    groups: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveLook {
    shoot_id: ShootId,
    look_id: String,
}
