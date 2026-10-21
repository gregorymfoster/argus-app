//! The cull's taste stage: star Ratings, Best Photos and the order inside each burst, from the
//! Facts alone (no model). The model's stage of a cull checks only for technical failures,
//! which a local model calls reliably; whether a sound frame is one the person keeps is taste,
//! where Qwen ranked Greg's keepers at chance (Greg 2026-10-20, docs/research/cull-taste.md).
//!
//! Taste here is the faces: on his clean 2026-10-10 card, how much the main faces smile and
//! whether any looks off ([`eyes::awkward`]) ranked his keepers at AUC .65, where Qwen's stars
//! scored .54 and Claude Opus's .66. Sharpness and exposure didn't rank keepers at all; they
//! are technical, so they stay with the model's stage. A frame without a readable face scores
//! [`NO_FACES`], so a Shoot with no people gets level stars rather than invented ones.
//!
//! Each stage fills only what the one before left empty, so a model asked for the whole cull
//! (a harness variant's own prompt, [`crate::cull::Taste::Model`]) keeps its own stars.

use crate::analyzers::{eyes, Facts};
use crate::cull::{ContactSheet, CullPhoto, CullResult};
use crate::library::Verdict;
use std::collections::HashMap;

/// Taste of a frame with no main face: between a frame where someone looks off and one where
/// everyone smiles.
pub const NO_FACES: f32 = 0.0;

/// How likely the person keeps a frame, from -1 to 1: the main faces' mean smile minus the share
/// of them that look off. [`NO_FACES`] when no main face was read.
pub fn score(facts: &Facts) -> f32 {
    let reads = facts.get("eyes").map(eyes::read).unwrap_or_default();
    let main = eyes::main_faces(&reads);
    if main.is_empty() {
        return NO_FACES;
    }
    let n = main.len() as f32;
    let smile = main.iter().map(|r| eyes::smile(r)).sum::<f32>() / n;
    let off = main.iter().filter(|r| !eyes::awkward(r).is_empty()).count() as f32 / n;
    smile - off
}

/// A frame this much darker than the median of the frames shot within [`NEAR_S`] of it is a
/// misjudged exposure of the same moment: 2 EV under is well below this, while on Greg's cards
/// only 4 of 661 frames fall under it, none of them keepers. The Technical check leaves such a
/// frame alone (a RAW can be rescued), so Taste ranks it below its neighbours instead.
const DARKER: f32 = 0.65;
const NEAR_S: i64 = 30;

/// What a frame's dark exposure costs: enough to put it below any neighbour that tastes the same.
const DARK_COST: f32 = 1.0;

/// Each Photo's taste, less [`DARK_COST`] when it is much darker than its neighbours in time.
fn tastes(sheet: &ContactSheet) -> HashMap<usize, f32> {
    let at: Vec<i64> = sheet
        .photos
        .iter()
        .map(|p| crate::cull::seconds(&p.captured_at))
        .collect();
    sheet
        .photos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut near: Vec<f32> = (0..sheet.photos.len())
                .filter(|&j| j != i && (at[j] - at[i]).abs() <= NEAR_S)
                .map(|j| sheet.photos[j].signals.brightness)
                .collect();
            near.sort_by(f32::total_cmp);
            let dark = near
                .get(near.len() / 2)
                .is_some_and(|&m| p.signals.brightness < DARKER * m);
            (p.n, p.taste - if dark { DARK_COST } else { 0.0 })
        })
        .collect()
}

/// Stars for a frame the technical stage kept, from where its taste falls in the Shoot (`q`: the
/// share of kept frames that taste better, ties counting half): the top tenth 5, the next quarter
/// 4, then 3, and the bottom 30% 2. A Shoot where every frame tastes the same is all 3s.
fn stars(q: f32) -> u8 {
    match q {
        q if q < 0.10 => 5,
        q if q < 0.35 => 4,
        q if q < 0.70 => 3,
        _ => 2,
    }
}

/// Fill in what the model's stage left empty: stars (1 for a reject, the rest by taste), the
/// Stacks of bursts with the best-tasting frame first, and about one Best Photo in ten from the
/// frames that taste better than the Shoot's middle, never two from one burst.
pub fn rate(sheet: &ContactSheet, mut result: CullResult) -> CullResult {
    let by_n: HashMap<usize, &crate::cull::SheetPhoto> =
        sheet.photos.iter().map(|p| (p.n, p)).collect();
    let tasted = tastes(sheet);
    let taste = |c: &CullPhoto| tasted.get(&c.n).copied().unwrap_or(NO_FACES);
    let sharp = |c: &CullPhoto| by_n.get(&c.n).map_or(0, |p| p.sharpness_pct);
    let kept: Vec<f32> = result
        .photos
        .iter()
        .filter(|c| c.verdict != Verdict::Reject)
        .map(taste)
        .collect();
    let m = kept.len().max(1) as f32;
    for c in &mut result.photos {
        // "ok" is the Technical check's word for a sound frame; the grid needs no reason for it.
        if c.verdict != Verdict::Reject && c.reason.eq_ignore_ascii_case("ok") {
            c.reason.clear();
        }
        if c.stars.is_some() {
            continue;
        }
        c.stars = Some(if c.verdict == Verdict::Reject {
            1
        } else {
            let t = taste(c);
            let better = kept.iter().filter(|&&k| k > t).count() as f32;
            let level = kept.iter().filter(|&&k| k == t).count() as f32 - 1.0;
            stars((better + level / 2.0) / m)
        });
    }
    // Best first: taste, then the sharper frame.
    let order = |a: &&mut CullPhoto, b: &&mut CullPhoto| {
        taste(b)
            .total_cmp(&taste(a))
            .then(sharp(b).cmp(&sharp(a)))
            .then(a.n.cmp(&b.n))
    };
    if result.photos.iter().all(|c| c.stack.is_none()) {
        let mut bursts: HashMap<usize, Vec<&mut CullPhoto>> = HashMap::new();
        for c in &mut result.photos {
            if let Some(b) = by_n.get(&c.n).and_then(|p| p.burst) {
                bursts.entry(b).or_default().push(c);
            }
        }
        for (b, mut frames) in bursts {
            frames.sort_by(order);
            for (rank, c) in frames.into_iter().enumerate() {
                c.stack = Some(b as u32 + 1);
                c.rank = Some(rank as u32 + 1);
            }
        }
    }
    if result.photos.iter().all(|c| c.best.is_none()) {
        let mut sorted = kept.clone();
        sorted.sort_by(f32::total_cmp);
        let middle = sorted.get(sorted.len() / 2).copied().unwrap_or(NO_FACES);
        let target = (sheet.photos.len() / 10).max(1);
        let mut cands: Vec<&mut CullPhoto> = result
            .photos
            .iter_mut()
            .filter(|c| c.verdict != Verdict::Reject && taste(c) > middle)
            .collect();
        cands.sort_by(order);
        let mut used = std::collections::HashSet::new();
        let mut rank = 0u32;
        for c in cands {
            if rank as usize >= target {
                break;
            }
            if by_n
                .get(&c.n)
                .and_then(|p| p.burst)
                .is_some_and(|b| !used.insert(b))
            {
                continue;
            }
            rank += 1;
            c.best = Some(rank);
        }
    }
    result
}
