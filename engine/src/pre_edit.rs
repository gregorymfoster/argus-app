//! Edit as you cull: once the person has edited a RAW of a Shoot by hand, every Pick and likely
//! keeper they haven't edited gets a Shoot match ([`crate::shoot_match`]) as a Suggestion,
//! worked out in the background with no model call, so most of a Shoot is edited by the time
//! the cull is done. Nothing changes until the person accepts one (`accept_suggestion`).
//!
//! The hero is the RAW they edited last; its other settings (colour mixer, effects, Look) come
//! along as Lightroom's Sync would bring them, and only the Basic sliders are matched to each
//! frame's own light. Crop, Masks and Spots stay behind. Each new hand edit makes the open
//! Shoot matches stale, and the next run works them out again with the new anchors.

use crate::geometry::Crop;
use crate::library::{Author, Library, Photo, PhotoFilter, PhotoId, ShootId, Verdict};
use crate::recipe::Recipe;
use crate::suggest::{shoot_match_recipe, shoot_match_why, SHOOT_MATCH};
use crate::Result;
use serde::Serialize;

/// Unflagged frames rated this high (by the AI or the person) count as likely keepers.
pub const MIN_STARS: u8 = 4;

/// What one run did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub shoot_id: ShootId,
    /// The hand-edited Photos the matches read, the hero first.
    pub anchors: Vec<PhotoId>,
    /// Photos given a fresh Shoot match.
    pub matched: Vec<PhotoId>,
    /// Photos whose open Shoot match was already up to date, or which hold other matched
    /// takes ("apply to the rest") that a pre-edit mustn't replace.
    pub kept: usize,
}

