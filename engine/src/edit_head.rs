//! The Edit head: a small learned step on top of Auto that moves the Basic sliders toward
//! how the person finishes their own edits (docs/research/edit-head.md).
//!
//! Auto measures a Photo and sets the sliders any careful editor would. The person, though,
//! has a style: Greg's finished edits run about +37 Contrast and +19 Temp from Auto. The head
//! reads cheap facts about the unedited render (brightness percentiles, clipping, colour cast,
//! skin and blue-sky share) plus Auto's own sliders, and predicts the offset with a ridge
//! regression fitted by `bench/edit_head.py export` to the person's finished edits. It is off
//! unless a harness variant turns it on (`auto.edit_head`, crate::auto::Params), so the
//! review page can judge it against Auto first.

use crate::recipe::Adjust;
use crate::RgbImage;
use serde::Deserialize;
use std::sync::OnceLock;

/// The sliders the head moves, in the model file's order.
const SLIDERS: [&str; 9] = [
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

/// A fitted head: `offset = intercept + weights . (features - mean) / sd`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Model {
    features: Vec<String>,
    sliders: Vec<String>,
    mean: Vec<f64>,
    sd: Vec<f64>,
    intercept: Vec<f64>,
    /// One row per feature, one column per slider.
    weights: Vec<Vec<f64>>,
    #[allow(dead_code)]
    note: String,
}

fn model() -> &'static Model {
    static M: OnceLock<Model> = OnceLock::new();
    M.get_or_init(|| {
        let m: Model =
            serde_json::from_str(include_str!("edit_head.json")).expect("edit_head.json parses");
        assert_eq!(m.sliders, SLIDERS, "edit_head.json sliders");
        m
    })
}

/// The offset from Auto's sliders `auto` that the head predicts for a Photo whose unedited
/// render (Auto measures at [`crate::auto::MEASURE_EDGE`]) is `none`.
pub fn offset(none: &RgbImage, auto: &Adjust) -> Adjust {
    let m = model();
    let mut facts = features(none);
    for (k, v) in SLIDERS.iter().zip(sliders(auto)) {
        facts.push((format!("auto_{k}"), v));
    }
    let mut out = m.intercept.clone();
    for (i, name) in m.features.iter().enumerate() {
        let x = facts
            .iter()
            .find(|(k, _)| k == name)
            .unwrap_or_else(|| panic!("edit_head.json feature {name} unknown"))
            .1;
        let z = if m.sd[i] > 0.0 {
            (x - m.mean[i]) / m.sd[i]
        } else {
            0.0
        };
        for (o, w) in out.iter_mut().zip(&m.weights[i]) {
            *o += w * z;
        }
    }
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
