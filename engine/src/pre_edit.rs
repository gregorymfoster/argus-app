//! Shoot match Suggestions for a Shoot ([`crate::shoot_match`]), no model call, when asked for
//! (tool `pre_edit_shoot`). Until 2026-11-02 this ran by itself after every hand edit ("edit as
//! you cull"); Greg chose one way to spread a look instead, Edit's Apply look
//! ([`crate::shoot_look`]), so nothing queues it now. Nothing changes until the person accepts one.
//!
//! The hero is the RAW they edited last; its other settings (colour mixer, effects, Look) come
//! along as Lightroom's Sync would bring them, and only the Basic sliders are matched to each
//! frame's own light. Crop, Masks and Spots stay behind, but a crop offered to the frame
//! ([`crate::crop_offer`]) rides along in its Shoot match. Each new hand edit makes the open
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
/// Picked or rated [`MIN_STARS`] and up (all of them when `every`, the Shoot look locked).
/// Picks first, then by stars, in capture order.
pub fn targets(lib: &Library, shoot: ShootId, every: bool) -> Result<Vec<Photo>> {
    let mut out = Vec::new();
    for p in photos(lib, shoot)? {
        if p.kind != "raw" || p.verdict == Verdict::Reject {
            continue;
        }
        if !every && p.verdict != Verdict::Pick && p.stars < MIN_STARS {
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

/// Give every target of `shoot` an up-to-date Shoot match; `progress(done, total)` counts them.
/// Runs again while the person's edits keep arriving, so none is missed. With `every` (Lock
/// look, [`crate::shoot_look`]) every frame that isn't a Reject gets one, and a Shoot match
/// they accepted or dismissed before is worked out again for the look they locked.
pub fn run(
    lib: &mut Library,
    shoot: ShootId,
    every: bool,
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
            targets(lib, shoot, every)?.into_iter().partition(|p| {
                let theirs: Vec<_> = all.iter().filter(|s| s.photo_id == p.id).collect();
                // A crop offer isn't a match: the Shoot match takes its crop along.
                let mine: Vec<_> = theirs
                    .iter()
                    .filter(|s| {
                        s.status == "open"
                            && s.reference_id.is_some()
                            && !crate::crop_offer::is_offer(&s.title)
                    })
                    .collect();
                // Times are to the second, so a match from the edit's own second is stale.
                let fresh =
                    matches!(mine.as_slice(), [s] if s.title == SHOOT_MATCH && s.at > newest);
                let other_takes = mine.iter().any(|s| s.title != SHOOT_MATCH) || mine.len() > 1;
                // A take they accepted, or a Shoot match they dismissed, is their answer.
                let answered = !every
                    && theirs.iter().any(|s| {
                        s.status == "accepted"
                            || (s.status == "dismissed" && s.title == SHOOT_MATCH)
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
            let mut recipe = shoot_match_recipe(lib, p, &base, &want)?.to_value();
            let mut why = shoot_match_why(samples.len());
            // A crop offered to this frame (Crop like this) stays with its Shoot match.
            let open: Vec<_> = all
                .iter()
                .filter(|s| s.photo_id == p.id && s.status == "open" && s.reference_id.is_some())
                .collect();
            if let (Some((crop, words)), Some(o)) =
                (crate::crop_offer::carried(&open), recipe.as_object_mut())
            {
                o.insert("crop".into(), crop);
                why.push_str(&words);
            }
            lib.replace_suggestions(
                p.id,
                job,
                Some(hero.id),
                &[(SHOOT_MATCH.to_string(), why, recipe, None)],
            )?;
            if !report.matched.contains(&p.id) {
                report.matched.push(p.id);
            }
        }
        progress(total, total);
    }
    Ok(report)
}

/// The Shoot's look was locked (Lock look): from then on every pre-edit of it covers every
/// frame that isn't a Reject, so a later hand edit re-matches the whole Shoot.
pub fn locked(lib: &Library, shoot: ShootId) -> Result<bool> {
    Ok(lib
        .setting(&lock_key(shoot))?
        .is_some_and(|v| v.as_bool() == Some(true)))
}

/// Remember that `shoot`'s look is locked (see [`locked`]).
pub fn lock(lib: &Library, shoot: ShootId) -> Result<()> {
    lib.set_setting(&lock_key(shoot), &serde_json::Value::Bool(true))
}

fn lock_key(shoot: ShootId) -> String {
    format!("shoot_look_locked:{shoot}")
}

/// What a pre-edit and Apply look start from: the hero's Recipe less what belongs to that
/// frame alone, as Lightroom's Sync leaves Crop, Spot removal and drawn Masks unticked. Fixes
/// that find their own target in each Photo carry over (Greg, 2026-11-01: "brighten faces ...
/// apply it intelligently to all the photos"): Masks of faces, people, skin, hair, sky,
/// subject, background, a luminance range or a frame-edge gradient are worked out again on
/// every frame, as is the face exposure. Brushes, Linear and Radial Masks, colour picks, a
/// Mask on one numbered face or person, and Lens Blur (its focus is a point in the hero) stay.
pub fn sync_base(hero: &Recipe) -> Recipe {
    Recipe {
        crop: Crop::default(),
        local: hero
            .local
            .iter()
            .filter(|l| finds_its_own(&l.mask))
            .cloned()
            .collect(),
        spots: Vec::new(),
        correction: crate::recipe::Correction {
            face: hero.correction.face,
            ..Default::default()
        },
        lens_blur: Default::default(),
        ..hero.clone()
    }
}

/// A Mask that each Photo works out for itself, so it means the same on any frame.
pub fn finds_its_own(m: &crate::masks::Mask) -> bool {
    use crate::masks::Shape::*;
    let own = match &m.shape {
        Subject | Background | Skin | Hair | Sky | Luminance { .. } | Gradient { .. } => true,
        Faces { face } => face.is_none(),
        Person { person, .. } => person.is_none(),
        Brush { .. } | Linear { .. } | Radial { .. } | Color { .. } => false,
    };
    own && m.components.iter().all(|c| finds_its_own(&c.mask))
}

fn photos(lib: &Library, shoot: ShootId) -> Result<Vec<Photo>> {
    lib.list_photos(&PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })
}