/// The Shoot's RAWs the person edited by hand, the most recently edited first (the hero).
pub fn hand_edited(lib: &Library, shoot: ShootId) -> Result<Vec<(Photo, String)>> {
    let mut out = Vec::new();
    for p in photos(lib, shoot)? {
        if p.kind != "raw" {
            continue;
        }
        if let Some(v) = lib.read_recipe(p.id)? {
            if v.author == Author::Person {
                out.push((p, v.at));
            }
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(out)
}

/// The frames a pre-edit is for: RAWs that aren't Rejects and that the person hasn't edited,
/// Picked or rated [`MIN_STARS`] and up. Picks first, then by stars, in capture order.
pub fn targets(lib: &Library, shoot: ShootId) -> Result<Vec<Photo>> {
    let mut out = Vec::new();
    for p in photos(lib, shoot)? {
        if p.kind != "raw" || p.verdict == Verdict::Reject {
            continue;
        }
        if p.verdict != Verdict::Pick && p.stars < MIN_STARS {
            continue;
        }
        if lib
            .read_recipe(p.id)?
            .is_some_and(|v| v.author == Author::Person)
        {
            continue;
        }
        out.push(p);
    }
    out.sort_by_key(|p| (p.verdict != Verdict::Pick, std::cmp::Reverse(p.stars)));
    Ok(out)
}

/// Queue a pre-edit of `shoot` when the person has edited one of its RAWs, so it picks up
/// new Picks, new likely keepers and new anchors. One at a time per Shoot; a run that finds
/// everything up to date costs a few reads.
pub fn queue(lib: &mut Library, shoot: ShootId) -> Result<()> {
    if hand_edited(lib, shoot)?.is_empty() {
        return Ok(());
    }
    enqueue(lib, shoot)
}

/// After the person saved a Recipe of `photo`: a hand edit of a RAW sets the Shoot's style.
pub fn queue_after_edit(lib: &mut Library, photo: PhotoId) -> Result<()> {
    match lib.photo(photo)? {
        Some(p) if p.kind == "raw" => enqueue(lib, p.shoot_id),
        _ => Ok(()),
    }
}

fn enqueue(lib: &mut Library, shoot: ShootId) -> Result<()> {
    crate::jobs::enqueue(
        lib,
        "pre_edit_shoot",
        &serde_json::json!({ "shoot_id": shoot }),
        None,
        None,
    )?;
    Ok(())
}

/// Give every target of `shoot` an up-to-date Shoot match; `progress(done, total)` counts them.
/// Runs again while the person's edits keep arriving, so none is missed.
pub fn run(
    lib: &mut Library,
    shoot: ShootId,
    job: Option<i64>,
    progress: &dyn Fn(usize, usize),
) -> Result<Report> {
    let mut report = Report {
        shoot_id: shoot,
        ..Report::default()
    };
    let mut seen = String::new();
    // At most a few passes: an edit made during a pass makes the matches before it stale.
    for _ in 0..3 {
        let edited = hand_edited(lib, shoot)?;
        let Some((hero, newest)) = edited.first().cloned() else {
            return Ok(report);
        };
        if newest == seen {
            break;
        }
        seen = newest.clone();
        let anchors: Vec<Photo> = edited
            .into_iter()
            .take(crate::shoot_match::MAX_ANCHORS)
            .map(|(p, _)| p)
            .collect();
        report.anchors = anchors.iter().map(|p| p.id).collect();
        let all = lib.suggestions(None, Some(shoot), true)?;
        let (todo, kept): (Vec<Photo>, Vec<Photo>) =
            targets(lib, shoot)?.into_iter().partition(|p| {
                let theirs: Vec<_> = all.iter().filter(|s| s.photo_id == p.id).collect();
                let mine: Vec<_> = theirs
                    .iter()
                    .filter(|s| s.status == "open" && s.reference_id.is_some())
                    .collect();
                // Times are to the second, so a match from the edit's own second is stale.
                let fresh =
                    matches!(mine.as_slice(), [s] if s.title == SHOOT_MATCH && s.at > newest);
                let other_takes = mine.iter().any(|s| s.title != SHOOT_MATCH) || mine.len() > 1;
                // A take they accepted, or a Shoot match they dismissed, is their answer.
                let answered = theirs.iter().any(|s| {
                    s.status == "accepted" || (s.status == "dismissed" && s.title == SHOOT_MATCH)
                });
                !(fresh || other_takes || answered)
            });
        report.kept = kept.len();
        if todo.is_empty() {
            continue;
        }
        // Measuring the anchors renders them, so only once there is something to match.
        let samples = crate::shoot_match::samples(lib, &anchors)?;
        if samples.is_empty() {
            return Ok(report);
        }
        let base = sync_base(&crate::tools::current_recipe(lib, hero.id)?);
        let total = todo.len();
        for (i, p) in todo.iter().enumerate() {
            progress(i, total);
            // An odd file costs that frame, not the run.
            let want = match crate::shoot_match::sliders_from(lib, p, &samples) {
                Ok(Some((want, _))) => want,
                Ok(None) => continue,
                Err(e) => {
                    eprintln!("argus pre-edit: Photo {}: {e}", p.id);
                    continue;
                }
            };
            let recipe = shoot_match_recipe(lib, p, &base, &want)?;
            lib.replace_suggestions(
                p.id,
                job,
                Some(hero.id),
                &[(
                    SHOOT_MATCH.to_string(),
                    shoot_match_why(samples.len()),
                    recipe.to_value(),
                    None,
                )],
            )?;
            if !report.matched.contains(&p.id) {
                report.matched.push(p.id);
            }
        }
        progress(total, total);
    }
    Ok(report)
}

/// What a pre-edit starts from: the hero's Recipe less what belongs to that frame alone, as
/// Lightroom's Sync leaves Crop, Masks and Spot removal unticked; one-click fixes and Lens
/// Blur read the hero's own subject, so they stay behind too.
pub fn sync_base(hero: &Recipe) -> Recipe {
    Recipe {
        crop: Crop::default(),
        local: Vec::new(),
        spots: Vec::new(),
        correction: Default::default(),
        lens_blur: Default::default(),
        ..hero.clone()
    }
}

fn photos(lib: &Library, shoot: ShootId) -> Result<Vec<Photo>> {
    lib.list_photos(&PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })
}
