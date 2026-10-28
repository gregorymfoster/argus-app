//! Crop like this (Greg, 2026-10-27): after the person crops a Photo by hand, every frame of
//! its Shoot with the same shape, shot within [`WINDOW`] seconds of it, gets that crop as a
//! Suggestion. Nothing changes until they accept one, and Accept all is one Undo.
//!
//! Greg crops few frames (6%) but in runs around a moment, and around a moment he reuses
//! nearly the same rectangle: copying it as is matched his own crops best (IoU 0.72, no gain
//! from following the scene's movement), and he cropped 39% of the frames it would be
//! offered to (`docs/research/recompose.md`). So the crop is copied, never placed by a model.
//!
//! An offer is its own take titled [`PREFIX`] and the hero's name, matched to the hero. A
//! frame with a Shoot match ready gets the crop inside that take instead, so Accept all takes
//! both at once and a later pre-edit carries it on ([`carried`]). A frame already cropped, or
//! one where the person said Not for these to this hero's crop, is left alone. Removing the
//! hero's crop takes its open offers back.

use crate::geometry::Crop;
use crate::library::time::capture_seconds;
use crate::library::{Library, Photo, PhotoFilter, PhotoId, Suggestion, Verdict};
use crate::recipe::Recipe;
use crate::suggest::SHOOT_MATCH;
use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;

/// How far apart in capture time a frame may be from the hero to be offered its crop.
pub const WINDOW: i64 = 120;
/// The start of an offer's title, followed by the hero's file name less its extension.
pub const PREFIX: &str = "Crop like ";

/// A take that only offers a crop.
pub fn is_offer(title: &str) -> bool {
    title.starts_with(PREFIX)
}

/// What one offer did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub hero_id: PhotoId,
    /// The hero's crop as offered; none when it has no crop (its offers were taken back).
    pub crop: Option<Crop>,
    /// Frames given (or still holding) the crop as its own take.
    pub offered: Vec<PhotoId>,
    /// Frames whose open Shoot match now carries the crop.
    pub joined: Vec<PhotoId>,
    /// Frames whose open offer from this hero was taken back.
    pub withdrawn: Vec<PhotoId>,
}

/// After the person saved `recipe` on `photo`: offer its crop to the frames around it, or take
/// the offers back when it has none. Runs inside the save, so it only reads and writes rows.
pub fn after_edit(lib: &mut Library, photo: PhotoId, recipe: &Value) -> Result<()> {
    let cropped = recipe.get("crop").is_some();
    // Most saves aren't crops: only look further when there is a crop or an offer to take back.
    if !cropped && !has_open_offers(lib, photo)? {
        return Ok(());
    }
    offer(lib, photo).map(|_| ())
}

