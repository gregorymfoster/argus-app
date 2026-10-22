//! The Edit head: a small learned step on top of Auto that moves the Basic sliders toward
//! how the person finishes their own edits (docs/research/edit-head.md).
//!
//! Auto measures a Photo and sets the sliders any careful editor would. The person, though,
//! has a style: Greg's finished edits run about +37 Contrast and +19 Temp from Auto. The head
//! reads cheap facts about the unedited render (brightness percentiles, clipping, colour cast,
//! skin and blue-sky share) plus Auto's own sliders, and predicts the offset with a ridge
//! regression fitted by `bench/edit_head.py export` to the person's finished edits. With the
//! Photo's Analysis (the Mask models' faces, people and sky) it also reads the scene
//! ([`scene_features`]): how bright the faces, the people and the sky sit against the frame,
//! and whether the light is behind the subject (`bench/edit_scene.py`). Each Library learns
//! its own on top of the shipped one from the RAWs the person exports ([`learn_library`]).
//! It is off unless a harness variant turns it on (`auto.edit_head`, crate::auto::Params),
//! so the review page can judge it against Auto first.

use crate::masks::Analysis;
use crate::recipe::Adjust;
use crate::RgbImage;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// The sliders the head moves, in the model file's order.
pub const SLIDERS: [&str; 9] = [
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "temp",
    "tint",
    "vibrance",
];

/// A fitted head: `offset = intercept + weights . (features - mean) / sd`, the shipped one
/// (fitted to Greg's bench edits) or one learned on top of it from a Library's own finished
/// edits ([`Head::learn`]).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Head {
    features: Vec<String>,
    sliders: Vec<String>,
    mean: Vec<f64>,
    sd: Vec<f64>,
    intercept: Vec<f64>,
    /// One row per feature, one column per slider.
    weights: Vec<Vec<f64>>,
    note: String,
}

/// One finished edit to learn from: the facts the head reads ([`facts`]) and the offset
/// from Auto the person's own sliders made, in [`SLIDERS`] order.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Sample {
    pub facts: Vec<(String, f64)>,
    pub offset: [f64; 9],
}

/// A feature further from the average than this many SDs reads as this far, so one odd
/// scene (a red-lit room) can't swing a slider past anything the head learned from.
const ZMAX: f64 = 3.0;

/// The person's average move from the shipped head counts in full only after this many
/// edits: n / (n + SHRINK) of it, as the Style profile does (crate::style).
const SHRINK: f64 = 4.0;
/// How firmly the per-feature weights hold to the shipped head's (the ridge strength the
/// bench chose for the shipped head on 104 edits): a few dozen edits barely move them.
const ALPHA: f64 = 300.0;

