#![allow(unknown_lints, clippy::chunks_exact_to_as_chunks)] // dev tool, clarity over lint-golf

//! `argus edit-fit`: dev tool for the edit-heuristics bench (`bench/edit_head.py`). Not on
//! the render hot path.
//!
//! Each case's RAW is decoded and demosaiced once at the size of the person's own edit, so
//! hundreds of Recipes per Photo cost a few milliseconds each. For every case it reports:
//!
//! - `features`: cheap facts about the unedited render (engine::edit_head::features, at
//!   Auto's measuring size) and the camera's as-shot white balance and model;
//! - `head`: the offset from Auto the shipped Edit head predicts (engine::edit_head);
//! - `auto`: what Auto sets (engine::auto, as the app runs it);
//! - `fit` (with `"fit": true`): the global Basic sliders that bring the render closest to
//!   the person's edit, by coordinate descent from Auto (the "oracle" a heuristic tries to
//!   predict);
//! - `scene` (with `"scene": true`): the Mask models' facts about the unedited render;
//! - `fit_local` (with `"fit_local": true`): AI-Mask settings on top of the global fit
//!   (or Auto), by the same descent on the whole frame's dE plus the faces' and the sky's;
//! - `face_scores`, `sky_scores` (with `"scene": true`): the same dE over only the blocks in
//!   face boxes or the sky, where a Photo has enough of them;
//! - `sensitivity` (with `"sensitivity": true`): how the render moves with each Basic slider
//!   at the fit (or Auto): the 10 x 10 matrix JᵀJ / blocks, J the change of every 8 x 8 Lab
//!   block per unit of each slider (FIT order), by central differences. Near the fit the
//!   squared render error of a slider guess p is about (p - fit)ᵀ M (p - fit), so a model
//!   can be trained on rendered error instead of slider distance;
//! - with `save_plain`, the unedited render at Auto's measuring size as `<dir>/<id>.jpg`;
//! - `scores`: dE76 to the edit for each named Recipe in the job (flat `{"exposure": 0.3}`
//!   settings, as `argus edit` takes them), rendered to `save_dir` when given.
//!
//! dE matches `bench/score.py` `distance()` at rot 0: centre-crop to the edit's aspect,
//! resize to its size, average 8 x 8 blocks, CIELAB dE76.

use argus_engine::edit_head::lab;
use argus_engine::recipe::{Adjust, Recipe};
use argus_engine::{decode, pipeline::Look, Demosaiced, RgbImage};
use rayon::prelude::*;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct Job {
    cases: Vec<Case>,
    #[serde(default)]
    fit: bool,
    /// Also run the Mask models on the unedited render: `scene` facts
    /// (engine::edit_head::scene_features), and `local.<kind>.<slider>` settings in Recipes
    /// render through them.
    #[serde(default)]
    scene: bool,
    /// After the global fit, the AI-Mask exposures (faces, subject, background, sky) that
    /// bring it closer still: where the person's edit differs locally (`fit_local`).
    #[serde(default)]
    fit_local: bool,
    /// Also the image embedding of the unedited render (engine::analyzers::embedding, 1280
    /// floats, unit length) as `embedding`, for learned scene types.
    #[serde(default)]
    embed: bool,
    /// Also `sensitivity`: the slider-to-render matrix at the fit (see the module docs).
    #[serde(default)]
    sensitivity: bool,
    /// Save the unedited render at Auto's measuring size here as `<id>.jpg` (for embedding
    /// models that run outside the engine).
    save_plain: Option<PathBuf>,
    /// Named Recipes per case id: `{"id": {"auto+head": {"exposure": 0.2}}}`. The name
    /// "auto" in a Recipe's settings is not special; Auto's own values come back in `auto`.
    /// A setting that isn't a number goes through `Recipe::set_value` (`lensCorrections`,
    /// point curves).
    #[serde(default)]
    recipes: BTreeMap<String, BTreeMap<String, BTreeMap<String, Value>>>,
    /// Settings every render starts from (Auto, the fit, the scores' Recipes go on top), as
    /// `recipes` takes them: e.g. `{"clarity": 20}` to ask how close the best Basic sliders
    /// get with some Clarity.
    #[serde(default)]
    base: BTreeMap<String, Value>,
    save_dir: Option<PathBuf>,
    /// Long edge of the saved renders (default: the edit's size, as scored).
    save_edge: Option<usize>,
    /// Only save these Recipe names (default: all).
    save: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    raw: PathBuf,
    target: PathBuf,
}

