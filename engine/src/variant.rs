//! Harness variants: a JSON file named by `ARGUS_VARIANT` (or `Tools::with_variant`) that swaps the AI's prompts, the
//! takes of a fresh Suggestion and the review rounds without a rebuild, so one binary can
//! run several experiments back to back (`bench/score.py --variant`, see
//! `docs/agents/experiments.md`). Every field is optional; what a file leaves out keeps the
//! built-in harness. Unset, nothing changes.

use crate::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    /// A name for the run, shown in bench reports.
    pub name: Option<String>,
    /// Notes for whoever reads the run later: what this variant tries.
    pub notes: Option<String>,
    /// Replaces the fresh Suggestions system prompt (`{count}` is filled in).
    pub fresh: Option<String>,
    /// Replaces the review-round prompt (`{before}` is filled in).
    pub review: Option<String>,
    /// Replaces the settings reference appended to both.
    pub settings: Option<String>,
    /// Replaces the takes of a fresh Suggestion, in order.
    pub takes: Option<Vec<TakeSpec>>,
    /// Review rounds at every Compute level (ARGUS_REFINE_ROUNDS still wins).
    pub rounds: Option<usize>,
    /// The first take keeps Auto's Exposure and Highlights (default true).
    pub keep_brightness: Option<bool>,
    /// Takes start with the Adaptive layer of presence and Masks (default true).
    pub adaptive: Option<bool>,
    /// Replaces the cull system prompt.
    pub cull: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TakeSpec {
    pub name: String,
    pub brief: String,
    /// Starting moves on top of Auto, in suggest.rs setting paths.
    #[serde(default)]
    pub start: BTreeMap<String, f64>,
}

/// The variant `ARGUS_VARIANT` names, or the built-in harness (the cull's; Suggestions go
/// through `Tools::variant`). A file that can't be read or
/// parsed is an error, so an experiment never silently runs the default.
pub fn current() -> Result<Variant> {
    match std::env::var("ARGUS_VARIANT") {
        Ok(path) if !path.is_empty() => load(&path),
        _ => Ok(Variant::default()),
    }
}

pub fn load(path: &str) -> Result<Variant> {
    let text =
        std::fs::read_to_string(path).map_err(|e| Error(format!("ARGUS_VARIANT {path}: {e}")))?;
    let v: Variant =
        serde_json::from_str(&text).map_err(|e| Error(format!("ARGUS_VARIANT {path}: {e}")))?;
    if v.takes.as_ref().is_some_and(|t| t.is_empty()) {
        return Err(Error(format!("ARGUS_VARIANT {path}: takes is empty")));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_are_refused() {
        let dir = std::env::temp_dir().join(format!("argus-variant-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.json");
        std::fs::write(&path, r#"{"name":"x","frsh":"typo"}"#).unwrap();
        assert!(load(path.to_str().unwrap()).is_err());
        std::fs::write(
            &path,
            r#"{"name":"x","takes":[{"name":"Natural","brief":"b","start":{"contrast":5}}]}"#,
        )
        .unwrap();
        let v = load(path.to_str().unwrap()).unwrap();
        assert_eq!(v.takes.unwrap()[0].start["contrast"], 5.0);
    }
}
