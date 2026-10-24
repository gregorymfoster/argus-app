//! Photo analysis, Mask and Spot (Heal and Clone) tools.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "analyze_photo",
            description: "What the local specialist models see in a Photo: its faces (numbered from 0, left to right, with boxes as 0..1 of the image, and per face whether the eyes are closed and how sharp they are: focus about 1 = as sharp as anything in the frame, under 0.25 = soft), how much of the frame is people, skin, hair and sky, and its sharpness, brightness and clipping. Use it before targeting one face, checking a Photo has sky, or judging focus and blinks. A second or two the first time, then cached.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "add_mask",
            description: ADD_MASK_DOC,
            input_schema: json!({ "type": "object", "required": ["photo_ids", "mask", "adjust"], "properties": {
                "photo_ids": ids,
                "mask": { "type": "object", "description": "e.g. {\"kind\": \"sky\"}, {\"kind\": \"faces\", \"face\": 0}, {\"kind\": \"radial\", \"x\": 0.5, \"y\": 0.4, \"rx\": 0.2, \"ry\": 0.3}" },
                "adjust": { "type": "object", "additionalProperties": { "type": "number" },
                    "description": "Slider deltas inside the Mask, same paths and units as edit_recipe, e.g. {\"exposure\": -0.3, \"highlights\": -20}" },
                "name": { "type": "string", "description": "Shown to the person, e.g. \"Sky\" or \"Her face\"" },
                "amount": { "type": "number", "description": "Lightroom's Mask Amount, 0..200 percent (default 100): scales the whole adjustment" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "edit_mask",
            description: "Change one local adjustment: its sliders (set, same paths as edit_recipe; relative adds), its Amount (0..200, Lightroom's), its Mask, or its name. Ids are in read_recipe's local list.",
            input_schema: json!({ "type": "object", "required": ["photo_id", "local_id"], "properties": {
                "photo_id": id, "local_id": { "type": "string" },
                "set": { "type": "object", "additionalProperties": { "type": "number" } },
                "relative": { "type": "boolean" },
                "mask": { "type": "object", "description": "Replaces the Mask" },
                "name": { "type": "string" },
                "amount": { "type": "number", "description": "0..200 percent, 100 is the sliders as set" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "remove_mask",
            description: "Remove one local adjustment (and its Mask) from a Photo's Recipe, as a new undoable Version.",
            input_schema: json!({ "type": "object", "required": ["photo_id", "local_id"], "properties": {
                "photo_id": id, "local_id": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "view_mask",
            description: "See where a Mask lands before or after using it: the Photo rendered with its Recipe and the Mask tinted red, plus the share of the frame it covers. Pass a saved local_id or a mask to try.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id,
                "local_id": { "type": "string" },
                "mask": { "type": "object" },
                "long_edge": { "type": "integer", "minimum": 64, "maximum": MAX_PREVIEW_EDGE },
                "as": { "type": "string", "enum": ["overlay", "weights"],
                    "description": "overlay (default): tinted over the render; weights: the Mask alone as greyscale, white = full" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "view_depth",
            description: "Lens Blur's depth map for a Photo (computed the first time, a few seconds): an image from cool (far) to warm (near) with what's outside the focal range dimmed, the focal range lensBlur uses now (0 far .. 100 near), a histogram of depths far to near, and with x, y the focal range around that point (Lightroom's point focus) to set as lensBlur.focus.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id,
                "x": { "type": "number", "minimum": 0, "maximum": 1, "description": "A point to focus on, 0..1 of the uncropped upright width" },
                "y": { "type": "number", "minimum": 0, "maximum": 1 },
                "long_edge": { "type": "integer", "minimum": 64, "maximum": MAX_PREVIEW_EDGE } } }),
            read_only: true,
        },
        ToolSpec {
            name: "add_spot",
            description: ADD_SPOT_DOC,
            input_schema: json!({ "type": "object", "required": ["photo_ids", "x", "y", "r"], "properties": {
                "photo_ids": ids,
                "x": { "type": "number", "minimum": 0, "maximum": 1, "description": "Centre, 0..1 of the uncropped upright image's width" },
                "y": { "type": "number", "minimum": 0, "maximum": 1, "description": "Centre, 0..1 of its height" },
                "r": { "type": "number", "minimum": heal::MIN_R, "maximum": heal::MAX_R, "description": "Radius as a fraction of the long edge (0.01 is a blemish on a half-body portrait)" },
                "mode": { "type": "string", "enum": ["heal", "clone"], "description": "heal (default) blends into the surroundings; clone copies exactly" },
                "from": { "type": "object", "description": "Source centre {x, y}; omit to let Argus pick one nearby" },
                "feather": { "type": "number", "minimum": 0, "maximum": 100, "description": "Soft edge (default 50)" },
                "opacity": { "type": "number", "minimum": 0, "maximum": 100, "description": "Default 100" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "edit_spot",
            description: "Change one Heal or Clone Spot: move it (x, y), resize it (r), move its source (from {x, y}) or pick a different source (repick), switch mode, or change feather and opacity. Ids are in read_recipe's spots list.",
            input_schema: json!({ "type": "object", "required": ["photo_id", "spot_id"], "properties": {
                "photo_id": id, "spot_id": { "type": "string" },
                "x": { "type": "number" }, "y": { "type": "number" }, "r": { "type": "number" },
                "from": { "type": "object" }, "repick": { "type": "boolean" },
                "mode": { "type": "string", "enum": ["heal", "clone"] },
                "feather": { "type": "number" }, "opacity": { "type": "number" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "remove_spot",
            description: "Remove one Heal or Clone Spot by id, or every Spot with all: true, as a new undoable Version.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id, "spot_id": { "type": "string" }, "all": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "pick_spot_source",
            description: "Where Argus would copy from for a Spot at x, y with radius r: a nearby patch of the Photo (rendered with recipe, or its saved Recipe) that continues the spot's surroundings. Changes nothing.",
            input_schema: json!({ "type": "object", "required": ["photo_id", "x", "y", "r"], "properties": {
                "photo_id": id, "x": { "type": "number" }, "y": { "type": "number" }, "r": { "type": "number" },
                "recipe": { "type": "object", "description": "A draft Recipe to pick against instead of the saved one" },
                "avoid": { "type": "array", "items": { "type": "object" }, "description": "Sources {x, y} to skip, for a different pick" } } }),
            read_only: true,
        },
    ]
}

impl Tools {
    pub(super) fn call_masks(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "analyze_photo" => {
                let a: PhotoArg = parse(args)?;
                analyze_photo(&lib, &find_photo(&lib, a.photo_id)?)
            }
            "add_mask" => add_mask(&mut lib, parse(args)?, batch),
            "edit_mask" => edit_mask(&mut lib, parse(args)?, batch),
            "remove_mask" => {
                let a: LocalArg = parse(args)?;
                find_photo(&lib, a.photo_id)?;
                let mut r = current_recipe(&lib, a.photo_id)?;
                r.local_mut(&a.local_id)?;
                r.local.retain(|l| l.id != a.local_id);
                let batch = batch_or(&mut lib, batch, "remove_mask")?;
                let version =
                    lib.write_recipe(a.photo_id, &r.to_value(), Author::Ai, Some(batch))?;
                one(json!({ "batch_id": batch, "version": version, "local": r.local }))
            }
            "view_mask" => view_mask(&lib, parse(args)?),
            "view_depth" => view_depth(&lib, parse(args)?),
            "add_spot" => add_spot(&mut lib, parse(args)?, batch),
            "edit_spot" => edit_spot(&mut lib, parse(args)?, batch),
            "remove_spot" => remove_spot(&mut lib, parse(args)?, batch),
            "pick_spot_source" => {
                let a: PickSpotSource = parse(args)?;
                let photo = find_photo(&lib, a.photo_id)?;
                let recipe = match &a.recipe {
                    Some(v) => Recipe::from_value(v)?,
                    None => current_recipe(&lib, a.photo_id)?,
                };
                let from = spot_source(&lib, &photo, &recipe, (a.x, a.y, a.r), &a.avoid)?;
                one(json!({ "from": from }))
            }
            other => Err(unknown_tool(other)),
        }
    }
}

const MASK_DOC: &str = "Mask kinds (JSON, \"kind\" plus fields): \
subject (every person), background (all but the people), faces (face skin; \"face\": n picks one face, numbered from 0 left to right as analyze_photo lists them), \
skin (face and body skin), hair, sky; \
person {person?: n, parts: [face_skin|body_skin|eyes|teeth|hair|whole]} (Lightroom's Select People: those parts of person n, or of everyone); \
brush {dots: [{x, y, r}], erase?: [{x, y, r}]}, linear {from: {x, y}, to: {x, y}} (full at from, none at to), \
radial {x, y, rx, ry, feather? 0..1 (default 0.5)}, \
luminance {lo, hi, smooth?} (tones of the render between lo and hi, 0..1; smooth 0..1 softens the edges, default 0.25), \
color {points: [{x, y}], range? 0..1 (default 0.5)} (colours like the render's at those points, Lightroom's Color Range). \
Add \"invert\": true to flip any Mask. Combine Masks with \"components\": [{\"op\": \"add\"|\"subtract\"|\"intersect\", \"kind\": ...}], \
e.g. {\"kind\": \"sky\", \"components\": [{\"op\": \"intersect\", \"kind\": \"luminance\", \"lo\": 0.6, \"hi\": 1}]} for only the bright sky, \
or {\"kind\": \"subject\", \"components\": [{\"op\": \"subtract\", \"kind\": \"faces\"}]} for clothes and hair but not faces. x and y are 0..1 of the upright image's width and height; rx is a fraction of the width, ry of the height, a dot's r of the long edge. \
AI kinds come from local models on this Mac and follow the image's edges.";

const ADD_SPOT_DOC: &str = "Lightroom's Heal and Clone: cover a small circle (a blemish, a stray hair, sensor dust, a speck in the sky) \
with pixels copied from elsewhere in the same Photo. Nothing is generated. heal keeps the source's texture and matches the colour and light around the spot; \
clone copies it exactly (for edges and patterns). Argus picks a nearby source unless from is given. \
Several photo_ids put the same Spot on each, with its own source (sensor dust in the same place on every frame). \
Coordinates are on the uncropped upright image, like Masks. One undoable batch; check it with render_photo.";

const ADD_MASK_DOC: &str = "Add a local adjustment to one or many Photos as one undoable batch: slider deltas that apply only inside a Mask, \
added to the global sliders (e.g. sky {\"exposure\": -0.4, \"highlights\": -30, \"dehaze\": 20}; a face {\"exposure\": 0.3, \"skin\": 30, \"texture\": -15}). \
Light, colour, curve, grading and retouch settings work in a Mask, plus texture, clarity, dehaze, sharpen, noise, colorNoise and defringe. \
Returns each Photo's new local id for edit_mask and remove_mask. Check it with view_mask or render_photo.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalArg {
    photo_id: PhotoId,
    local_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddSpot {
    photo_ids: Vec<PhotoId>,
    x: f64,
    y: f64,
    r: f64,
    mode: Option<heal::SpotMode>,
    from: Option<heal::Point>,
    feather: Option<f64>,
    opacity: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditSpot {
    photo_id: PhotoId,
    spot_id: String,
    x: Option<f64>,
    y: Option<f64>,
    r: Option<f64>,
    from: Option<heal::Point>,
    #[serde(default)]
    repick: bool,
    mode: Option<heal::SpotMode>,
    feather: Option<f64>,
    opacity: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveSpot {
    photo_id: PhotoId,
    spot_id: Option<String>,
    #[serde(default)]
    all: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PickSpotSource {
    photo_id: PhotoId,
    x: f64,
    y: f64,
    r: f64,
    recipe: Option<Value>,
    #[serde(default)]
    avoid: Vec<heal::Point>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddMask {
    photo_ids: Vec<PhotoId>,
    mask: Value,
    adjust: BTreeMap<String, f64>,
    name: Option<String>,
    amount: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditMask {
    photo_id: PhotoId,
    local_id: String,
    #[serde(default)]
    set: BTreeMap<String, f64>,
    #[serde(default)]
    relative: bool,
    mask: Option<Value>,
    name: Option<String>,
    amount: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewMask {
    photo_id: PhotoId,
    local_id: Option<String>,
    mask: Option<Value>,
    long_edge: Option<u32>,
    #[serde(rename = "as", default)]
    view: MaskView,
}

#[derive(Deserialize)]
struct ViewDepth {
    photo_id: PhotoId,
    x: Option<f64>,
    y: Option<f64>,
    long_edge: Option<u32>,
}

#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum MaskView {
    #[default]
    Overlay,
    Weights,
}

fn parse_mask(v: &Value) -> Result<Mask> {
    serde_json::from_value(v.clone()).map_err(|e| Error(format!("bad mask: {e}. {MASK_DOC}")))
}

/// Refuse a `faces` Mask that names a face the Photo doesn't have.
fn check_mask(lib: &Library, photo: &Photo, mask: &Mask) -> Result<()> {
    for m in mask.all() {
        check_one_mask(lib, photo, m)?;
    }
    Ok(())
}

fn check_one_mask(lib: &Library, photo: &Photo, m: &Mask) -> Result<()> {
    if let Shape::Faces { face: Some(i) }
    | Shape::Person {
        person: Some(i), ..
    } = m.shape
    {
        let n = render::analysis(lib, photo)?.faces.len();
        if i >= n {
            return Err(Error(format!(
                "Photo {} has {n} face(s), numbered from 0 left to right; there is no face {i}",
                photo.id
            )));
        }
    }
    Ok(())
}

/// A Mask Amount as stored: clamped to 0..200, and absent at the default 100.
fn mask_amount(a: Option<f64>) -> Option<f64> {
    a.filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 200.0))
        .filter(|v| (v - 100.0).abs() > 1e-9)
}

/// Slider deltas from edit_recipe-style paths (no `correction.` paths in a local).
fn local_adjust(
    base: &crate::recipe::Adjust,
    set: &BTreeMap<String, f64>,
    relative: bool,
) -> Result<crate::recipe::Adjust> {
    let mut tmp = Recipe {
        global: base.clone(),
        ..Default::default()
    };
    for (path, &v) in set {
        if path.starts_with("correction.") {
            return Err(Error(format!(
                "{path}: a local adjustment has sliders only, not the Correction"
            )));
        }
        tmp.set(path, v, relative)?;
    }
    Ok(tmp.global)
}

fn default_name(m: &Mask) -> String {
    let n = match m.shape {
        Shape::Faces { face: Some(i) } => return format!("Face {}", i + 1),
        Shape::Faces { face: None } => "Faces",
        Shape::Subject => "Subject",
        Shape::Background => "Background",
        Shape::Skin => "Skin",
        Shape::Hair => "Hair",
        Shape::Sky => "Sky",
        Shape::Person {
            person: Some(i), ..
        } => return format!("Person {}", i + 1),
        Shape::Person { person: None, .. } => "People",
        Shape::Brush { .. } => "Brush",
        Shape::Linear { .. } | Shape::Gradient { .. } => "Gradient",
        Shape::Radial { .. } => "Radial",
        Shape::Luminance { .. } => "Luminance range",
        Shape::Color { .. } => "Color range",
    };
    if m.invert {
        format!("Not {}", n.to_lowercase())
    } else {
        n.into()
    }
}

/// A local id not yet used in this Recipe, e.g. `sky-2`.
fn new_local_id(r: &Recipe, kind: &str) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                kind.to_string()
            } else {
                format!("{kind}-{n}")
            }
        })
        .find(|id| r.local.iter().all(|l| &l.id != id))
        .expect("an unused id")
}

fn add_mask(lib: &mut Library, a: AddMask, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let mask = parse_mask(&a.mask)?;
    let adjust = local_adjust(&Default::default(), &a.adjust, false)?;
    let mut next = Vec::new();
    for &id in &a.photo_ids {
        let photo = find_photo(lib, id)?;
        check_mask(lib, &photo, &mask)?;
        let mut r = current_recipe(lib, id)?;
        let local_id = new_local_id(&r, mask.kind());
        r.local.push(Local {
            id: local_id.clone(),
            name: a.name.clone().unwrap_or_else(|| default_name(&mask)),
            mask: mask.clone(),
            adjust: adjust.clone(),
            amount: mask_amount(a.amount),
            source: Some("ai".into()),
            other: Default::default(),
        });
        next.push((id, local_id, r));
    }
    let batch = batch_or(lib, batch, "add_mask")?;
    let mut out = BTreeMap::new();
    for (id, local_id, r) in next {
        let version = lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
        out.insert(id, json!({ "version": version, "local_id": local_id }));
    }
    one(json!({ "batch_id": batch, "photos": out }))
}

fn edit_mask(lib: &mut Library, a: EditMask, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let photo = find_photo(lib, a.photo_id)?;
    let mut r = current_recipe(lib, a.photo_id)?;
    let l = r.local_mut(&a.local_id)?;
    if let Some(m) = &a.mask {
        let m = parse_mask(m)?;
        check_mask(lib, &photo, &m)?;
        l.mask = m;
    }
    if let Some(n) = a.name {
        l.name = n;
    }
    if a.amount.is_some() {
        l.amount = mask_amount(a.amount);
    }
    l.adjust = local_adjust(&l.adjust, &a.set, a.relative)?;
    let local = l.clone();
    let batch = batch_or(lib, batch, "edit_mask")?;
    let version = lib.write_recipe(a.photo_id, &r.to_value(), Author::Ai, Some(batch))?;
    one(json!({ "batch_id": batch, "version": version, "local": local }))
}

/// Long edge of the render a Spot's source is picked on: plenty for a nearby match, and
/// quick from the Edit screen's cache of demosaiced RAWs.
const SPOT_PICK_EDGE: usize = 1024;

/// Where a Spot at `x`, `y`, `r` should copy from, judged on the Photo rendered with
/// `recipe` uncropped (Spots live on the whole frame, earlier Spots included).
fn spot_source(
    lib: &Library,
    photo: &Photo,
    recipe: &Recipe,
    (x, y, r): (f64, f64, f64),
    avoid: &[heal::Point],
) -> Result<heal::Point> {
    let whole = Recipe {
        crop: Default::default(),
        ..recipe.clone()
    };
    let img = render::render_in(lib, photo, &whole, Some(SPOT_PICK_EDGE))?;
    Ok(heal::find_source_except(&img, x, y, r, avoid))
}

/// A Spot id not yet used in this Recipe: `spot`, `spot-2`, ...
fn new_spot_id(r: &Recipe) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                "spot".to_string()
            } else {
                format!("spot-{n}")
            }
        })
        .find(|id| r.spots.iter().all(|s| &s.id != id))
        .expect("an unused id")
}

fn spot_mut<'a>(r: &'a mut Recipe, id: &str) -> Result<&'a mut heal::Spot> {
    let ids: Vec<String> = r.spots.iter().map(|s| s.id.clone()).collect();
    r.spots
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| Error(format!("no Spot {id:?}; this Recipe has {ids:?}")))
}

fn check_point(what: &str, x: f64, y: f64) -> Result<()> {
    if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return Err(Error(format!(
            "{what} ({x}, {y}) is outside the Photo: x and y are 0..1 of the upright image"
        )));
    }
    Ok(())
}

fn add_spot(lib: &mut Library, a: AddSpot, batch: Option<BatchId>) -> Result<Vec<Part>> {
    check_point("the Spot", a.x, a.y)?;
    if let Some(f) = a.from {
        check_point("from", f.x, f.y)?;
    }
    if !(heal::MIN_R..=heal::MAX_R).contains(&a.r) {
        return Err(Error(format!(
            "r {} is out of range {}..{} (a fraction of the long edge); Spots are for small things, not object removal",
            a.r,
            heal::MIN_R,
            heal::MAX_R
        )));
    }
    let mut next = Vec::new();
    for &id in &a.photo_ids {
        let photo = find_photo(lib, id)?;
        let mut r = current_recipe(lib, id)?;
        let from = match a.from {
            Some(f) => f,
            None => spot_source(lib, &photo, &r, (a.x, a.y, a.r), &[])?,
        };
        let spot = heal::Spot {
            id: new_spot_id(&r),
            mode: a.mode.unwrap_or_default(),
            x: a.x,
            y: a.y,
            r: a.r,
            from,
            feather: a.feather.unwrap_or(heal::DEFAULT_FEATHER),
            opacity: a.opacity.unwrap_or(100.0),
            source: Some("ai".into()),
        }
        .clamped();
        r.spots.push(spot.clone());
        next.push((id, spot, r));
    }
    let batch = batch_or(lib, batch, "add_spot")?;
    let mut out = BTreeMap::new();
    for (id, spot, r) in next {
        let version = lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
        out.insert(id, json!({ "version": version, "spot": spot }));
    }
    one(json!({ "batch_id": batch, "photos": out }))
}

fn edit_spot(lib: &mut Library, a: EditSpot, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let photo = find_photo(lib, a.photo_id)?;
    let mut r = current_recipe(lib, a.photo_id)?;
    let s = spot_mut(&mut r, &a.spot_id)?;
    s.x = a.x.unwrap_or(s.x);
    s.y = a.y.unwrap_or(s.y);
    s.r = a.r.unwrap_or(s.r);
    s.mode = a.mode.unwrap_or(s.mode);
    s.feather = a.feather.unwrap_or(s.feather);
    s.opacity = a.opacity.unwrap_or(s.opacity);
    if let Some(f) = a.from {
        s.from = f;
    }
    check_point("the Spot", s.x, s.y)?;
    check_point("from", s.from.x, s.from.y)?;
    *s = s.clone().clamped();
    if a.repick {
        let (x, y, rad, id, was) = (s.x, s.y, s.r, s.id.clone(), s.from);
        // Pick against everything but this Spot, so it doesn't copy its own patch.
        let without = Recipe {
            spots: r.spots.iter().filter(|s| s.id != id).cloned().collect(),
            ..r.clone()
        };
        let from = spot_source(lib, &photo, &without, (x, y, rad), &[was])?;
        spot_mut(&mut r, &id)?.from = from;
    }
    let spot = spot_mut(&mut r, &a.spot_id)?.clone();
    let batch = batch_or(lib, batch, "edit_spot")?;
    let version = lib.write_recipe(a.photo_id, &r.to_value(), Author::Ai, Some(batch))?;
    one(json!({ "batch_id": batch, "version": version, "spot": spot }))
}

fn remove_spot(lib: &mut Library, a: RemoveSpot, batch: Option<BatchId>) -> Result<Vec<Part>> {
    find_photo(lib, a.photo_id)?;
    let mut r = current_recipe(lib, a.photo_id)?;
    match (&a.spot_id, a.all) {
        (Some(id), false) => {
            spot_mut(&mut r, id)?;
            r.spots.retain(|s| &s.id != id);
        }
        (None, true) => r.spots.clear(),
        _ => return Err(Error("pass either spot_id or all: true".into())),
    }
    let batch = batch_or(lib, batch, "remove_spot")?;
    let version = lib.write_recipe(a.photo_id, &r.to_value(), Author::Ai, Some(batch))?;
    one(json!({ "batch_id": batch, "version": version, "spots": r.spots }))
}

fn mean(p: &[f32]) -> f64 {
    let s: f64 = p.par_iter().map(|&v| v as f64).sum();
    (s / p.len().max(1) as f64 * 1000.0).round() / 1000.0
}

fn analyze_photo(lib: &Library, photo: &Photo) -> Result<Vec<Part>> {
    use crate::analyzers::{self, eyes, faces, signals};
    let ids: Vec<&str> = analyzers::REGISTRY
        .iter()
        .filter(|a| a.available() && !a.on_demand())
        .map(|a| a.id())
        .collect();
    let facts = analyzers::facts(lib, photo, &ids)?;
    let r3 = |v: f32| (v as f64 * 1000.0).round() / 1000.0;
    let reads = facts.get("eyes").map(eyes::read).unwrap_or_default();
    let face_list: Vec<Value> = facts
        .get("faces")
        .map(faces::read)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut v = json!({ "face": i, "x": r3(f.x), "y": r3(f.y), "w": r3(f.w), "h": r3(f.h), "score": r3(f.score) });
            if let Some(e) = reads.iter().find(|e| e.face == i && e.read) {
                v["eyes_closed"] = json!(e.eyes_closed);
                v["blink"] = json!(e.blink);
                v["focus"] = json!(e.focus);
            }
            v
        })
        .collect();
    let seg = &facts["segment"].value["coverage"];
    let num = |v: &Value| v.as_f64().unwrap_or(0.0);
    let people = 1.0 - num(&seg["background"]);
    let skin = num(&seg["face_skin"]) + num(&seg["body_skin"]);
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    one(json!({
        "photo_id": photo.id,
        "faces": face_list,
        "coverage": { "people": round(people), "skin": round(skin),
                      "hair": seg["hair"], "sky": facts["sky"].value["coverage"] },
        "signals": facts.get("signals").map(signals::read),
        "summary": analyzers::caption(&facts),
    }))
}

fn view_depth(lib: &Library, a: ViewDepth) -> Result<Vec<Part>> {
    use crate::lens_blur;
    let photo = find_photo(lib, a.photo_id)?;
    let recipe = current_recipe(lib, a.photo_id)?;
    let analysis = render::analysis_with(lib, &photo, true)?;
    let depth = analysis
        .depth
        .as_ref()
        .ok_or_else(|| Error(format!("no depth map for Photo {}", photo.id)))?;
    let n = crate::masks::models::SEG_SIZE;
    let subject: Vec<f32> = analysis
        .class("background")
        .iter()
        .map(|v| 1.0 - v)
        .collect();
    let subject = crate::masks::refine::resize(&subject, n, n, depth.width, depth.height);
    let range = lens_blur::focus_range(&recipe.lens_blur, depth, Some(&subject));
    // The depth on the whole frame, its edges snapped to the photo's as a render does.
    let edge = a.long_edge.unwrap_or(1024).clamp(64, MAX_PREVIEW_EDGE) as usize;
    let whole = Recipe {
        crop: Default::default(),
        local: Vec::new(),
        lens_blur: Default::default(),
        ..recipe.clone()
    };
    let img = render::render_in(lib, &photo, &whole, Some(edge))?;
    let guide: Vec<f32> = img
        .data
        .chunks(3)
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0)
        .collect();
    let full = lens_blur::depth_at_size(depth, &guide, img.width, img.height);
    let view = crate::RgbImage {
        width: img.width,
        height: img.height,
        data: lens_blur::visualize(&full, range),
    };
    let pct = |v: f32| (v as f64 * 1000.0).round() / 10.0;
    let mut out = json!({
        "photo_id": photo.id,
        "focus": [pct(range[0]), pct(range[1])],
        "subject": recipe.lens_blur.focus.is_none(),
        "histogram": depth.histogram(32).iter().map(|v| (*v as f64 * 1000.0).round() / 1000.0).collect::<Vec<_>>(),
        "width": view.width, "height": view.height,
    });
    if let (Some(x), Some(y)) = (a.x, a.y) {
        out["at"] = json!(lens_blur::focus_at(depth, x, y));
    }
    Ok(vec![
        Part::Json(out),
        Part::Jpeg(render::encode_jpeg(&view, 85)?),
    ])
}

fn view_mask(lib: &Library, a: ViewMask) -> Result<Vec<Part>> {
    let photo = find_photo(lib, a.photo_id)?;
    let recipe = current_recipe(lib, a.photo_id)?;
    let mask = match (&a.local_id, &a.mask) {
        (Some(id), None) => recipe
            .local
            .iter()
            .find(|l| &l.id == id)
            .map(|l| l.mask.clone())
            .ok_or_else(|| Error(format!("no local adjustment {id:?} on Photo {}", photo.id)))?,
        (None, Some(m)) => parse_mask(m)?,
        _ => return Err(Error("pass either local_id or mask".into())),
    };
    check_mask(lib, &photo, &mask)?;
    let edge = a.long_edge.unwrap_or(1024).clamp(64, MAX_PREVIEW_EDGE) as usize;
    // Masks live on the whole frame, so look at it uncropped and crop the view last. They
    // are drawn on the render before any local adjustment, as a render draws them, so range
    // Masks see the same tones and colours.
    let whole = Recipe {
        crop: Default::default(),
        local: Vec::new(),
        ..recipe.clone()
    };
    let img = render::render_in(lib, &photo, &whole, Some(edge))?;
    let guide: Vec<f32> = img
        .data
        .chunks(3)
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0)
        .collect();
    let analysis = if mask.needs_analysis() {
        Some(render::analysis(lib, &photo)?)
    } else {
        None
    };
    let rgb: Vec<f32> = img.data.iter().map(|&v| v as f32 / 255.0).collect();
    let canvas = Canvas {
        width: img.width,
        height: img.height,
        guide: &guide,
        rgb: Some(&rgb),
        analysis: analysis.as_deref(),
    };
    let w = canvas.plane(&mask)?;
    let out: Vec<u8> = if a.view == MaskView::Weights {
        w.iter()
            .flat_map(|&k| [(k * 255.0 + 0.5) as u8; 3])
            .collect()
    } else {
        img.data
            .chunks(3)
            .zip(&w)
            .flat_map(|(p, &k)| {
                let k = k * 0.55;
                let tint = [255.0, 40.0, 40.0];
                std::array::from_fn::<u8, 3, _>(|c| (p[c] as f32 * (1.0 - k) + tint[c] * k) as u8)
            })
            .collect()
    };
    let view = crate::geometry::apply(
        crate::RgbImage {
            width: img.width,
            height: img.height,
            data: out,
        },
        &recipe.crop,
    );
    Ok(vec![
        Part::Json(
            json!({ "photo_id": photo.id, "kind": mask.kind(), "coverage": mean(&w),
            "width": view.width, "height": view.height }),
        ),
        Part::Jpeg(render::encode_jpeg(&view, 85)?),
    ])
}