/// Offer `hero`'s current crop to the frames around it (see the module docs).
pub fn offer(lib: &mut Library, hero: PhotoId) -> Result<Report> {
    let h = lib
        .photo(hero)?
        .ok_or_else(|| Error(format!("no Photo with id {hero}")))?;
    let crop = match lib.read_recipe(hero)? {
        Some(v) => Recipe::from_value(&v.recipe)?.crop,
        None => Crop::default(),
    };
    let name = stem(&h.file_name);
    let mut report = Report {
        hero_id: hero,
        crop: (!crop.is_identity()).then_some(crop),
        ..Report::default()
    };
    let all = lib.suggestions(None, Some(h.shoot_id), true)?;
    let mark = marker(name);
    if crop.is_identity() {
        for s in all.iter().filter(|s| s.status == "open") {
            if is_offer(&s.title) && s.reference_id == Some(hero) {
                lib.withdraw_suggestion(s.id)?;
                report.withdrawn.push(s.photo_id);
            } else if s.title == SHOOT_MATCH && s.note.contains(&mark) {
                let mut r = s.recipe.clone();
                if let Some(o) = r.as_object_mut() {
                    o.remove("crop");
                }
                lib.update_suggestion(s.id, s.note.replace(&mark, "").trim_end(), &r)?;
                report.withdrawn.push(s.photo_id);
            }
        }
        return Ok(report);
    }
    let title = format!("{PREFIX}{name}");
    let crop_value = serde_json::to_value(crop).map_err(|e| Error(e.to_string()))?;
    for n in neighbours(lib, &h)? {
        let theirs: Vec<&Suggestion> = all.iter().filter(|s| s.photo_id == n.id).collect();
        // Not for these: a frame that turned down this hero's crop keeps its answer.
        if theirs
            .iter()
            .any(|s| s.status == "dismissed" && is_offer(&s.title) && s.reference_id == Some(hero))
        {
            continue;
        }
        let current = lib.read_recipe(n.id)?;
        if current
            .as_ref()
            .is_some_and(|v| v.recipe.get("crop").is_some())
        {
            continue; // already framed, by the person or an accepted offer
        }
        let open: Vec<&&Suggestion> = theirs.iter().filter(|s| s.status == "open").collect();
        let offers: Vec<&&&Suggestion> = open.iter().filter(|s| is_offer(&s.title)).collect();
        let shoot_match = open
            .iter()
            .find(|s| s.title == SHOOT_MATCH && s.reference_id.is_some());
        if let Some(m) = shoot_match {
            let mut r = m.recipe.clone();
            if let Some(o) = r.as_object_mut() {
                o.insert("crop".into(), crop_value.clone());
            }
            let note = format!("{}{mark}", strip_marker(&m.note));
            lib.update_suggestion(m.id, &note, &r)?;
            for s in offers {
                lib.withdraw_suggestion(s.id)?;
            }
            report.joined.push(n.id);
            continue;
        }
        // Already offered as it stands: leave it (and its place among the takes).
        if let [s] = offers.as_slice() {
            if s.reference_id == Some(hero) && s.recipe.get("crop") == Some(&crop_value) {
                report.offered.push(n.id);
                continue;
            }
        }
        for s in offers {
            lib.withdraw_suggestion(s.id)?;
        }
        let mut r = match current {
            Some(v) => v.recipe,
            None => Recipe::default().to_value(),
        };
        if let Some(o) = r.as_object_mut() {
            o.insert("crop".into(), crop_value.clone());
        }
        let secs = (capture_seconds(&n.captured_at) - capture_seconds(&h.captured_at)).abs();
        let note = format!(
            "The crop you gave {name} ({}% of the frame), for this frame shot {secs} s from it. Around a moment you crop the frames alike about 2 times in 5.",
            (crop.w * crop.h * 100.0).round()
        );
        lib.add_suggestion(n.id, Some(hero), &title, &note, &r)?;
        report.offered.push(n.id);
    }
    Ok(report)
}

/// The frames a crop of `hero` goes to: the rest of its Shoot that isn't a Reject, the same
/// shape (orientation and aspect), shot within [`WINDOW`] seconds of it.
pub fn neighbours(lib: &Library, hero: &Photo) -> Result<Vec<Photo>> {
    let t = capture_seconds(&hero.captured_at);
    let aspect = |p: &Photo| p.width as f64 / p.height.max(1) as f64;
    Ok(lib
        .list_photos(&PhotoFilter {
            shoot: Some(hero.shoot_id),
            verdict: None,
        })?
        .into_iter()
        .filter(|p| {
            p.id != hero.id
                && p.verdict != Verdict::Reject
                && (aspect(p) - aspect(hero)).abs() < 0.02
                && (capture_seconds(&p.captured_at) - t).abs() <= WINDOW
        })
        .collect())
}

/// The crop an open take of this frame carries for a hero (an offer, or one inside a Shoot
/// match), with the words that say so, for a fresh Shoot match to keep.
pub fn carried(open: &[&Suggestion]) -> Option<(Value, String)> {
    open.iter().find_map(|s| {
        let crop = s.recipe.get("crop")?.clone();
        let words = if is_offer(&s.title) {
            marker(s.title.trim_start_matches(PREFIX))
        } else if s.title == SHOOT_MATCH {
            s.note.find(" With ").map(|i| s.note[i..].to_string())?
        } else {
            return None;
        };
        Some((crop, words))
    })
}

fn has_open_offers(lib: &Library, hero: PhotoId) -> Result<bool> {
    let Some(h) = lib.photo(hero)? else {
        return Ok(false);
    };
    let mark = marker(stem(&h.file_name));
    Ok(lib
        .suggestions(None, Some(h.shoot_id), false)?
        .iter()
        .any(|s| {
            s.status == "open"
                && ((s.reference_id == Some(hero) && is_offer(&s.title)) || s.note.contains(&mark))
        }))
}

/// What a Shoot match's words gain when it carries a hero's crop.
fn marker(name: &str) -> String {
    format!(" With {name}'s crop.")
}

fn strip_marker(note: &str) -> &str {
    match note.find(" With ") {
        Some(i) if note.ends_with("'s crop.") => &note[..i],
        _ => note,
    }
}

fn stem(file: &str) -> &str {
    file.rsplit_once('.').map_or(file, |(s, _)| s)
}
