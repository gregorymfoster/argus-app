//! Recipe tools: read, write and edit a Photo's Recipe, and render it.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "read_recipe",
            description: "Read a Photo's current Recipe (its JSON edit settings) and Version, or null if it was never edited.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "write_recipe",
            description: "Save a whole Recipe as a new Version on one or many Photos, as one undoable batch: {\"schema\":1, \"global\":{sliders}, \"look\":null, \"correction\":{...}, \"local\":[]}, sliders as in edit_recipe. Prefer edit_recipe to change a few settings. Originals are never touched.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "recipe"], "properties": {
                "photo_ids": ids, "recipe": { "type": "object" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "edit_recipe",
            description: EDIT_RECIPE_DOC,
            input_schema: json!({ "type": "object", "required": ["photo_ids", "set"], "properties": {
                "photo_ids": ids,
                "set": { "type": "object", "additionalProperties": { "type": ["number", "array"] },
                    "description": "Setting path -> value, e.g. {\"exposure\": 0.3, \"temp\": -10, \"hsl.blue.sat\": -20, \"clarity\": 15}; a list only for curve.<rgb|red|green|blue> and pointColor" },
                "relative": { "type": "boolean", "description": "Add to the current values instead of replacing them (default false)" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "render_photo",
            description: "See a Photo with its edits: renders it from the original with its current Recipe (or the `recipe` you pass, without saving it) and returns a JPEG (long edge 1024 by default, at most 1536) and `light`, the render measured (luma percentiles, clipped and crushed percent, zones, mid-tone colour cast): trust those numbers over your eye for brightness and clipping. About a second for a RAW.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id,
                "long_edge": { "type": "integer", "minimum": 64, "maximum": MAX_PREVIEW_EDGE },
                "recipe": { "type": "object", "description": "Try a Recipe without saving it" },
                "preset": { "type": "string", "description": "Try a Preset (list_presets) on top, without saving it" },
                "amount": { "type": "number", "minimum": 0, "maximum": 200, "description": "The Preset's Amount, percent (default 100)" } } }),
            read_only: true,
        },
    ]
}

impl Tools {
    pub(super) fn call_edit(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "read_recipe" => {
                let a: PhotoArg = parse(args)?;
                find_photo(&lib, a.photo_id)?;
                one(lib.read_recipe(a.photo_id)?)
            }
            "write_recipe" => {
                let a: WriteRecipe = parse(args)?;
                if !a.recipe.is_object() {
                    return Err(Error("recipe must be a JSON object".into()));
                }
                let mut recipe = Recipe::from_value(&a.recipe)?;
                if let Some(k) = recipe.other.keys().next() {
                    return Err(Error(format!(
                        "unknown Recipe field {k:?}: sliders go under \"global\" (e.g. {{\"global\": {{\"exposure\": 0.3}}}}); top-level fields are schema, global, look, correction and local"
                    )));
                }
                recipe.global = recipe.global.clamped();
                for &id in &a.photo_ids {
                    find_photo(&lib, id)?;
                }
                let batch = batch_or(&mut lib, batch, "write_recipe")?;
                let mut versions = BTreeMap::new();
                for &id in &a.photo_ids {
                    versions.insert(
                        id,
                        lib.write_recipe(id, &recipe.to_value(), Author::Ai, Some(batch))?,
                    );
                }
                one(json!({ "batch_id": batch, "versions": versions }))
            }
            "edit_recipe" => {
                let a: EditRecipe = parse(args)?;
                if a.set.is_empty() {
                    return Err(Error("set at least one setting".into()));
                }
                let mut next = Vec::new();
                for &id in &a.photo_ids {
                    find_photo(&lib, id)?;
                    let mut r = current_recipe(&lib, id)?;
                    for (path, v) in &a.set {
                        r.set_value(path, v, a.relative)?;
                    }
                    next.push((id, r));
                }
                let batch = batch_or(&mut lib, batch, "edit_recipe")?;
                let mut out = BTreeMap::new();
                for (id, r) in next {
                    let version = lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
                    // The global sliders, plus any other part of the Recipe this call
                    // touched (lensBlur, crop, ...), so the reply shows what changed.
                    let v = r.to_value();
                    let mut reply = json!({ "version": version, "global": r.global });
                    for path in a.set.keys() {
                        let head = path.split('.').next().unwrap_or(path);
                        if head != "global" && head != "version" {
                            if let Some(part) = v.get(head) {
                                reply[head] = part.clone();
                            }
                        }
                    }
                    out.insert(id, reply);
                }
                one(json!({ "batch_id": batch, "photos": out }))
            }
            "render_photo" => render_parts(&lib, parse(args)?, MAX_PREVIEW_EDGE),
            other => Err(unknown_tool(other)),
        }
    }
}

