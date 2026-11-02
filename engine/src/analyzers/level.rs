//! Auto straighten's answer for a Photo (crate::framing::level): the turn that levels its
//! horizon or architecture, "level", or nothing to go by. On demand: only Crop & straighten
//! asks for it, so imports don't pay for a second run of the scene model.

use super::{Analyzer, Cost, Fact, Input};
use crate::framing::level::{self, Level};
use crate::Result;
use serde_json::json;

/// The long edge the detector reads at (the bench's size).
pub const EDGE: usize = 1024;

pub struct LevelFact;

impl Analyzer for LevelFact {
    fn id(&self) -> &'static str {
        "level"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn on_demand(&self) -> bool {
        true
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let full = crate::RgbImage {
            width: input.preview.width() as usize,
            height: input.preview.height() as usize,
            data: input.preview.as_raw().clone(),
        };
        let img = crate::masks::shrink(&full, EDGE);
        let scene = crate::framing::scene(&img)?;
        let mut v = match level::find(&img, Some(&scene)) {
            Level::Turn(a) => json!({ "turn": a }),
            Level::Level => json!({ "level": true }),
            Level::Unsure => json!({}),
        };
        // The upright shape, which the crop that goes with a turn depends on.
        v["w"] = json!(img.width);
        v["h"] = json!(img.height);
        Ok(Fact::json(v))
    }
}

/// The turn a Fact this Analyzer made proposes, if any.
pub fn turn(fact: &Fact) -> Option<f64> {
    fact.value.get("turn").and_then(|v| v.as_f64())
}

/// The upright width and height the Fact was made at.
pub fn size(fact: &Fact) -> Option<(f64, f64)> {
    Some((
        fact.value.get("w")?.as_f64()?,
        fact.value.get("h")?.as_f64()?,
    ))
}
