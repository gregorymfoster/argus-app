//! Shoot look: the person edits a hero and the whole Shoot shows the edit as it drags, each
//! frame matched to its own light (Greg, 2026-10-25). It is the Shoot match
//! ([`crate::shoot_match`]) with the hero's Recipe as it stands on screen, saved or not: each
//! frame is measured once (cached), so a slider drag re-matches the Shoot with no rendering
//! and only the small tiles are drawn again.
//!
//! Applying the look ([`apply`], tool `apply_shoot_look`, Greg 2026-11-01) writes that match
//! on the frames the person ticked in Edit's Apply look popover, as one undoable batch. Frames
//! edited by hand are never touched, and each keeps its own crop and spots.

use crate::edit_head::{Sample, SLIDERS};
use crate::library::{Author, BatchId, Library, Photo, PhotoId};
use crate::pre_edit::sync_base;
use crate::recipe::Recipe;
use crate::shoot_match::{self, Model, MAX_ANCHORS};
use crate::suggest::shoot_match_recipe;
use crate::{Error, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;

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
    /// The Recipe the match would write (before the frame's own crop and spots go back on);
    /// None for a hand-edited frame or one that couldn't be matched.
    #[serde(skip)]
    pub recipe: Option<Recipe>,
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
    preview_within(lib, hero, recipe, ids, long_edge, STALL)
}

/// [`preview`], giving up on frames once none has finished for `stall` (see [`STALL`]).
pub fn preview_within(
    lib: &Library,
    hero: &Photo,
    recipe: &Recipe,
    ids: &[PhotoId],
    long_edge: Option<usize>,
    stall: Duration,
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
    let samples = std::sync::Arc::new(samples);

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

    let frames = frames_within(lib.root(), todo, base, samples, smart, long_edge, stall);
    Ok(Preview {
        hero_id: hero.id,
        anchors,
        frames,
    })
}

/// No frame finishing for this long means one is stuck: [`preview`] gives up waiting and
/// returns, the unfinished frames marked as failed (Greg, 2026-11-02: Apply look with Denoise
/// sat at "90 of 101" with nothing saying why).
pub const STALL: Duration = Duration::from_secs(60);
/// Every frame of `todo`, in order, in parallel ([`crate::stall::par_within`]): frames not
/// done once none has finished for `stall` come back failed.
fn frames_within(
    root: &std::path::Path,
    todo: Vec<(Photo, Option<Recipe>)>,
    base: Recipe,
    samples: std::sync::Arc<Vec<Sample>>,
    smart: bool,
    long_edge: Option<usize>,
    stall: Duration,
) -> Vec<Frame> {
    let order: Vec<(PhotoId, bool)> = todo.iter().map(|(p, own)| (p.id, own.is_some())).collect();
    let items = todo
        .into_iter()
        .map(|(p, own)| crate::stall::Item {
            photo_id: p.id,
            file: p.file_name.clone(),
            data: (p, own),
        })
        .collect();
    crate::stall::par_within(
        root,
        "Shoot look",
        items,
        stall,
        move |lib, id, (p, own): (Photo, Option<Recipe>)| {
            let anchor = own.is_some();
            frame(lib, &p, own, &base, &samples, smart, long_edge)
                .unwrap_or_else(|e| failed(id, anchor, e.to_string()))
        },
        |i, why| failed(order[i].0, order[i].1, why),
    )
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
    let (recipe, reach, matched) = match own {
        Some(r) => (r, 1.0, false),
        None if p.kind == "raw" && !samples.is_empty() => {
            let (facts, auto) = shoot_match::measure(lib, p)?;
            let want = shoot_match::matched(&facts, &auto, samples, smart);
            let reach = Model::shipped().reach(&facts, samples);
            (shoot_match_recipe(lib, p, base, &want)?, reach, true)
        }
        // A JPEG, or nothing edited yet: it shows as it is.
        None => (crate::tools::current_recipe(lib, p.id)?, 0.0, false),
    };
    let shown = crate::tools::with_look(lib, p, &recipe)?;
    let values = crate::edit_head::sliders(&shown.global);
    let sliders = if p.kind == "raw" {
        SLIDERS.iter().copied().zip(values).collect()
    } else {
        BTreeMap::new()
    };
    // AI denoise can't be seen in a tile and costs seconds a frame, so the tiles skip it; the
    // Recipe written on Apply keeps it.
    let jpeg = match long_edge {
        Some(e) => {
            let mut quick = shown.clone();
            quick.set("denoise", 0.0, false)?;
            Some(crate::render::encode_jpeg(
                &crate::render::render_in(lib, p, &quick, Some(e))?,
                82,
            )?)
        }
        None => None,
    };
    Ok(Frame {
        photo_id: p.id,
        anchor,
        sliders,
        reach,
        check: !anchor && p.kind == "raw" && !samples.is_empty() && reach < CHECK_BELOW,
        jpeg,
        recipe: matched.then_some(recipe),
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
        recipe: None,
        error: Some(error),
    }
}

/// What [`apply`] did.
#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub batch_id: BatchId,
    /// The frames that got the look.
    pub applied: Vec<PhotoId>,
    /// Frames left as they were, with why (edited by hand, a JPEG, couldn't be measured).
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub photo_id: PhotoId,
    pub why: String,
}

/// Apply the hero's saved look to `ids` (Photos of its Shoot): each frame gets the Shoot match
/// it shows in the preview, keeping its own crop and spots, all in `batch` (one Undo). Frames
/// edited by hand, and the hero, are skipped. Any Shoot match still waiting on an applied frame
/// is closed, since the look it offered is now on the frame.
pub fn apply(lib: &mut Library, hero: &Photo, ids: &[PhotoId], batch: BatchId) -> Result<Applied> {
    let recipe = crate::tools::current_recipe(lib, hero.id)?;
    let ids: Vec<PhotoId> = ids.iter().copied().filter(|&id| id != hero.id).collect();
    let pv = preview(lib, hero, &recipe, &ids, None)?;
    let mut applied = Vec::new();
    let mut skipped = Vec::new();
    for f in pv.frames {
        let why = if f.anchor {
            Some("edited by hand".to_string())
        } else if let Some(e) = &f.error {
            Some(e.clone())
        } else if f.recipe.is_none() {
            Some("not a RAW, or no hand-edited RAW to match".to_string())
        } else {
            None
        };
        if let Some(why) = why {
            skipped.push(Skipped {
                photo_id: f.photo_id,
                why,
            });
            continue;
        }
        let mut r = f.recipe.expect("checked above");
        let own = crate::tools::current_recipe(lib, f.photo_id)?;
        r.crop = own.crop;
        r.spots = own.spots;
        lib.write_recipe(f.photo_id, &r.to_value(), Author::Ai, Some(batch))?;
        lib.close_shoot_match(f.photo_id)?;
        lib.log_usage(
            f.photo_id,
            "look_applied",
            serde_json::json!({ "hero_id": hero.id, "batch_id": batch, "reach": f.reach, "check": f.check }),
        );
        applied.push(f.photo_id);
    }
    Ok(Applied {
        batch_id: batch,
        applied,
        skipped,
    })
}
