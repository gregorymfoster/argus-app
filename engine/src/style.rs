//! Style profile: how this person corrects the AI's takes, learned from the usage log.
//!
//! Each exported Photo whose History has a kept take says what the person changed on top of
//! it (`History::moved`). Over many Photos those changes show a habit ("brightens every take
//! a third of a stop, cools it a little"), and fresh Suggestions add that habit to their
//! takes, so the AI drifts toward the person's own edits without any training. A slider
//! counts only when the person moves it on most Photos (its median is the move), and
//! moves shrink toward zero while the evidence is thin, so a handful of edits changes
//! little and an empty log changes nothing.

use crate::library::Library;
use crate::usage::{history, History};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The global sliders a profile learns, with the smallest move worth making.
pub const SLIDERS: &[(&str, f64)] = &[
    ("exposure", 0.05),
    ("contrast", 2.0),
    ("highlights", 3.0),
    ("shadows", 3.0),
    ("whites", 3.0),
    ("blacks", 3.0),
    ("temp", 2.0),
    ("tint", 2.0),
    ("vibrance", 2.0),
    ("saturation", 2.0),
    ("clarity", 2.0),
    ("texture", 2.0),
    ("dehaze", 2.0),
];

/// Below this many corrections a profile stays empty.
pub const MIN_PHOTOS: usize = 3;
/// A take's own moves replace the overall ones once it has this many corrections.
pub const MIN_PER_TAKE: usize = 5;
/// Moves are scaled by n / (n + SHRINK): half strength at 4 corrections, 90% at 36.
const SHRINK: f64 = 4.0;
/// Only the most recent corrections count, so the profile follows a changing taste.
const RECENT: usize = 200;

/// One kept take and what the person changed on top of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Correction {
    /// The take's name (Natural, Punchy...) or Auto's label.
    #[serde(default)]
    pub take: String,
    /// Slider changes by dotted path, as `usage::slider_moves` gives them.
    #[serde(default)]
    pub moved: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Profile {
    /// How many corrections it was learned from.
    pub photos: usize,
    /// Moves added to every take, by slider.
    pub moves: BTreeMap<String, f64>,
    /// Moves for one take name, where it has enough corrections of its own.
    pub by_take: BTreeMap<String, BTreeMap<String, f64>>,
}

impl Profile {
    /// Learn from corrections, oldest first.
    pub fn learn(corrections: &[Correction]) -> Profile {
        let recent = &corrections[corrections.len().saturating_sub(RECENT)..];
        let mut p = Profile {
            photos: recent.len(),
            ..Default::default()
        };
        if recent.len() < MIN_PHOTOS {
            return p;
        }
        p.moves = moves(recent.iter());
        let mut names: BTreeMap<&str, usize> = BTreeMap::new();
        for c in recent {
            *names.entry(c.take.as_str()).or_default() += 1;
        }
        for (name, n) in names {
            if n >= MIN_PER_TAKE && !name.is_empty() {
                let own = moves(recent.iter().filter(|c| c.take == name));
                if own != p.moves {
                    p.by_take.insert(name.to_string(), own);
                }
            }
        }
        p
    }

    /// The Library's profile, from its usage log.
    pub fn of_library(lib: &Library) -> Result<Profile> {
        let mut events: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for e in lib.usage_events(None)? {
            events.entry(e.photo_id).or_insert_with(Vec::new).push(e);
        }
        // Oldest export first, so RECENT keeps the newest.
        let mut found: Vec<(String, Correction)> = events
            .values()
            .filter_map(|ev| {
                let at = ev.iter().rev().find(|e| e.kind == "export")?.at.clone();
                Some((at, correction(&history(ev))?))
            })
            .collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let list: Vec<Correction> = found.into_iter().map(|(_, c)| c).collect();
        Ok(Profile::learn(&list))
    }

    pub fn is_empty(&self) -> bool {
        self.moves.is_empty() && self.by_take.is_empty()
    }

    /// The moves for a take of this name.
    pub fn for_take(&self, name: &str) -> &BTreeMap<String, f64> {
        self.by_take.get(name).unwrap_or(&self.moves)
    }

