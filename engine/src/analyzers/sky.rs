//! Sky probability (DeepLab v3 on ADE20K, ADR 0012), at the scene model's output size.

use super::{Analyzer, Cost, Fact, Input};
use crate::masks::models::{self, SCENE_OUT};
use crate::{Error, Result};
use serde_json::json;

pub struct Sky;

impl Analyzer for Sky {
    fn id(&self) -> &'static str {
        "sky"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let plane = models::sky(input.small())?;
        let mean = plane.iter().sum::<f32>() / plane.len().max(1) as f32;
        Ok(Fact {
            value: json!({ "size": SCENE_OUT, "coverage": (mean * 1000.0).round() / 1000.0 }),
            blob: Some(super::to_bytes(&plane)),
        })
    }
}

/// The sky plane (`SCENE_OUT`²) in a Fact this Analyzer made.
pub fn plane(fact: &Fact) -> Result<Vec<f32>> {
    let b = fact
        .blob
        .as_deref()
        .filter(|b| b.len() == SCENE_OUT * SCENE_OUT)
        .ok_or_else(|| Error("sky Fact has no plane".into()))?;
    Ok(super::from_bytes(b))
}
