//! Shoot look: the person edits a hero and the whole Shoot shows the edit as it drags, each
//! frame matched to its own light (Greg, 2026-10-25). It is the Shoot match
//! ([`crate::shoot_match`]) with the hero's Recipe as it stands on screen, saved or not: each
//! frame is measured once (cached), so a slider drag re-matches the Shoot with no rendering
//! and only the small tiles are drawn again.
//!
//! Locking the look is `pre_edit_shoot` with `every`: a Shoot match Suggestion, ready to
//! accept, for every frame that isn't a Reject.

use crate::edit_head::{Sample, SLIDERS};
use crate::library::{Author, Library, Photo, PhotoId};
use crate::pre_edit::sync_base;
use crate::recipe::Recipe;
use crate::shoot_match::{self, Model, MAX_ANCHORS};
use crate::suggest::shoot_match_recipe;
use crate::{Error, Result};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;

/// A frame the anchors cover less than this is marked to check: its scene is unlike every
/// hand-edited frame, so the match leans on the Edit head.
pub const CHECK_BELOW: f64 = 0.5;

/// One frame of the Shoot as it will look.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub photo_id: PhotoId,
    /// Edited by hand (the hero among them): it shows its own Recipe.
    pub anchor: bool,
    /// The Basic sliders it gets, as they render (Look included); empty when it isn't a
    /// RAW or couldn't be measured.
    pub sliders: BTreeMap<&'static str, f64>,
    /// How well the hand-edited frames cover its scene, 0 to 1.
    pub reach: f64,
    /// Its scene is unlike every hand-edited frame: worth a look before accepting.
    pub check: bool,
    /// The render at `long_edge`, as JPEG; skipped in JSON (the HTTP command sends it as a
    /// data URL).
    #[serde(skip)]
    pub jpeg: Option<Vec<u8>>,
    pub error: Option<String>,
}

/// The Shoot as it will look with the hero edited as `recipe`.
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub hero_id: PhotoId,
    /// The hand-edited frames the match reads, the hero first.
    pub anchors: Vec<PhotoId>,
    pub frames: Vec<Frame>,
}

/// Matches `ids` (Photos of the hero's Shoot) to the hero edited as `recipe` and the Shoot's
/// other hand edits, rendering each at `long_edge` when given. Frames are measured the first
/// time they are seen (a RAW decode each, in parallel), then from memory.
pub fn preview(
    lib: &Library,
    hero: &Photo,
    recipe: &Recipe,
    ids: &[PhotoId],
    long_edge: Option<usize>,
) -> Result<Preview> {
    let others: Vec<Photo> = shoot_match::anchors_of(lib, hero.shoot_id, hero.id, hero.id)?
        .into_iter()
        .filter(|p| p.id != hero.id)
        .take(MAX_ANCHORS - 1)
        .collect();
    let mut samples: Vec<Sample> = Vec::new();
    let mut anchors = Vec::new();
    if hero.kind == "raw" {
        if let Some(s) = shoot_match::sample_with(lib, hero, recipe)? {
            samples.push(s);
            anchors.push(hero.id);
        }
    }
    for p in &others {
        if let Some(s) =
            shoot_match::sample_with(lib, p, &crate::tools::current_recipe(lib, p.id)?)?
        {
            samples.push(s);
            anchors.push(p.id);
        }
    }
    let smart = crate::jobs::config(lib)?.shoot_match_smart;
    let base = sync_base(recipe);

    // What each frame shows before rendering: the hero as dragged, other hand edits as saved.
    let mut todo: Vec<(Photo, Option<Recipe>)> = Vec::new();
    for &id in ids {
        let p = lib
            .photo(id)?
            .ok_or_else(|| Error(format!("no Photo with id {id}")))?;
        if p.shoot_id != hero.shoot_id {
            return Err(Error(format!("Photo {id} isn't in the hero's Shoot")));
        }
        let own = if p.id == hero.id {
            Some(recipe.clone())
        } else if matches!(lib.read_recipe(p.id)?, Some(v) if v.author == Author::Person) {
            Some(crate::tools::current_recipe(lib, p.id)?)
        } else {
            None
        };
        todo.push((p, own));
    }

    let root = lib.root().to_path_buf();
    let frames = todo
        .into_par_iter()
        .map_init(
            || Library::open(&root),
            |lib, (p, own)| {
                let lib = match lib {
                    Ok(l) => l,
                    Err(e) => return failed(p.id, own.is_some(), e.to_string()),
                };
                frame(lib, &p, own, &base, &samples, smart, long_edge)
                    .unwrap_or_else(|e| failed(p.id, false, e.to_string()))
            },
        )
        .collect();
    Ok(Preview {
        hero_id: hero.id,
        anchors,
        frames,
    })
}

fn frame(
    lib: &Library,
    p: &Photo,
    own: Option<Recipe>,
    base: &Recipe,
    samples: &[Sample],
    smart: bool,
    long_edge: Option<usize>,
) -> Result<Frame> {
    let anchor = own.is_some();
    let (recipe, reach) = match own {
        Some(r) => (r, 1.0),
        None if p.kind == "raw" && !samples.is_empty() => {
            let (facts, auto) = shoot_match::measure(lib, p)?;
            let want = shoot_match::matched(&facts, &auto, samples, smart);
            let reach = Model::shipped().reach(&facts, samples);
            (shoot_match_recipe(lib, p, base, &want)?, reach)
        }
        // A JPEG, or nothing edited yet: it shows as it is.
        None => (crate::tools::current_recipe(lib, p.id)?, 0.0),
    };
    let shown = crate::tools::with_look(lib, p, &recipe)?;
    let values = crate::edit_head::sliders(&shown.global);
    let sliders = if p.kind == "raw" {
        SLIDERS.iter().copied().zip(values).collect()
    } else {
        BTreeMap::new()
    };
    let jpeg = match long_edge {
        Some(e) => Some(crate::render::encode_jpeg(
            &crate::render::render_in(lib, p, &shown, Some(e))?,
            82,
        )?),
        None => None,
    };
    Ok(Frame {
        photo_id: p.id,
        anchor,
        sliders,
        reach,
        check: !anchor && p.kind == "raw" && !samples.is_empty() && reach < CHECK_BELOW,
        jpeg,
        error: None,
    })
}

fn failed(photo_id: PhotoId, anchor: bool, error: String) -> Frame {
    Frame {
        photo_id,
        anchor,
        sliders: BTreeMap::new(),
        reach: 0.0,
        check: false,
        jpeg: None,
        error: Some(error),
    }
}