impl Head {
    /// The head shipped with Argus, fitted by `bench/edit_scene.py export`.
    pub fn shipped() -> &'static Head {
        static M: OnceLock<Head> = OnceLock::new();
        M.get_or_init(|| {
            let m: Head = serde_json::from_str(include_str!("edit_head.json"))
                .expect("edit_head.json parses");
            assert_eq!(m.sliders, SLIDERS, "edit_head.json sliders");
            m
        })
    }

    /// The offset from Auto's sliders `auto` that the head predicts for a Photo whose
    /// unedited render (Auto measures at [`crate::auto::MEASURE_EDGE`]) is `none`. Without
    /// `analysis` the scene facts read as the training average.
    pub fn offset(&self, none: &RgbImage, auto: &Adjust, analysis: Option<&Analysis>) -> Adjust {
        let out = self.predict(&facts(none, auto, analysis));
        let r = |v: f64| v.round();
        Adjust {
            exposure: (out[0] * 20.0).round() / 20.0,
            contrast: r(out[1]),
            highlights: r(out[2]),
            shadows: r(out[3]),
            whites: r(out[4]),
            blacks: r(out[5]),
            temp: r(out[6]),
            tint: r(out[7]),
            vibrance: r(out[8]),
            ..Adjust::default()
        }
    }

    /// The z-scored features of a Photo's facts (0 for a scene fact it lacks).
    fn z(&self, facts: &[(String, f64)]) -> Vec<f64> {
        self.features
            .iter()
            .enumerate()
            .map(|(i, name)| match fact_of(facts, name) {
                Some(x) if self.sd[i] > 0.0 => ((x - self.mean[i]) / self.sd[i]).clamp(-ZMAX, ZMAX),
                _ => 0.0,
            })
            .collect()
    }

    fn predict(&self, facts: &[(String, f64)]) -> Vec<f64> {
        let mut out = self.intercept.clone();
        for (z, w) in self.z(facts).iter().zip(&self.weights) {
            for (o, w) in out.iter_mut().zip(w) {
                *o += w * z;
            }
        }
        out
    }

    /// This head moved toward the person's own finished edits: their average miss shifts
    /// the intercept (shrunk while there are few), and a ridge on what's left nudges the
    /// weights, held to these ones. No edits, no change.
    pub fn learn(&self, samples: &[Sample]) -> Head {
        let mut h = self.clone();
        let n = samples.len();
        h.note = format!(
            "learned from {n} of the person's edits on top of: {}",
            self.note
        );
        if n == 0 {
            return h;
        }
        let k = SLIDERS.len();
        let zs: Vec<Vec<f64>> = samples.iter().map(|s| self.z(&s.facts)).collect();
        let miss: Vec<Vec<f64>> = samples
            .iter()
            .map(|s| {
                let p = self.predict(&s.facts);
                (0..k).map(|j| s.offset[j] - p[j]).collect()
            })
            .collect();
        let shift: Vec<f64> = (0..k)
            .map(|j| miss.iter().map(|m| m[j]).sum::<f64>() / (n as f64 + SHRINK))
            .collect();
        // (Z'Z + alpha I) dW = Z'(miss - shift), one right-hand side per slider.
        let f = self.features.len();
        let mut a = vec![vec![0.0; f]; f];
        let mut b = vec![vec![0.0; k]; f];
        for (z, m) in zs.iter().zip(&miss) {
            for i in 0..f {
                for (j, aij) in a[i].iter_mut().enumerate() {
                    *aij += z[i] * z[j];
                }
                for j in 0..k {
                    b[i][j] += z[i] * (m[j] - shift[j]);
                }
            }
        }
        for (i, row) in a.iter_mut().enumerate() {
            row[i] += ALPHA;
        }
        let dw = solve(a, b);
        for (w, d) in h.weights.iter_mut().zip(&dw) {
            for (w, d) in w.iter_mut().zip(d) {
                *w += d;
            }
        }
        for (c, s) in h.intercept.iter_mut().zip(&shift) {
            *c += s;
        }
        h
    }
}

/// Solves `a x = b` (a symmetric positive definite, from a ridge) by Gaussian elimination
/// with partial pivoting; `b` has one column per right-hand side.
#[allow(clippy::needless_range_loop)] // row operations read clearest as indices
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let n = a.len();
    for c in 0..n {
        let p = (c..n)
            .max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))
            .unwrap_or(c);
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..n {
            let f = a[r][c] / a[c][c];
            if f != 0.0 {
                for k in c..n {
                    a[r][k] -= f * a[c][k];
                }
                for k in 0..b[r].len() {
                    b[r][k] -= f * b[c][k];
                }
            }
        }
    }
    for c in (0..n).rev() {
        for k in 0..b[c].len() {
            let s: f64 = (c + 1..n).map(|j| a[c][j] * b[j][k]).sum();
            b[c][k] = (b[c][k] - s) / a[c][c];
        }
    }
    b
}

/// The scene facts [`scene_features`] gives, which a Photo without an Analysis lacks.
const SCENE: [&str; 8] = [
    "people",
    "sky_share",
    "faces_n",
    "face_area",
    "face_l",
    "subject_l",
    "backlit",
    "sky_l",
];

/// Everything the head reads about a Photo: cheap facts about its unedited render, Auto's
/// sliders, and the scene when its Analysis is known.
pub fn facts(none: &RgbImage, auto: &Adjust, analysis: Option<&Analysis>) -> Vec<(String, f64)> {
    let mut facts = features(none);
    for (k, v) in SLIDERS.iter().zip(sliders(auto)) {
        facts.push((format!("auto_{k}"), v));
    }
    if let Some(a) = analysis {
        facts.extend(scene_features(none, a));
    }
    facts
}

/// [`Head::offset`] of the shipped head.
pub fn offset(none: &RgbImage, auto: &Adjust, analysis: Option<&Analysis>) -> Adjust {
    Head::shipped().offset(none, auto, analysis)
}

/// A fact by name; None only for a scene fact the Photo has no Analysis for.
fn fact_of(facts: &[(String, f64)], name: &str) -> Option<f64> {
    match facts.iter().find(|(k, _)| k == name) {
        Some((_, v)) => Some(*v),
        None if SCENE.contains(&name) => None,
        None => panic!("edit_head.json feature {name} unknown"),
    }
}

fn sliders(a: &Adjust) -> [f64; 9] {
    [
        a.exposure,
        a.contrast,
        a.highlights,
        a.shadows,
        a.whites,
        a.blacks,
        a.temp,
        a.tint,
        a.vibrance,
    ]
}