/// Sliders the fit moves: (name, first step, min, max).
const FIT: &[(&str, f64, f64, f64)] = &[
    ("exposure", 0.3, -2.0, 2.0),
    ("contrast", 15.0, -60.0, 60.0),
    ("highlights", 20.0, -100.0, 100.0),
    ("shadows", 20.0, -100.0, 100.0),
    ("whites", 20.0, -100.0, 100.0),
    ("blacks", 20.0, -100.0, 100.0),
    ("temp", 8.0, -60.0, 60.0),
    ("tint", 6.0, -40.0, 40.0),
    ("vibrance", 15.0, -100.0, 100.0),
    ("saturation", 15.0, -100.0, 100.0),
];

fn slot<'a>(a: &'a mut Adjust, k: &str) -> &'a mut f64 {
    match k {
        "exposure" => &mut a.exposure,
        "contrast" => &mut a.contrast,
        "highlights" => &mut a.highlights,
        "shadows" => &mut a.shadows,
        "whites" => &mut a.whites,
        "blacks" => &mut a.blacks,
        "temp" => &mut a.temp,
        "tint" => &mut a.tint,
        "vibrance" => &mut a.vibrance,
        "saturation" => &mut a.saturation,
        _ => unreachable!("{k}"),
    }
}

fn get(a: &Adjust, k: &str) -> f64 {
    *slot(&mut a.clone(), k)
}

const BLOCK: usize = 8;

struct Target {
    w: usize,
    h: usize,
    lab: Vec<[f32; 3]>,
}

/// 8 x 8 block means (in 8-bit sRGB, as score.py) of `img`, in Lab.
fn blocks(img: &image::RgbImage) -> Vec<[f32; 3]> {
    let (w, h) = (img.width() as usize / BLOCK, img.height() as usize / BLOCK);
    let mut out = Vec::with_capacity(w * h);
    for by in 0..h {
        for bx in 0..w {
            let mut s = [0f32; 3];
            for y in 0..BLOCK {
                for x in 0..BLOCK {
                    let p = img.get_pixel((bx * BLOCK + x) as u32, (by * BLOCK + y) as u32);
                    for c in 0..3 {
                        s[c] += p[c] as f32;
                    }
                }
            }
            let n = (BLOCK * BLOCK) as f32;
            out.push(lab([s[0] / n, s[1] / n, s[2] / n]));
        }
    }
    out
}

fn to_image(img: &RgbImage) -> image::RgbImage {
    image::RgbImage::from_raw(img.width as u32, img.height as u32, img.data.clone()).unwrap()
}

/// dE, dL, da, db of `img` against the target, as score.py's distance() at rot 0.
fn distance(img: &RgbImage, t: &Target) -> [f64; 4] {
    weighted_distance(img, t, None)
}

/// [`distance`] over the blocks `weight` (one per block, 0..1) picks out, such as the faces.
fn weighted_distance(img: &RgbImage, t: &Target, weight: Option<&[f32]>) -> [f64; 4] {
    let b = aligned_blocks(img, t);
    let (mut de, mut off, mut n) = (0f64, [0f64; 3], 0f64);
    for (i, (x, y)) in b.iter().zip(&t.lab).enumerate() {
        let w = weight.map_or(1.0, |w| w[i] as f64);
        let d = [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
        de += w * ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64).sqrt();
        for c in 0..3 {
            off[c] += w * d[c] as f64;
        }
        n += w;
    }
    let n = n.max(1e-9);
    [de / n, off[0] / n, off[1] / n, off[2] / n]
}

/// `img` cropped to the target's aspect, resized to its size, as Lab block means.
fn aligned_blocks(img: &RgbImage, t: &Target) -> Vec<[f32; 3]> {
    let mut im = to_image(img);
    let (ta, ia) = (
        t.w as f64 / t.h as f64,
        im.width() as f64 / im.height() as f64,
    );
    if (ia - ta).abs() >= 0.01 {
        let (w, h) = (im.width() as f64, im.height() as f64);
        let (cw, ch) = if ia > ta { (h * ta, h) } else { (w, w / ta) };
        let (x, y) = (((w - cw) / 2.0) as u32, ((h - ch) / 2.0) as u32);
        im = image::imageops::crop_imm(&im, x, y, cw as u32, ch as u32).to_image();
    }
    let im = image::imageops::resize(
        &im,
        t.w as u32,
        t.h as u32,
        image::imageops::FilterType::Lanczos3,
    );
    blocks(&im)
}

