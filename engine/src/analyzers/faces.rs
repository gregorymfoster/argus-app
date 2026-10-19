//! Face boxes and five landmarks per face (YuNet, ADR 0012), left to right.

use super::{Analyzer, Cost, Fact, Input};
use crate::masks::{models, Face};
use crate::Result;

pub struct Faces;

impl Analyzer for Faces {
    fn id(&self) -> &'static str {
        "faces"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let faces = models::detect_faces(input.small())?;
        Ok(Fact::json(
            serde_json::to_value(faces).expect("faces serialise"),
        ))
    }
    fn caption(&self, fact: &Fact) -> Option<String> {
        match read(fact).len() {
            0 => None,
            1 => Some("1 face".into()),
            n => Some(format!("{n} faces")),
        }
    }
}

/// The faces in a Fact this Analyzer made.
pub fn read(fact: &Fact) -> Vec<Face> {
    serde_json::from_value(fact.value.clone()).unwrap_or_default()
}
