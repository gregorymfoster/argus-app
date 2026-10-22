//! Signals (sharpness, brightness, clipping) as a Cheap Analyzer, measured on the
//! preview at the Contact sheet's size so values compare across a Shoot.

use super::{Analyzer, Cost, Fact, Input};
use crate::Result;

pub struct Signals;

impl Analyzer for Signals {
    fn id(&self) -> &'static str {
        "signals"
    }
    fn version(&self) -> u32 {
        2
    }
    fn cost(&self) -> Cost {
        Cost::Cheap
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let edge = crate::cull::SHEET_LONG_EDGE;
        let img = image::DynamicImage::ImageRgb8(input.preview.clone())
            .thumbnail(edge, edge)
            .to_rgb8();
        let s = crate::signals::measure(&img);
        Ok(Fact::json(
            serde_json::to_value(s).expect("signals serialise"),
        ))
    }
}

/// The Signals in a Fact this Analyzer made.
pub fn read(fact: &Fact) -> crate::signals::Signals {
    serde_json::from_value(fact.value.clone()).unwrap_or_default()
}