/// Per-block weights for the faces and the sky of an Analysis, on the target's block grid
/// (None when the Photo has too little of either to score).
fn region_weights(a: &argus_engine::masks::Analysis, t: &Target) -> [Option<Vec<f32>>; 2] {
    let (bw, bh) = (t.w / BLOCK, t.h / BLOCK);
    let centre = |i: usize| {
        (
            ((i % bw) as f32 + 0.5) / bw as f32,
            ((i / bw) as f32 + 0.5) / bh as f32,
        )
    };
    let faces: Vec<f32> = (0..bw * bh)
        .map(|i| {
            let (u, v) = centre(i);
            let inside = a
                .faces
                .iter()
                .any(|f| u >= f.x && u <= f.x + f.w && v >= f.y && v <= f.y + f.h);
            if inside {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    let sky: Vec<f32> = (0..bw * bh)
        .map(|i| {
            let (u, v) = centre(i);
            let x = ((u * a.sky_w as f32) as usize).min(a.sky_w - 1);
            let y = ((v * a.sky_h as f32) as usize).min(a.sky_h - 1);
            a.sky[y * a.sky_w + x]
        })
        .collect();
    let enough = |w: &[f32]| w.iter().sum::<f32>() >= 4.0;
    [
        Some(faces).filter(|w| enough(w)),
        Some(sky).filter(|w| enough(w)),
    ]
}

/// The Adjust an `adjust_json` object describes.
fn fit_adjust(v: &Value) -> Adjust {
    let mut g = Adjust::default();
    for (k, ..) in FIT {
        *slot(&mut g, k) = v[*k].as_f64().unwrap_or(0.0);
    }
    g
}

fn adjust_json(a: &Adjust) -> Value {
    Value::Object(
        FIT.iter()
            .map(|(k, ..)| {
                (
                    (*k).to_string(),
                    json!((get(a, k) * 1000.0).round() / 1000.0),
                )
            })
            .collect(),
    )
}

fn run_case(job: &Job, c: &Case) -> Result<Value, String> {
    let e = |x: &dyn std::fmt::Display| format!("{}: {x}", c.id);
    let tgt = image::open(&c.target).map_err(|x| e(&x))?.to_rgb8();
    let target = Target {
        w: tgt.width() as usize,
        h: tgt.height() as usize,
        lab: blocks(&tgt),
    };
    let raw = decode(&c.raw).map_err(|x| e(&x))?;
    let edge = target.w.max(target.h);
    let d = Demosaiced::new(&raw, edge).ok_or_else(|| e(&"RAW smaller than the edit"))?;
    let small = Demosaiced::new(&raw, argus_engine::auto::MEASURE_EDGE).unwrap();
    let mut base = Recipe::default();
    for (k, v) in &job.base {
        match v.as_f64() {
            Some(x) => argus_engine::suggest::apply_setting(&mut base, k, x, false),
            None => base.set_value(k, v, false),
        }
        .map_err(|x| e(&x))?;
    }
    let render = |d: &Demosaiced, a: &Adjust| {
        let r = Recipe {
            global: base.global.plus(a),
            ..base.clone()
        };
        d.finish(&Look::DEFAULT, &r, None)
    };
    // Plain Auto, without the Edit head: the head is what's learned on top of it.
    let plain = argus_engine::auto::Params {
        edit_head: false,
        ..argus_engine::auto::Params::current().map_err(|x| e(&x))?
    };
    let auto = argus_engine::auto::auto_adjust_with(|a| Ok(render(&small, a)), true, &plain)
        .map_err(|x| e(&x))?;
    let none_small = render(&small, &Adjust::default());
    let analysis = if job.scene || job.fit_local {
        Some(argus_engine::masks::Analysis::of(&none_small).map_err(|x| e(&x))?)
    } else {
        None
    };
    let mut out = json!({
        "id": c.id,
        "camera": format!("{} {}", raw.make, raw.model),
        "exposure": std::fs::read(&c.raw)
            .map(|b| argus_engine::library::exposure_of(&b))
            .ok(),
        "as_shot_wb": raw.wb,
        // At Auto's measuring size, as the Edit head reads them in the app.
        "features": Map::from_iter(
            argus_engine::edit_head::features(&render(&small, &Adjust::default()))
                .into_iter()
                .map(|(k, v)| (k, json!((v * 1000.0).round() / 1000.0)))
        ),
        "head": adjust_json(&argus_engine::edit_head::offset(&none_small, &auto, analysis.as_ref())),
        "auto": adjust_json(&auto),
    });
    if job.embed {
        let img = image::RgbImage::from_raw(
            none_small.width as u32,
            none_small.height as u32,
            none_small.data.clone(),
        )
        .ok_or_else(|| "render size".to_string())?;
        let v = argus_engine::analyzers::embedding::embed(&img).map_err(|x| e(&x))?;
        out["embedding"] = json!(v
            .iter()
            .map(|x| (x * 1e5).round() / 1e5)
            .collect::<Vec<_>>());
    }
    if let Some(dir) = &job.save_plain {
        to_image(&none_small)
            .save(dir.join(format!("{}.jpg", c.id)))
            .map_err(|x| e(&x))?;
    }
    if let Some(a) = &analysis {
        out["scene"] = Value::Object(Map::from_iter(
            argus_engine::edit_head::scene_features(&none_small, a)
                .into_iter()
                .map(|(k, v)| (k, json!((v * 1000.0).round() / 1000.0))),
        ));
    }
    let score = |a: &Adjust| distance(&render(&d, a), &target);
    let rnd = |v: [f64; 4]| v.map(|x| (x * 100.0).round() / 100.0);
    out["none"] = json!(rnd(score(&Adjust::default())));
    out["auto_dE"] = json!(rnd(score(&auto)));
    if job.fit {
        let mut x = auto.clone();
        let mut best = score(&x)[0];
        for round in 0..5 {
            let scale = 0.5f64.powi(round);
            for (k, step, lo, hi) in FIT {
                let step = step * scale;
                for sign in [1.0, -1.0] {
                    loop {
                        let mut y = x.clone();
                        let v = (get(&x, k) + sign * step).clamp(*lo, *hi);
                        if v == get(&x, k) {
                            break;
                        }
                        *slot(&mut y, k) = v;
                        let s = score(&y)[0];
                        if s < best - 0.005 {
                            (x, best) = (y, s);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        out["fit"] = adjust_json(&x);
        out["fit_dE"] = json!(rnd(score(&x)));
    }
    if job.sensitivity {
        let at = match out.get("fit") {
            Some(_) => fit_adjust(&out["fit"]),
            None => auto.clone(),
        };
        let cols: Vec<Vec<f32>> = FIT
            .iter()
            .map(|(k, step, lo, hi)| {
                let h = step * 0.25;
                let side = |sign: f64| {
                    let mut y = at.clone();
                    let v = (get(&at, k) + sign * h).clamp(*lo, *hi);
                    *slot(&mut y, k) = v;
                    (aligned_blocks(&render(&d, &y), &target), v)
                };
                let ((bp, vp), (bm, vm)) = (side(1.0), side(-1.0));
                let dv = (vp - vm).max(1e-9) as f32;
                bp.iter()
                    .zip(&bm)
                    .flat_map(|(p, m)| (0..3).map(move |c| (p[c] - m[c]) / dv))
                    .collect()
            })
            .collect();
        let n = (cols[0].len() / 3).max(1) as f64;
        let m: Vec<Vec<f64>> = cols
            .iter()
            .map(|a| {
                cols.iter()
                    .map(|b| a.iter().zip(b).map(|(x, y)| (x * y) as f64).sum::<f64>() / n)
                    .map(|v| (v * 1e6).round() / 1e6)
                    .collect()
            })
            .collect();
        out["sensitivity"] = json!(m);
    }
    if let (true, Some(a)) = (job.fit_local, &analysis) {
        let global = match out.get("fit") {
            Some(f) => fit_adjust(f),
            None => auto.clone(),
        };
        let c = a.coverage();
        let mut keys: Vec<(&str, f64, f64)> = Vec::new();
        if c.people >= 0.02 {
            keys.push(("local.subject.exposure", 0.2, 1.0));
            keys.push(("local.background.exposure", 0.2, 1.0));
            if c.faces > 0 {
                keys.push(("local.faces.exposure", 0.2, 1.0));
            }
        }
        if c.sky >= 0.04 {
            keys.push(("local.sky.exposure", 0.2, 1.0));
            keys.push(("local.sky.saturation", 15.0, 60.0));
        }
        let regions = region_weights(a, &target);
        let local_score = |v: &BTreeMap<&str, f64>| -> Result<f64, String> {
            let mut r = Recipe {
                global: base.global.plus(&global),
                ..base.clone()
            };
            for (k, x) in v {
                argus_engine::suggest::apply_setting(&mut r, k, *x, false).map_err(|x| e(&x))?;
            }
            // The whole frame plus faces and sky on their own: a face is a small share of
            // the frame, so the whole-frame dE alone barely sees a face tweak.
            let img = d.finish(&Look::DEFAULT, &r, Some(a));
            let mut s = distance(&img, &target)[0];
            for w in regions.iter().flatten() {
                s += weighted_distance(&img, &target, Some(w))[0];
            }
            Ok(s)
        };
        let mut x: BTreeMap<&str, f64> = keys.iter().map(|k| (k.0, 0.0)).collect();
        let mut best = local_score(&x)?;
        for round in 0..4 {
            let scale = 0.5f64.powi(round);
            for (k, step, lim) in &keys {
                for sign in [1.0, -1.0] {
                    loop {
                        let mut y = x.clone();
                        let v = (x[k] + sign * step * scale).clamp(-lim, *lim);
                        if v == x[k] {
                            break;
                        }
                        y.insert(k, v);
                        let s = local_score(&y)?;
                        if s < best - 0.005 {
                            (x, best) = (y, s);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        out["fit_local"] = json!(x
            .iter()
            .map(|(k, v)| ((*k).to_string(), json!((v * 1000.0).round() / 1000.0)))
            .collect::<Map<_, _>>());
        out["fit_local_score"] = json!((best * 100.0).round() / 100.0);
    }
    if let Some(rs) = job.recipes.get(&c.id) {
        let big = match (job.save_dir.is_some(), job.save_edge) {
            (true, Some(e)) if e > edge => Demosaiced::new(&raw, e),
            _ => None,
        };
        let mut scores = Map::new();
        let regions = analysis.as_ref().map(|a| region_weights(a, &target));
        let (mut face_scores, mut sky_scores) = (Map::new(), Map::new());
        for (name, settings) in rs {
            let mut r = Recipe::default();
            for (k, v) in settings {
                match v.as_f64() {
                    Some(x) => argus_engine::suggest::apply_setting(&mut r, k, x, false),
                    None => r.set_value(k, v, false),
                }
                .map_err(|x| e(&x))?;
            }
            let mut img = d.finish(&Look::DEFAULT, &r, analysis.as_ref());
            // Post-crop Vignette and Grain, as render_photo adds them.
            argus_engine::effects::post_crop(&mut img, &r.render_adjust());
            scores.insert(name.clone(), json!(rnd(distance(&img, &target))));
            if let Some([faces, sky]) = &regions {
                if let Some(w) = faces {
                    let d = weighted_distance(&img, &target, Some(w));
                    face_scores.insert(name.clone(), json!(rnd(d)));
                }
                if let Some(w) = sky {
                    let d = weighted_distance(&img, &target, Some(w));
                    sky_scores.insert(name.clone(), json!(rnd(d)));
                }
            }
            let wanted = job.save.as_ref().is_none_or(|s| s.contains(name));
            if let (Some(dir), true) = (&job.save_dir, wanted) {
                let img = match &big {
                    Some(b) => {
                        let mut big = b.finish(&Look::DEFAULT, &r, analysis.as_ref());
                        argus_engine::effects::post_crop(&mut big, &r.render_adjust());
                        big
                    }
                    None => img,
                };
                let path = dir.join(format!("{}_{name}.jpg", c.id));
                std::fs::write(
                    &path,
                    argus_engine::render::encode_jpeg(&img, 90).map_err(|x| e(&x))?,
                )
                .map_err(|x| e(&x))?;
            }
        }
        out["scores"] = Value::Object(scores);
        if !face_scores.is_empty() {
            out["face_scores"] = Value::Object(face_scores);
        }
        if !sky_scores.is_empty() {
            out["sky_scores"] = Value::Object(sky_scores);
        }
    }
    Ok(out)
}

/// Runs the job; one JSON object per case, in the job's order (failures carry `error`).
pub fn run(job: Job) -> Vec<Value> {
    if let Some(dir) = &job.save_dir {
        std::fs::create_dir_all(dir).ok();
    }
    let done = std::sync::atomic::AtomicUsize::new(0);
    let n = job.cases.len();
    job.cases
        .par_iter()
        .map(|c| {
            let v = run_case(&job, c).unwrap_or_else(|e| json!({"id": c.id, "error": e}));
            let k = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            eprintln!("::progress {k}/{n} edit-fit");
            v
        })
        .collect()
}