/// CIELAB of an 8-bit sRGB pixel.
pub fn lab(rgb: [f32; 3]) -> [f32; 3] {
    let lin = |v: f32| {
        let c = v / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.119192 * g + 0.9503041 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Cheap facts about an unedited render, by name: what the head (and `argus edit-fit`)
/// read.
pub fn features(img: &RgbImage) -> Vec<(String, f64)> {
    let px = img.data.as_chunks::<3>().0;
    let labs: Vec<[f32; 3]> = px
        .iter()
        .step_by(2)
        .map(|p| lab([p[0] as f32, p[1] as f32, p[2] as f32]))
        .collect();
    let mut l: Vec<f32> = labs.iter().map(|x| x[0]).collect();
    l.sort_by(|a, b| a.total_cmp(b));
    let n = l.len().max(1) as f64;
    let pct = |q: f64| {
        l.get(((l.len().max(1) - 1) as f64 * q) as usize)
            .copied()
            .unwrap_or(0.0) as f64
    };
    let mean = |f: &dyn Fn(&[f32; 3]) -> f32| labs.iter().map(|x| f(x) as f64).sum::<f64>() / n;
    let ml = mean(&|x| x[0]);
    let sl = (labs.iter().map(|x| (x[0] as f64 - ml).powi(2)).sum::<f64>() / n).sqrt();
    let chroma = mean(&|x| (x[1] * x[1] + x[2] * x[2]).sqrt());
    // Mid-tone cast (what Auto's white balance reads).
    let mids: Vec<&[f32; 3]> = labs.iter().filter(|x| x[0] > 25.0 && x[0] < 85.0).collect();
    let mm = mids.len().max(1) as f64;
    let mid_a = mids.iter().map(|x| x[1] as f64).sum::<f64>() / mm;
    let mid_b = mids.iter().map(|x| x[2] as f64).sum::<f64>() / mm;
    // Skin-ish pixels: warm hue, moderate chroma, mid brightness (a rough share of people).
    let skin = labs
        .iter()
        .filter(|x| {
            let h = x[2].atan2(x[1]).to_degrees();
            let c = (x[1] * x[1] + x[2] * x[2]).sqrt();
            x[0] > 30.0 && x[0] < 85.0 && (20.0..=70.0).contains(&h) && (10.0..=40.0).contains(&c)
        })
        .count() as f64
        / n;
    // Blue-ish upper third (sky).
    let w = img.width.max(1);
    let top = w * (img.height / 3);
    let sky = px[..top.min(px.len())]
        .iter()
        .filter(|p| p[2] as i32 > p[0] as i32 + 15 && p[2] > 120)
        .count() as f64
        / top.max(1) as f64;
    [
        ("l_mean", ml),
        ("l_std", sl),
        ("l_p01", pct(0.01)),
        ("l_p05", pct(0.05)),
        ("l_p25", pct(0.25)),
        ("l_p50", pct(0.5)),
        ("l_p75", pct(0.75)),
        ("l_p95", pct(0.95)),
        ("l_p99", pct(0.99)),
        (
            "clip_hi",
            l.iter().filter(|v| **v > 97.0).count() as f64 / n,
        ),
        ("clip_lo", l.iter().filter(|v| **v < 3.0).count() as f64 / n),
        ("a_mean", mean(&|x| x[1])),
        ("b_mean", mean(&|x| x[2])),
        ("mid_a", mid_a),
        ("mid_b", mid_b),
        ("chroma", chroma),
        ("skin", skin),
        ("sky", sky),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Scene facts from the Photo's Analysis (Masks' models) over the same unedited render:
/// how much is people, sky and faces, and how bright each sits against the frame, so the
/// head can tell a backlit portrait from a dim interior or a bright sky. Without an
/// Analysis the head reads these as the training average.
pub fn scene_features(img: &RgbImage, a: &Analysis) -> Vec<(String, f64)> {
    let (w, h) = (img.width.max(1), img.height.max(1));
    let px = img.data.as_chunks::<3>().0;
    // L at a normalised point (nearest pixel).
    let l_at = |u: f32, v: f32| {
        let x = ((u * w as f32) as usize).min(w - 1);
        let y = ((v * h as f32) as usize).min(h - 1);
        let p = px[y * w + x];
        lab([p[0] as f32, p[1] as f32, p[2] as f32])[0] as f64
    };
    // Mean L weighted by a probability plane of side `n` stretched over the frame.
    let weighted = |plane: &[f32], pw: usize, ph: usize| {
        let (mut s, mut t) = (0f64, 0f64);
        for y in (0..ph).step_by(2) {
            for x in (0..pw).step_by(2) {
                let p = plane[y * pw + x] as f64;
                if p > 0.05 {
                    s += p * l_at((x as f32 + 0.5) / pw as f32, (y as f32 + 0.5) / ph as f32);
                    t += p;
                }
            }
        }
        (s, t)
    };
    let frame = {
        let (mut s, mut n) = (0f64, 0f64);
        for y in (0..h).step_by(4) {
            for x in (0..w).step_by(4) {
                let p = px[y * w + x];
                s += lab([p[0] as f32, p[1] as f32, p[2] as f32])[0] as f64;
                n += 1.0;
            }
        }
        s / n.max(1.0)
    };
    let n = crate::masks::models::SEG_SIZE;
    let bg = a.class("background");
    let people: Vec<f32> = bg.iter().map(|b| 1.0 - b).collect();
    let (ps, pt) = weighted(&people, n, n);
    let (bs, bt) = weighted(bg, n, n);
    let (ss, st) = weighted(&a.sky, a.sky_w, a.sky_h);
    let cov = a.coverage();
    // Faces: mean L inside the middle of each box, weighted by area.
    let (mut fs, mut ft, mut area) = (0f64, 0f64, 0f64);
    for f in &a.faces {
        let ar = (f.w * f.h) as f64;
        area += ar;
        for j in 0..5 {
            for i in 0..5 {
                let u = f.x + f.w * (0.2 + 0.15 * i as f32);
                let v = f.y + f.h * (0.2 + 0.15 * j as f32);
                fs += ar * l_at(u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
                ft += ar;
            }
        }
    }
    // Each region's brightness against the frame, 0 when the region isn't there.
    let rel = |s: f64, t: f64, min: f64| if t > min { s / t - frame } else { 0.0 };
    let min = 0.01 * ((n / 2) * (n / 2)) as f64;
    let sky_min = 0.01 * ((a.sky_w / 2) * (a.sky_h / 2)) as f64;
    let subject = rel(ps, pt, min);
    let backdrop = rel(bs, bt, min);
    [
        ("people", cov.people as f64),
        ("sky_share", cov.sky as f64),
        ("faces_n", (a.faces.len() as f64).min(5.0)),
        ("face_area", area.min(1.0)),
        ("face_l", if ft > 0.0 { fs / ft - frame } else { 0.0 }),
        ("subject_l", subject),
        ("backlit", if pt > min { backdrop - subject } else { 0.0 }),
        ("sky_l", rel(ss, st, sky_min)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

// ---- Learning from the person's own Library ----

/// Where a Library keeps its learned head and the samples it came from.
const FILE: &str = "edit-head.json";

#[derive(Default, Deserialize, Serialize)]
struct Stored {
    head: Option<Head>,
    /// By Photo id: the hash of the Recipe the sample was read from, and the sample.
    #[serde(default)]
    samples: std::collections::BTreeMap<String, (String, Sample)>,
}

/// What [`learn_library`] did.
#[derive(Debug, Serialize)]
pub struct Learned {
    /// Finished edits learned from (exported RAWs).
    pub photos: usize,
    /// Of those, how many were read fresh (the rest came from the cache).
    pub read: usize,
    /// The learned head's mean offset from Auto, by slider.
    pub mean: std::collections::BTreeMap<String, f64>,
}

impl Head {
    /// The Library's learned head if it has one, else the shipped head.
    pub fn for_library(lib: &crate::library::Library) -> std::sync::Arc<Head> {
        let stored = std::fs::read(lib.root().join(FILE))
            .ok()
            .and_then(|b| serde_json::from_slice::<Stored>(&b).ok())
            .and_then(|s| s.head);
        std::sync::Arc::new(stored.unwrap_or_else(|| Head::shipped().clone()))
    }
}

/// Learn the Library's own head from every RAW the person has exported: what Auto (without
/// the head) makes of it, against the Recipe they finished it with. Samples are cached by
/// Recipe, so re-learning after one more export reads one Photo.
pub fn learn_library(lib: &crate::library::Library) -> crate::Result<Learned> {
    use crate::recipe::Recipe;
    let path = lib.root().join(FILE);
    let mut stored: Stored = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let exported: std::collections::BTreeSet<_> = lib
        .usage_events(None)?
        .into_iter()
        .filter(|e| e.kind == "export")
        .map(|e| e.photo_id)
        .collect();
    let plain = crate::auto::Params {
        edit_head: false,
        ..crate::auto::Params::current()?
    };
    let mut keep = std::collections::BTreeMap::new();
    let mut read = 0;
    for id in exported {
        let Ok(p) = crate::tools::find_photo(lib, id) else {
            continue;
        };
        if p.kind != "raw" {
            continue;
        }
        let own = crate::tools::current_recipe(lib, id)?;
        let hash = blake3::hash(own.to_value().to_string().as_bytes())
            .to_hex()
            .to_string();
        let key = id.to_string();
        if let Some((h, s)) = stored.samples.remove(&key) {
            if h == hash {
                keep.insert(key, (h, s));
                continue;
            }
        }
        let base = crate::tools::with_look(lib, &p, &crate::auto::cleared(&own))?;
        let masks = crate::render::masks_for(lib, &p, &base);
        let render = |s: &Adjust| {
            let r = Recipe {
                global: base.global.plus(s),
                ..base.clone()
            };
            crate::render::render_photo(&p, &r, Some(crate::auto::MEASURE_EDGE), masks.clone())
        };
        let auto = crate::auto::auto_adjust_with(render, true, &plain)?;
        let analysis = crate::render::analysis(lib, &p).ok();
        let g = &own.global;
        let mine = [
            g.exposure,
            g.contrast,
            g.highlights,
            g.shadows,
            g.whites,
            g.blacks,
            g.temp,
            g.tint,
            g.vibrance,
        ];
        let theirs = sliders(&auto);
        let sample = Sample {
            facts: facts(&render(&Adjust::default())?, &auto, analysis.as_deref()),
            offset: std::array::from_fn(|j| mine[j] - theirs[j]),
        };
        keep.insert(key, (hash, sample));
        read += 1;
    }
    let samples: Vec<Sample> = keep.values().map(|(_, s)| s.clone()).collect();
    let head = Head::shipped().learn(&samples);
    let mean = SLIDERS
        .iter()
        .enumerate()
        .map(|(j, k)| {
            let m = samples
                .iter()
                .map(|s| head.predict(&s.facts)[j])
                .sum::<f64>()
                / samples.len().max(1) as f64;
            (k.to_string(), (m * 100.0).round() / 100.0)
        })
        .collect();
    let out = Learned {
        photos: samples.len(),
        read,
        mean,
    };
    stored = Stored {
        head: (!samples.is_empty()).then_some(head),
        samples: keep,
    };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec(&stored).map_err(|e| crate::Error(e.to_string()))?,
    )
    .and_then(|_| std::fs::rename(&tmp, &path))
    .map_err(|e| crate::Error(format!("{}: {e}", path.display())))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(seed: f64) -> Vec<(String, f64)> {
        let h = Head::shipped();
        h.features
            .iter()
            .enumerate()
            .map(|(i, f)| {
                (
                    f.clone(),
                    h.mean[i] + h.sd[i] * ((seed * (i as f64 + 1.0)).sin()),
                )
            })
            .collect()
    }

    #[test]
    fn learning_moves_the_head_toward_the_persons_edits_by_how_many_there_are() {
        let h = Head::shipped();
        let mut none = h.learn(&[]);
        none.note = h.note.clone();
        assert_eq!(&none, h, "no edits, no change");
        // A person who always wants 20 more Contrast than the shipped head gives.
        let edits = |n: usize| -> Vec<Sample> {
            (0..n)
                .map(|i| {
                    let facts = photo(i as f64 * 0.7 + 0.3);
                    let mut offset: [f64; 9] = h.predict(&facts).try_into().unwrap();
                    offset[1] += 20.0;
                    Sample { facts, offset }
                })
                .collect()
        };
        let probe = photo(9.1);
        let before = h.predict(&probe)[1];
        let few = h.learn(&edits(4)).predict(&probe)[1] - before;
        let many = h.learn(&edits(60)).predict(&probe)[1] - before;
        assert!((8.0..=12.0).contains(&few), "4 edits: half way, got {few}");
        assert!(
            (17.0..=20.5).contains(&many),
            "60 edits: nearly all, got {many}"
        );
        // The other sliders stay where the shipped head put them.
        let other = h.learn(&edits(60)).predict(&probe)[6] - h.predict(&probe)[6];
        assert!(other.abs() < 0.5, "temp moved {other}");
    }

    #[test]
    fn a_missing_scene_reads_as_the_average_scene() {
        let h = Head::shipped();
        let mut facts = photo(1.3);
        let with: Vec<f64> = h.z(&facts);
        facts.retain(|(k, _)| !SCENE.contains(&k.as_str()));
        let without = h.z(&facts);
        for (i, f) in h.features.iter().enumerate() {
            if SCENE.contains(&f.as_str()) {
                assert_eq!(without[i], 0.0, "{f}");
            } else {
                assert_eq!(without[i], with[i], "{f}");
            }
        }
    }
}