    /// One line for a prompt or a person: "exposure +0.30, temp -4".
    pub fn line(moves: &BTreeMap<String, f64>) -> String {
        moves
            .iter()
            .map(|(k, v)| {
                if k == "exposure" {
                    format!("{k} {v:+.2}")
                } else {
                    format!("{k} {v:+.0}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What the person changed on a take they kept and exported; None when they kept no take
/// (their edit then says how they edit from scratch, not how they correct the AI).
pub fn correction(h: &History) -> Option<Correction> {
    let kept = &h.takes[h.kept?];
    let take = kept["title"]
        .as_str()
        .or(kept["label"].as_str())
        .unwrap_or("")
        .to_string();
    let moved = h
        .moved
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), v.as_f64()?)))
        .collect();
    Some(Correction { take, moved })
}

/// Per slider: the median change (an unmoved slider counts as 0, so a slider moved on
/// fewer than half the Photos has median 0), shrunk by the evidence, kept if big enough.
fn moves<'a>(cs: impl Iterator<Item = &'a Correction> + Clone) -> BTreeMap<String, f64> {
    let n = cs.clone().count();
    let mut out = BTreeMap::new();
    if n == 0 {
        return out;
    }
    for &(slider, least) in SLIDERS {
        let mut v: Vec<f64> = cs
            .clone()
            .map(|c| c.moved.get(slider).copied().unwrap_or(0.0))
            .collect();
        v.sort_by(f64::total_cmp);
        let median = if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        };
        let m = median * n as f64 / (n as f64 + SHRINK);
        if m.abs() >= least {
            let step = if slider == "exposure" { 100.0 } else { 1.0 };
            out.insert(slider.to_string(), (m * step).round() / step);
        }
    }
    out
}

/// Corrections from a tool argument (`style_profile`'s `corrections`), for benches and tests.
pub fn corrections_from(v: &Value) -> Result<Vec<Correction>> {
    serde_json::from_value(v.clone()).map_err(|e| crate::Error(format!("corrections: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(take: &str, moved: &[(&str, f64)]) -> Correction {
        Correction {
            take: take.into(),
            moved: moved.iter().map(|&(k, v)| (k.to_string(), v)).collect(),
        }
    }

    #[test]
    fn too_few_corrections_learn_nothing() {
        let p = Profile::learn(&vec![c("Natural", &[("exposure", 0.5)]); 2]);
        assert!(p.is_empty());
        assert_eq!(p.photos, 2);
    }

    #[test]
    fn a_habit_on_most_photos_becomes_a_shrunk_move() {
        let mut cs = vec![c("Natural", &[("exposure", 0.4), ("temp", -6.0)]); 6];
        cs.push(c("Natural", &[("exposure", 0.2)]));
        cs.push(c("Natural", &[]));
        let p = Profile::learn(&cs);
        // Median exposure 0.4 over 8, scaled 8/12; temp moved on 6 of 8 too.
        assert_eq!(p.moves["exposure"], 0.27);
        assert_eq!(p.moves["temp"], -4.0);
        assert!(
            p.by_take.is_empty(),
            "one take name: its moves are the overall ones"
        );
    }

    #[test]
    fn a_slider_moved_on_few_photos_or_barely_is_left_alone() {
        let mut cs = vec![c("Natural", &[("vibrance", 1.0)]); 10];
        cs[0].moved.insert("clarity".into(), 40.0);
        cs[1].moved.insert("clarity".into(), 40.0);
        assert!(Profile::learn(&cs).is_empty());
    }

    #[test]
    fn a_take_with_enough_corrections_gets_its_own_moves() {
        let mut cs = vec![c("Natural", &[("exposure", 0.3)]); 6];
        cs.extend(vec![c("Moody", &[("exposure", 0.6), ("blacks", 10.0)]); 6]);
        let p = Profile::learn(&cs);
        assert_eq!(p.for_take("Moody")["exposure"], 0.36);
        assert_eq!(p.for_take("Moody")["blacks"], 6.0);
        assert_eq!(p.for_take("Natural")["exposure"], 0.18);
        assert_eq!(p.moves["exposure"], 0.34);
        // A take it has never seen gets the overall moves.
        assert_eq!(p.for_take("Airy"), &p.moves);
    }
}
