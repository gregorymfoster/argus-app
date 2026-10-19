//! An image embedding: MediaPipe's image embedder (MobileNetV3 large, Apache 2.0), 1280
//! numbers per Photo, unit length, so the dot product of two is their cosine similarity.
//! Near-identical frames score about 0.9 and above; it groups bursts into Stacks.

use super::runtime::{floats, ndarray, Lazy, Tensor};
use super::{Analyzer, Cost, Fact, Input};
use crate::{Error, Result};
use serde_json::json;

const EMBEDDER: &[u8] = include_bytes!("../../models/image_embedder_mediapipe.onnx");
const SIZE: usize = 224;
pub const DIMS: usize = 1280;
static MODEL: Lazy = Lazy::new(EMBEDDER, &[1, SIZE, SIZE, 3]);

pub struct Embedding;

impl Analyzer for Embedding {
    fn id(&self) -> &'static str {
        "embedding"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        // The whole frame stretched to the model's square, RGB 0..1, as MediaPipe does.
        let img = image::imageops::resize(
            input.preview,
            SIZE as u32,
            SIZE as u32,
            image::imageops::FilterType::Triangle,
        );
        let data: Vec<f32> = img.as_raw().iter().map(|&v| v as f32 / 255.0).collect();
        let t: Tensor = ndarray::Array4::from_shape_vec((1, SIZE, SIZE, 3), data)
            .map_err(|e| Error(format!("embedder input: {e}")))?
            .into();
        let out = MODEL.run(t)?;
        let v = floats(&out[0])?;
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        let blob = v.iter().flat_map(|x| (x / norm).to_le_bytes()).collect();
        Ok(Fact {
            value: json!({ "dims": DIMS }),
            blob: Some(blob),
        })
    }
}

/// The unit vector in a Fact this Analyzer made.
pub fn vector(fact: &Fact) -> Option<Vec<f32>> {
    let b = fact.blob.as_deref().filter(|b| b.len() == DIMS * 4)?;
    Some(
        b.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

/// Cosine similarity of two unit vectors.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
