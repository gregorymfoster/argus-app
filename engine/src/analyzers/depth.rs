//! Depth: Depth Anything V2 Small (Apache-2.0, `engine/models/manifest.json`), how near each
//! pixel of the frame is, for Lens Blur ([`crate::lens_blur`]). Relative, not metric: 0 is
//! the farthest thing in the Photo and 1 the nearest. On demand: a Photo gets one the first
//! time Lens Blur is on for it (inline, a second or two), never from the Queue, since the
//! map is big (about 800 KB) and few Photos need it.
//!
//! `ARGUS_DEPTH_FAKE=1` swaps the model for a cheap stand-in (nearer toward the bottom and
//! the centre) so UI checks and tests run without it.

use super::runtime::{floats, ndarray, Lazy, Tensor};
use super::{Analyzer, Cost, Fact, Input};
use crate::lens_blur::Depth;
use crate::{Error, Result, RgbImage};
use serde_json::json;

const MODEL_BYTES: &[u8] = include_bytes!("../../models/depth_anything_v2_small_f16.onnx");
/// The model's input, a multiple of its 14-pixel patches near the 3:2 of most cameras: 518
/// on the short edge, as the model was trained.
const SHORT: usize = 518;
const LONG: usize = 784;
static LANDSCAPE: Lazy = Lazy::new(MODEL_BYTES, &[1, 3, SHORT, LONG]);
static PORTRAIT: Lazy = Lazy::new(MODEL_BYTES, &[1, 3, LONG, SHORT]);

pub struct DepthMap;

impl Analyzer for DepthMap {
    fn id(&self) -> &'static str {
        "depth"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Heavy
    }
    fn on_demand(&self) -> bool {
        true
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let d = estimate(input.small())?;
        Ok(Fact {
            value: json!({ "width": d.width, "height": d.height }),
            blob: Some(quantize(&d.plane)),
        })
    }
}

/// The depth map in a Fact this Analyzer made.
pub fn read(fact: &Fact) -> Result<Depth> {
    let dim = |k: &str| fact.value[k].as_u64().unwrap_or(0) as usize;
    let (width, height) = (dim("width"), dim("height"));
    let b = fact
        .blob
        .as_deref()
        .filter(|b| width > 0 && b.len() == width * height * 2)
        .ok_or_else(|| Error("depth Fact has no map".into()))?;
    Ok(Depth {
        plane: b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c) as f32 / 65535.0)
            .collect(),
        width,
        height,
    })
}

/// 16 bits a pixel is plenty for a map this smooth, and half the size of f32s.
fn quantize(plane: &[f32]) -> Vec<u8> {
    plane
        .iter()
        .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes())
        .collect()
}

/// Estimate the depth of an upright 8-bit image (any size), at the model's resolution.
pub fn estimate(img: &RgbImage) -> Result<Depth> {
    if std::env::var_os("ARGUS_DEPTH_FAKE").is_some() {
        return Ok(fake(img));
    }
    let portrait = img.height > img.width;
    let (w, h) = if portrait {
        (SHORT, LONG)
    } else {
        (LONG, SHORT)
    };
    let src = image::RgbImage::from_raw(img.width as u32, img.height as u32, img.data.clone())
        .ok_or_else(|| Error("depth: image buffer is the wrong size".into()))?;
    let small = image::imageops::resize(
        &src,
        w as u32,
        h as u32,
        image::imageops::FilterType::Triangle,
    );
    // ImageNet mean and deviation, channels first, as Depth Anything's preprocessing.
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    let mut data = vec![0f32; 3 * w * h];
    for (i, p) in small.pixels().enumerate() {
        for c in 0..3 {
            data[c * w * h + i] = (p[c] as f32 / 255.0 - MEAN[c]) / STD[c];
        }
    }
    let t: Tensor = ndarray::Array4::from_shape_vec((1, 3, h, w), data)
        .map_err(|e| Error(format!("depth input: {e}")))?
        .into();
    let out = if portrait { &PORTRAIT } else { &LANDSCAPE }.run(t)?;
    let raw = floats(&out[0])?;
    if raw.len() != w * h {
        return Err(Error(format!(
            "depth model gave {} values for {w}x{h}",
            raw.len()
        )));
    }
    Ok(Depth {
        plane: normalize(raw),
        width: w,
        height: h,
    })
}

/// The model's relative inverse depth (bigger is nearer) to 0..1 between its 1st and 99th
/// percentiles, so one stray pixel doesn't squash the rest.
fn normalize(raw: &[f32]) -> Vec<f32> {
    let mut s: Vec<f32> = raw.iter().copied().filter(|v| v.is_finite()).collect();
    if s.is_empty() {
        return vec![0.5; raw.len()];
    }
    s.sort_by(f32::total_cmp);
    let q = |p: f32| s[((s.len() - 1) as f32 * p) as usize];
    let (lo, hi) = (q(0.01), q(0.99));
    let span = (hi - lo).max(1e-6);
    raw.iter()
        .map(|v| ((v - lo) / span).clamp(0.0, 1.0))
        .collect()
}

/// A stand-in depth: nearer toward the bottom and the middle of the frame.
fn fake(img: &RgbImage) -> Depth {
    let (w, h) = (96, (96 * img.height / img.width.max(1)).max(1));
    let plane = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
            let centre = 1.0 - ((x - 0.5).abs() * 2.0).powi(2);
            (0.15 + 0.55 * y + 0.3 * centre * (y > 0.25) as u8 as f32).clamp(0.0, 1.0)
        })
        .collect();
    Depth {
        plane,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_spreads_the_middle_and_clamps_outliers() {
        let mut raw: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        raw[0] = -1e6;
        raw[999] = 1e6;
        let n = normalize(&raw);
        assert_eq!(n[0], 0.0);
        assert_eq!(n[999], 1.0);
        assert!((n[500] - 0.5).abs() < 0.02);
    }

    #[test]
    fn a_fact_round_trips() {
        let plane: Vec<f32> = (0..12).map(|i| i as f32 / 11.0).collect();
        let f = Fact {
            value: json!({ "width": 4, "height": 3 }),
            blob: Some(quantize(&plane)),
        };
        let d = read(&f).unwrap();
        assert_eq!((d.width, d.height), (4, 3));
        assert!(d
            .plane
            .iter()
            .zip(&plane)
            .all(|(a, b)| (a - b).abs() < 1e-4));
    }
}
