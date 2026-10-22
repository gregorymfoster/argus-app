//! Who each face is, as numbers: OpenCV Zoo's SFace (Apache 2.0), 128 per face, unit
//! length, so the dot product of two faces is how alike they are. Faces of one person
//! score about 0.4 and above, strangers around 0. It groups faces for the People view
//! ([`crate::people`]); it never names anyone.
//!
//! Each face the `faces` Fact found (same order) is turned upright and scaled by its five
//! landmarks onto SFace's 112 px template, as OpenCV's `FaceRecognizerSF::alignCrop` does.

use super::runtime::{floats, ndarray, Lazy, Tensor};
use super::{Analyzer, Cost, Fact, Input};
use crate::masks::Face;
use crate::{Error, Result};
use serde_json::json;

const SFACE: &[u8] = include_bytes!("../../models/face_recognition_sface_2021dec.onnx");
const SIZE: usize = 112;
pub const DIMS: usize = 128;
static MODEL: Lazy = Lazy::new(SFACE, &[1, 3, SIZE, SIZE]);

/// Faces narrower than this (pixels of the preview) are too small to tell people apart.
pub const MIN_FACE_PX: f32 = 36.0;

/// Where SFace (and ArcFace) expect the eyes, nose and mouth corners, in its 112 px crop.
const TEMPLATE: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

pub struct Identity;

impl Analyzer for Identity {
    fn id(&self) -> &'static str {
        "identity"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn needs(&self) -> &'static [&'static str] {
        &["faces"]
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let faces = super::faces::read(input.fact("faces")?);
        let img = input.preview;
        let (w, h) = (img.width() as f32, img.height() as f32);
        // Which faces have a vector, in `faces` order; the blob holds those vectors in turn.
        let mut read = Vec::new();
        let mut blob = Vec::new();
        for f in &faces {
            if (f.w * w).max(f.h * h) < MIN_FACE_PX {
                read.push(false);
                continue;
            }
            let v = embed(img, f)?;
            blob.extend(v.iter().flat_map(|x| x.to_le_bytes()));
            read.push(true);
        }
        Ok(Fact {
            value: json!({ "dims": DIMS, "read": read }),
            blob: Some(blob),
        })
    }
}

/// The unit-length vector of one face in `img`.
pub fn embed(img: &image::RgbImage, f: &Face) -> Result<Vec<f32>> {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let src = [f.eyes[0], f.eyes[1], f.nose, f.mouth[0], f.mouth[1]].map(|p| [p[0] * w, p[1] * h]);
    let m = similarity(&src, &TEMPLATE);
    // The inverse maps a crop pixel back into the image.
    let det = m[0] * m[4] - m[1] * m[3];
    let inv = [
        m[4] / det,
        -m[1] / det,
        (m[1] * m[5] - m[4] * m[2]) / det,
        -m[3] / det,
        m[0] / det,
        (m[3] * m[2] - m[0] * m[5]) / det,
    ];
    // SFace takes RGB 0..255, channels first (OpenCV's blobFromImage with swapRB).
    let input: Tensor = ndarray::Array4::from_shape_fn((1, 3, SIZE, SIZE), |(_, c, y, x)| {
        let (u, v) = (x as f32, y as f32);
        let sx = inv[0] * u + inv[1] * v + inv[2];
        let sy = inv[3] * u + inv[4] * v + inv[5];
        bilinear(img, sx, sy, c)
    })
    .into();
    let out = MODEL.run(input)?;
    let v = floats(&out[0])?;
    if v.len() != DIMS {
        return Err(Error(format!("face model gave {} numbers", v.len())));
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    Ok(v.iter().map(|x| x / norm).collect())
}

/// One channel of `img` at a pixel position (centres at whole numbers), 0..255; black outside.
fn bilinear(img: &image::RgbImage, x: f32, y: f32, c: usize) -> f32 {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= w || y >= h {
            0.0
        } else {
            img.get_pixel(x as u32, y as u32)[c] as f32
        }
    };
    let a = px(x0, y0) * (1.0 - fx) + px(x0 + 1, y0) * fx;
    let b = px(x0, y0 + 1) * (1.0 - fx) + px(x0 + 1, y0 + 1) * fx;
    a * (1.0 - fy) + b * fy
}

/// The least-squares similarity transform (turn, scale, shift) taking `src` onto `dst`
/// (Umeyama's method in 2-D), as `[a, b, tx, c, d, ty]`: x' = a x + b y + tx.
fn similarity(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> [f32; 6] {
    let n = src.len() as f32;
    let mean = |p: &[[f32; 2]; 5]| {
        let s = p.iter().fold([0.0, 0.0], |a, q| [a[0] + q[0], a[1] + q[1]]);
        [s[0] / n, s[1] / n]
    };
    let (ms, md) = (mean(src), mean(dst));
    // With complex numbers z = x + iy, the best map is dst = k * src + t, k = Σ d·conj(s) / Σ |s|².
    let (mut re, mut im, mut var) = (0.0, 0.0, 0.0);
    for (s, d) in src.iter().zip(dst) {
        let (sx, sy) = (s[0] - ms[0], s[1] - ms[1]);
        let (dx, dy) = (d[0] - md[0], d[1] - md[1]);
        re += dx * sx + dy * sy;
        im += dy * sx - dx * sy;
        var += sx * sx + sy * sy;
    }
    let (a, b) = (re / var.max(1e-9), im / var.max(1e-9));
    let tx = md[0] - (a * ms[0] - b * ms[1]);
    let ty = md[1] - (b * ms[0] + a * ms[1]);
    [a, -b, tx, b, a, ty]
}

/// Each face's vector in a Fact this Analyzer made, in `faces` order (None: too small).
pub fn vectors(fact: &Fact) -> Vec<Option<Vec<f32>>> {
    let read: Vec<bool> = serde_json::from_value(fact.value["read"].clone()).unwrap_or_default();
    let blob = fact.blob.as_deref().unwrap_or_default();
    let mut at = 0;
    read.into_iter()
        .map(|r| {
            if !r || blob.len() < (at + DIMS) * 4 {
                return None;
            }
            let v = blob[at * 4..(at + DIMS) * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect();
            at += DIMS;
            Some(v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_maps_onto_itself_and_a_turned_copy_maps_back() {
        let m = similarity(&TEMPLATE, &TEMPLATE);
        for (got, want) in m.iter().zip([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]) {
            assert!((got - want).abs() < 1e-4, "{m:?}");
        }
        // The template turned 30 degrees, doubled and moved: the map undoes all three.
        let (c, s) = (30f32.to_radians().cos(), 30f32.to_radians().sin());
        let src =
            TEMPLATE.map(|[x, y]| [2.0 * (c * x - s * y) + 40.0, 2.0 * (s * x + c * y) - 7.0]);
        let m = similarity(&src, &TEMPLATE);
        for (p, q) in src.iter().zip(&TEMPLATE) {
            let x = m[0] * p[0] + m[1] * p[1] + m[2];
            let y = m[3] * p[0] + m[4] * p[1] + m[5];
            assert!(
                (x - q[0]).abs() < 1e-2 && (y - q[1]).abs() < 1e-2,
                "{x},{y} vs {q:?}"
            );
        }
    }
}
