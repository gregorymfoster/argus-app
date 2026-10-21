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
    /// Named Recipes per case id: `{"id": {"auto+head": {"exposure": 0.2}}}`. The name
    /// "auto" in a Recipe's settings is not special; Auto's own values come back in `auto`.
    #[serde(default)]
    recipes: BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>,
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
    let b = blocks(&im);
    let (mut de, mut off) = (0f64, [0f64; 3]);
    for (x, y) in b.iter().zip(&t.lab) {
        let d = [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
        de += ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64).sqrt();
        for c in 0..3 {
            off[c] += d[c] as f64;
        }
    }
    let n = b.len().max(1) as f64;
    [de / n, off[0] / n, off[1] / n, off[2] / n]
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
    let base = Recipe::default();
    let render = |d: &Demosaiced, a: &Adjust| {
        let r = Recipe {
            global: base.global.plus(a),
            ..base.clone()
        };
        d.finish(&Look::DEFAULT, &r, None)
    };
    let auto =
        argus_engine::auto::auto_adjust(|a| Ok(render(&small, a)), true).map_err(|x| e(&x))?;
    let mut out = json!({
        "id": c.id,
        "camera": format!("{} {}", raw.make, raw.model),
        "as_shot_wb": raw.wb,
        // At Auto's measuring size, as the Edit head reads them in the app.
        "features": Map::from_iter(
            argus_engine::edit_head::features(&render(&small, &Adjust::default()))
                .into_iter()
                .map(|(k, v)| (k, json!((v * 1000.0).round() / 1000.0)))
        ),
        "head": adjust_json(&argus_engine::edit_head::offset(&render(&small, &Adjust::default()), &auto)),
        "auto": adjust_json(&auto),
    });
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
    if let Some(rs) = job.recipes.get(&c.id) {
        let big = match (job.save_dir.is_some(), job.save_edge) {
            (true, Some(e)) if e > edge => Demosaiced::new(&raw, e),
            _ => None,
        };
        let mut scores = Map::new();
        for (name, settings) in rs {
            let mut r = Recipe::default();
            for (k, v) in settings {
                r.set(k, *v, false).map_err(|x| e(&x))?;
            }
            let img = d.finish(&Look::DEFAULT, &r, None);
            scores.insert(name.clone(), json!(rnd(distance(&img, &target))));
            let wanted = job.save.as_ref().is_none_or(|s| s.contains(name));
            if let (Some(dir), true) = (&job.save_dir, wanted) {
                let img = match &big {
                    Some(b) => b.finish(&Look::DEFAULT, &r, None),
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
