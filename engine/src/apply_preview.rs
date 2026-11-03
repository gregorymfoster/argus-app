//! The Apply popover's tiles for edits other than Apply look (Greg, 2026-11-02: "I love the
//! pop-over 'Apply to all' kind of experience. I think this should be my average experience
//! when applying edits to everything"). Before an edit reaches many Photos, the popover draws
//! each one as it will look; [`preview`] works out what the edit would write on each Photo,
//! exactly as the tool that applies it does, and renders it small, without saving anything.
//!
//! The edits so far are a [`Change`]: Paste (and Sync, and Cull's Apply this edit), which the
//! `paste_settings` tool applies, Auto, which `auto_edit` applies, and given Recipes (Cull's A,
//! the first AI take on each Photo, which `accept_suggestion` applies). Auto's measurement is
//! remembered, so Apply after the preview doesn't measure each Photo again.

use crate::library::{Author, Library, Photo, PhotoId};
use crate::recipe::Recipe;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// An edit the Apply popover previews.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    /// `groups` of `from`'s settings (all of them when None), as `paste_settings` writes them.
    Paste {
        from: PhotoId,
        groups: Option<Vec<String>>,
    },
    /// Lightroom's Auto, as `auto_edit` writes it.
    Auto,
    /// Sync to Look from `from`: every Photo under the Look as the synced Look leaves it.
    SyncLook { from: PhotoId },
    /// A Recipe per Photo, as given: the AI takes Cull's A accepts, each Photo's first.
    Recipes {
        /// By Photo id; string keys, since a tagged enum can't read number keys.
        recipes: std::collections::BTreeMap<String, serde_json::Value>,
    },
}

/// One Photo as the edit leaves it.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub photo_id: PhotoId,
    /// Its latest Version is the person's own and changes something: the popover leaves it
    /// out until they tick it.
    pub hand: bool,
    /// The AI Denoise Amount Auto sets on it (a noisy RAW), which no tile can show.
    pub denoise: Option<f64>,
    /// The render at `long_edge`, as JPEG; skipped in JSON (the HTTP command sends it as a
    /// data URL).
    #[serde(skip)]
    pub jpeg: Option<Vec<u8>>,
    pub error: Option<String>,
}

/// No Photo finishing for this long means one is stuck (see [`crate::stall`]).
pub const STALL: Duration = Duration::from_secs(60);

/// Every Photo of `ids` as `change` would leave it, drawn at `long_edge` (the source of a
/// Paste is left out). Nothing is saved.
pub fn preview(
    lib: &Library,
    change: &Change,
    ids: &[PhotoId],
    long_edge: usize,
) -> Result<Vec<Frame>> {
    preview_within(lib, change, ids, long_edge, STALL)
}

/// [`preview`], giving up on Photos once none has finished for `stall`.
pub fn preview_within(
    lib: &Library,
    change: &Change,
    ids: &[PhotoId],
    long_edge: usize,
    stall: Duration,
) -> Result<Vec<Frame>> {
    let mut items = Vec::new();
    match change {
        Change::Paste { from, groups } => {
            let groups = crate::paste::groups(groups.as_deref())?;
            for (id, r) in crate::tools::pasted(lib, *from, ids, &groups)? {
                items.push(item(lib, id, Some(r))?);
            }
        }
        Change::Auto => {
            for &id in ids {
                items.push(item(lib, id, None)?);
            }
        }
        Change::SyncLook { from } => {
            let s = crate::tools::synced(lib, *from)?;
            for &id in ids.iter().filter(|&&id| id != *from) {
                // As it will render: the new Look under its own sliders.
                let mut r = lib.current_recipe(id)?;
                r.global = s.look.adjust.plus(&r.global);
                r.look = None;
                items.push(item(lib, id, Some(r))?);
            }
        }
        Change::Recipes { recipes } => {
            for &id in ids {
                let v = recipes
                    .get(&id.to_string())
                    .ok_or_else(|| Error(format!("no Recipe given for Photo {id}")))?;
                items.push(item(lib, id, Some(Recipe::from_value(v)?))?);
            }
        }
    }
    let order: Vec<(PhotoId, bool)> = items.iter().map(|i| (i.photo_id, i.data.1)).collect();
    Ok(crate::stall::par_within(
        lib.root(),
        match change {
            Change::Paste { .. } => "Paste preview",
            Change::Auto => "Auto preview",
            Change::Recipes { .. } => "Takes preview",
            Change::SyncLook { .. } => "Sync preview",
        },
        items,
        stall,
        move |lib, id, (p, hand, pasted): (Photo, bool, Option<Recipe>)| {
            frame(lib, &p, hand, pasted, long_edge).unwrap_or_else(|e| Frame {
                photo_id: id,
                hand,
                denoise: None,
                jpeg: None,
                error: Some(e.to_string()),
            })
        },
        |i, why| Frame {
            photo_id: order[i].0,
            hand: order[i].1,
            denoise: None,
            jpeg: None,
            error: Some(why),
        },
    ))
}

type Data = (Photo, bool, Option<Recipe>);

fn item(lib: &Library, id: PhotoId, pasted: Option<Recipe>) -> Result<crate::stall::Item<Data>> {
    let p = lib
        .photo(id)?
        .ok_or_else(|| Error(format!("no Photo with id {id}")))?;
    // An Undo back to nothing is the person's Version too, but nothing of theirs to keep.
    let hand = match lib.read_recipe(id)? {
        Some(v) if v.author == Author::Person => {
            Recipe::from_value(&v.recipe)?.to_value() != Recipe::default().to_value()
        }
        _ => false,
    };
    Ok(crate::stall::Item {
        photo_id: id,
        file: p.file_name.clone(),
        data: (p, hand, pasted),
    })
}

fn frame(
    lib: &Library,
    p: &Photo,
    hand: bool,
    pasted: Option<Recipe>,
    long_edge: usize,
) -> Result<Frame> {
    let mut denoise = None;
    let recipe = match pasted {
        Some(r) => r,
        None => {
            let own = lib.current_recipe(p.id)?;
            let input = crate::tools::auto_input(lib, p, &own)?;
            let mut r = crate::tools::auto_from(p, &input)?;
            // As auto_edit does when it saves.
            denoise = crate::denoise::auto_fill(lib, p, &mut r)?;
            r
        }
    };
    // AI denoise can't be seen in a tile and costs seconds a Photo, so tiles skip it.
    let mut shown = lib.with_look(p, &recipe)?;
    shown.set("denoise", 0.0, false)?;
    let img = crate::render::render_in(lib, p, &shown, Some(long_edge))?;
    Ok(Frame {
        photo_id: p.id,
        hand,
        denoise,
        jpeg: Some(crate::render::encode_jpeg(&img, 82)?),
        error: None,
    })
}
