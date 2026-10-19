//! People segmentation (MediaPipe multiclass selfie segmenter, ADR 0012): one probability
//! plane per class, at the segmenter's size. It backs the people, skin and hair Masks.

use super::{Analyzer, Cost, Fact, Input};
use crate::masks::models::{self, CLASSES, SEG_SIZE};
use crate::{Error, Result};
use serde_json::json;

pub struct Segment;

impl Analyzer for Segment {
    fn id(&self) -> &'static str {
        "segment"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let planes = models::segment(input.small())?;
        let n = SEG_SIZE * SEG_SIZE;
        let coverage: serde_json::Map<String, serde_json::Value> = CLASSES
            .iter()
            .zip(&planes)
            .map(|(c, p)| {
                let mean = p.iter().sum::<f32>() / n as f32;
                (c.to_string(), json!((mean * 1000.0).round() / 1000.0))
            })
            .collect();
        Ok(Fact {
            value: json!({ "size": SEG_SIZE, "coverage": coverage }),
            blob: Some(planes.iter().flat_map(|p| super::to_bytes(p)).collect()),
        })
    }
}

/// The class planes in a Fact this Analyzer made, in [`CLASSES`] order.
pub fn planes(fact: &Fact) -> Result<[Vec<f32>; 6]> {
    let n = SEG_SIZE * SEG_SIZE;
    let b = fact
        .blob
        .as_deref()
        .filter(|b| b.len() == n * 6)
        .ok_or_else(|| Error("segment Fact has no planes".into()))?;
    Ok(std::array::from_fn(|c| {
        super::from_bytes(&b[c * n..(c + 1) * n])
    }))
}