const EDIT_RECIPE_DOC: &str = "Change a few edit settings on one or many Photos as one undoable batch, keeping the rest of each Recipe. \
Settings follow Lightroom's Edit panel, its names and units; 0 means unchanged unless a default is given. \
LIGHT: exposure in stops (-5..5); contrast, highlights, shadows, whites, blacks -100..100. \
TONE CURVE: curve.<highlights|lights|darks|shadows> -100..100 bend one tonal region (lights +20 brightens upper mids, shadows -20 deepens the low end); \
curve.<splitShadows|splitMidtones|splitHighlights> 0..100 move the region borders (defaults 25, 50, 75); \
curve.rgb, curve.red, curve.green, curve.blue: point curves as a list [[in, out], ...] on 0..255 (e.g. a fade: [[0, 20], [255, 255]]; [] resets); \
curve.refineSat 0..100 (default 100) keeps that much of the curves' saturation change. \
COLOR: temp (+ warmer, - cooler) and tint (+ magenta, - green) -100..100, relative to as shot (white_balance sets them from a preset or a grey point); \
vibrance (muted colours, spares skin) and saturation -100..100 (saturation -100 is black and white). \
COLOR MIXER: hsl.<red|orange|yellow|green|aqua|blue|purple|magenta>.<hue|sat|lum> -100..100 (skin lives in orange; skies in blue and aqua). \
POINT COLOR: pointColor, a list of {hue 0..360, sat 0..100, lum 0..100 (the colour to target, as sampled), hueShift, satShift, lumShift -100..100, range 0..100 (default 50)} for one exact colour. \
COLOR GRADING: grading.<shadows|midtones|highlights|global>.hue 0..360 (0 red, 40 orange, 60 yellow, 120 green, 200 teal, 220 blue, 280 purple), .sat 0..100 (10-30 is subtle), .lum -100..100; \
grading.blending 0..100 (default 50, how much the ranges overlap), grading.balance -100..100 (+ gives highlights more of the range). Teal shadows + orange highlights is the classic film grade. \
EFFECTS: texture -100..100 (fine detail: + crisper, - smoother skin); clarity -100..100 (midtone local contrast: + punchy, - soft glow); dehaze -100..100 (+ cuts haze and adds depth, - adds atmosphere); \
vignette -100..100 (post-crop: - darker corners, + lighter) with vignetteMidpoint 0..100 (default 50, higher = smaller), vignetteRoundness -100..100, vignetteFeather 0..100 (default 50), vignetteHighlights 0..100 (protects bright corners); \
grain 0..100 with grainSize 0..100 (default 25) and grainRoughness 0..100 (default 50). \
DETAIL: sharpen 0..150 (Lightroom applies 40 to RAWs), sharpenRadius 0.5..3 px (default 1), sharpenDetail 0..100 (default 25), sharpenMasking 0..100 (higher = edges only); \
noise 0..100 luminance noise reduction with noiseDetail (default 50) and noiseContrast; colorNoise 0..100 with colorNoiseDetail and colorNoiseSmoothness (default 50). Detail shows at full size more than in a small render. \
denoise 0..100 is AI Denoise (RAW only, like Lightroom's Denoise; 0 = off, 50 is a good start for high-ISO indoor and night shots, higher looks smoother and waxier): far cleaner than noise, which it replaces. The first time it runs the model over the whole frame as a background job (minutes); renders show it once that finishes. \
OPTICS: lensCorrections \"camera\" (default: the corrections the camera made to its own JPEG, from the lens data Sony writes into each ARW), \"on\" (distortion, vignetting and chromatic aberration from that data) or \"off\"; removeCA 1 removes lateral chromatic aberration; defringePurple, defringeGreen 0..20 take colour out of fringes at strong edges; distortion -100..100 (+ straightens barrel); lensVignette -100..100 (+ brightens dark lens corners) with lensVignetteMidpoint (default 50). \
CALIBRATION: calibration.shadowsTint -100..100 (+ magenta); calibration.<red|green|blue>.<hue|sat> -100..100 shift the camera primaries (blue sat +30 is a popular richer-colour trick). \
LENS BLUR (Lightroom's, from a depth model; nothing is generated): lensBlur.amount 0..100 (0 = off; 40-60 reads as a fast prime, 100 is very shallow), lensBlur.boost 0..100 (default 50, how much out-of-focus highlights bloom), lensBlur.bokeh \"circle\" | \"bubble\" | \"blade\" | \"ring\", lensBlur.focus [from, to] on depth 0 (farthest) .. 100 (nearest), or null to focus on the subject (the default); lensBlur.focusBrush and lensBlur.blurBrush take brush dots [{x, y, r}] to keep areas sharp or blur them. view_depth shows the depth map, the focal range and the range around a point. \
RETOUCH: skin (natural smoothing of every person's skin), eyes (brighter, crisper eyes) and teeth (whiter teeth), 0..100, found by local models. \
correction.exposure (stops) and correction.wbK (Kelvin) for the per-Photo baseline under a Look, and correction.face (stops of light on faces). \
crop.angle straightens in degrees (-45..45, + turns clockwise); crop.x, crop.y, crop.w, crop.h crop as fractions 0..1 of the \
straightened frame (never shows empty corners). \
For changes to one region (the sky, a face, a drawn area) use add_mask. Check the result with render_photo.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRecipe {
    photo_ids: Vec<PhotoId>,
    set: BTreeMap<String, Value>,
    #[serde(default)]
    relative: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteRecipe {
    photo_ids: Vec<PhotoId>,
    recipe: Value,
}
