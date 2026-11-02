//! Sensor noise ([`crate::noise`]) as an Analyzer: the RAW's own Poisson-Gaussian fit, which
//! Auto Denoise reads to pick a Denoise Amount. Decoding the RAW makes it a Model-cost
//! Analyzer (about a second a Photo); a JPEG or HEIC has no sensor data and gets `null`.

use super::{Analyzer, Cost, Fact, Input};
use crate::noise::NoiseModel;
use crate::Result;

pub struct Noise;

impl Analyzer for Noise {
    fn id(&self) -> &'static str {
        "noise"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        if input.photo.kind != "raw" {
            return Ok(Fact::json(serde_json::Value::Null));
        }
        let raw = crate::decode(std::path::Path::new(&input.photo.path))?;
        let model = crate::noise::measure(&raw);
        Ok(Fact::json(
            serde_json::to_value(model).expect("noise serialises"),
        ))
    }
    fn caption(&self, fact: &Fact) -> Option<String> {
        let v = crate::noise::visible(&read(fact)?, 0.0);
        (crate::noise::auto_amount(v) >= 40.0).then(|| "very noisy (high ISO)".into())
    }
}

/// The noise fit in a Fact this Analyzer made; None for a non-RAW.
pub fn read(fact: &Fact) -> Option<NoiseModel> {
    serde_json::from_value(fact.value.clone()).ok()
}
