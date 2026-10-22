//! The cull's taste stage: star Ratings, Best Photos and the order inside each burst, from the
//! Facts alone (no model). The model's stage of a cull checks only for technical failures,
//! which a local model calls reliably; whether a sound frame is one the person keeps is taste,
//! where Qwen ranked Greg's keepers at chance (Greg 2026-10-20, docs/research/cull-taste.md).
//!
//! Taste here is the faces, and the person's own flags once there are enough of them. On his
//! clean 2026-10-10 card, how much the main faces smile and whether any looks off
//! ([`eyes::awkward`]) ranked his keepers at AUC .65, where Qwen's stars scored .54 and Claude
//! Opus's .66. Sharpness and exposure didn't rank keepers at all; they are technical, so they
//! stay with the model's stage. A frame without a readable face scores [`NO_FACES`], so a
//! Shoot with no people and no flags to learn from gets level stars rather than invented ones.
//!
//! The [`Head`] learns from every Pick and Reject the person made themselves: the direction in
//! the image embedding from what they reject to what they pick. Trained on his four other bench
//! Shoots, it lifted the faces' AUC on 2026-10-10 from .65 to .69 and his share of the Best
//! Photos from 38% to 77%; trained on the first 80 frames he flagged of the card itself, .74
//! (`bench/taste_head.py`, docs/research/taste-head.md).
//!
//! Each stage fills only what the one before left empty, so a model asked for the whole cull
//! (a harness variant's own prompt, [`crate::cull::Taste::Model`]) keeps its own stars.

use crate::analyzers::{embedding, eyes, Fact, Facts};
use crate::cull::{ContactSheet, CullPhoto, CullResult};
use crate::library::{Author, Library, PhotoFilter, ShootId, Verdict};
use crate::Result;
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

/// Each frame's taste for a cull of `shoot` (`facts` in its Photos' order, with "eyes" and
/// "embedding"): the faces' [`score`], or, once the person has flagged enough to learn from,
/// the faces and the [`Head`] given equal weight.
pub fn scores(lib: &Library, shoot: ShootId, facts: &[Facts]) -> Result<Vec<f32>> {
    let faces: Vec<f32> = facts.iter().map(score).collect();
    let Some(head) = Head::learn(&flags(lib)?, shoot) else {
        return Ok(faces);
    };
    let vectors: Vec<Option<Vec<f32>>> = facts
        .iter()
        .map(|f| f.get("embedding").and_then(embedding::vector))
        .collect();
    let mean = mean(vectors.iter().flatten());
    let learned: Vec<f32> = vectors
        .iter()
        .map(|v| {
            v.as_ref()
                .map_or(f32::NAN, |v| head.score(&centred(v, &mean)))
        })
        .collect();
    Ok(blend(&faces, &learned))
}

/// Two tastes in equal measure: each as standard scores within the Shoot, summed. A taste that
/// is level across the Shoot (no faces anywhere) adds nothing; a frame one can't score (NaN, no
/// embedding) sits at that taste's middle.
pub fn blend(a: &[f32], b: &[f32]) -> Vec<f32> {
    let (za, zb) = (standard(a), standard(b));
    za.iter().zip(&zb).map(|(x, y)| x + y).collect()
}

fn standard(xs: &[f32]) -> Vec<f32> {
    let known: Vec<f32> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    let n = known.len().max(1) as f32;
    let m = known.iter().sum::<f32>() / n;
    let sd = (known.iter().map(|x| (x - m).powi(2)).sum::<f32>() / n).sqrt();
    xs.iter()
        .map(|&x| {
            if x.is_finite() && sd > 1e-6 {
                (x - m) / sd
            } else {
                0.0
            }
        })
        .collect()
}

/// Fewest Picks and fewest Rejects a set of the person's flags needs before the [`Head`] reads
/// anything into it.
pub const MIN_FLAGS: usize = 5;

/// One Photo the person flagged themselves: its embedding less its Shoot's mean (a keeper is
/// one that stands out from the rest of its card, not a kind of card), and whether they picked it.
#[derive(Debug, Clone)]
pub struct Flag {
    pub shoot: ShootId,
    pub vector: Vec<f32>,
    pub pick: bool,
}

/// What the person's own flags say about taste: the direction in the image embedding from what
/// they reject to what they pick. Their flags on other Shoots and on the Shoot being culled each
/// give one direction of unit length, summed, so years of history don't drown out the frames
/// they just flagged on this card. Retraining is an average, so it costs nothing to redo every
/// cull. A nearest-centroid head ranked as well as a logistic one on the bench and needs no
/// tuning (docs/research/taste-head.md).
#[derive(Debug, Clone)]
pub struct Head {
    direction: Vec<f32>,
}

impl Head {
    /// The head for a cull of `shoot`, or None when neither the other Shoots nor this one hold
    /// [`MIN_FLAGS`] Picks and Rejects.
    pub fn learn(flags: &[Flag], shoot: ShootId) -> Option<Head> {
        let parts = [
            direction(flags.iter().filter(|f| f.shoot != shoot)),
            direction(flags.iter().filter(|f| f.shoot == shoot)),
        ];
        let mut parts = parts.into_iter().flatten();
        let mut sum = parts.next()?;
        for p in parts {
            sum.iter_mut().zip(&p).for_each(|(s, x)| *s += x);
        }
        Some(Head { direction: sum })
    }

    /// How much a frame (its embedding less its Shoot's mean) looks like a keeper; only the
    /// order within a Shoot means anything.
    pub fn score(&self, centred: &[f32]) -> f32 {
        embedding::similarity(&self.direction, centred)
    }
}

/// Mean of the Picks less mean of the Rejects, unit length.
fn direction<'a>(flags: impl Iterator<Item = &'a Flag> + Clone) -> Option<Vec<f32>> {
    let picks = flags.clone().filter(|f| f.pick).map(|f| &f.vector);
    let rejects = flags.filter(|f| !f.pick).map(|f| &f.vector);
    if picks.clone().count() < MIN_FLAGS || rejects.clone().count() < MIN_FLAGS {
        return None;
    }
    let (p, r) = (mean(picks), mean(rejects));
    let d: Vec<f32> = p.iter().zip(&r).map(|(a, b)| a - b).collect();
    let norm = d.iter().map(|x| x * x).sum::<f32>().sqrt();
    (norm > 1e-9).then(|| d.iter().map(|x| x / norm).collect())
}

fn mean<'a>(vs: impl Iterator<Item = &'a Vec<f32>>) -> Vec<f32> {
    let mut sum = vec![0.0; embedding::DIMS];
    let mut n = 0.0;
    for v in vs {
        sum.iter_mut().zip(v).for_each(|(s, x)| *s += x);
        n += 1.0;
    }
    sum.iter().map(|s| s / f32::max(n, 1.0)).collect()
}

fn centred(v: &[f32], mean: &[f32]) -> Vec<f32> {
    v.iter().zip(mean).map(|(a, b)| a - b).collect()
}

/// Every Pick and Reject the person made themselves (never the AI's), on Photos whose embedding
/// is already stored, each centred on its Shoot's mean embedding.
pub fn flags(lib: &Library) -> Result<Vec<Flag>> {
    let photos = lib.list_photos(&PhotoFilter::default())?;
    let flagged = |p: &crate::library::Photo| {
        p.verdict_by == Some(Author::Person) && p.verdict != Verdict::Unflagged
    };
    let shoots: std::collections::HashSet<ShootId> = photos
        .iter()
        .filter(|p| flagged(p))
        .map(|p| p.shoot_id)
        .collect();
    let mut by_shoot: HashMap<ShootId, Vec<(&crate::library::Photo, Vec<f32>)>> = HashMap::new();
    for p in photos.iter().filter(|p| shoots.contains(&p.shoot_id)) {
        let Some(row) = lib.fact(p.id, "embedding")? else {
            continue;
        };
        let fact = Fact {
            value: row.value,
            blob: row.blob,
        };
        if let Some(v) = embedding::vector(&fact) {
            by_shoot.entry(p.shoot_id).or_default().push((p, v));
        }
    }
    let mut out = Vec::new();
    for (shoot, frames) in by_shoot {
        let m = mean(frames.iter().map(|(_, v)| v));
        out.extend(
            frames
                .iter()
                .filter(|(p, _)| flagged(p))
                .map(|(p, v)| Flag {
                    shoot,
                    vector: centred(v, &m),
                    pick: p.verdict == Verdict::Pick,
                }),
        );
    }
    Ok(out)
}

/// How far the heads in a frame lean, in degrees: the mean slope of each face's line between
/// its eyes, in the preview's normalised coordinates (so on a 3:2 frame a few degrees off the
/// true roll, the same way for every frame of a burst). None when no face was found.
///
/// Inside a burst, the frame whose heads sit most upright was Greg's keeper in 70% of 76
/// keep-versus-drop pairs on his clean 2026-10-10 card (56% on the older bench Shoots), where
/// the faces' expression, eye sharpness and the frame's sharpness were all at chance
/// (docs/research/taste-head.md). A tilted head is usually a head in motion.
pub fn tilt(facts: &Facts) -> Option<f32> {
    let faces = facts.get("faces").map(crate::analyzers::faces::read)?;
    if faces.is_empty() {
        return None;
    }
    let n = faces.len() as f32;
    Some(
        faces
            .iter()
            .map(|f| {
                let [l, r] = f.eyes;
                (r[1] - l[1]).atan2(r[0] - l[0]).to_degrees().abs()
            })
            .sum::<f32>()
            / n,
    )
}

/// A frame this much darker than the median of the frames shot within [`NEAR_S`] of it is a
/// misjudged exposure of the same moment: 2 EV under is well below this, while on Greg's cards
/// only 4 of 661 frames fall under it, none of them keepers. The Technical check leaves such a
/// frame alone (a RAW can be rescued), so Taste ranks it below its neighbours instead.
const DARKER: f32 = 0.65;
const NEAR_S: i64 = 30;

/// A frame under this share of the sharpness of a look-alike shot seconds from it is the same
/// moment caught with shake or missed focus ([`crate::technical`]); it pays [`DARK_COST`] too, so
/// it ranks below the sharp one even when the Technical check keeps it. Shaken twins on the
/// bench sit at 0.06 to 0.40 of their originals.
const SOFTER: f32 = 0.4;

/// What a frame's dark exposure costs: enough to put it below any neighbour that tastes the same.
const DARK_COST: f32 = 1.0;

/// Each Photo's taste, less [`DARK_COST`] when it is much darker than its neighbours in time,
/// and again when it is much softer than a look-alike ([`SOFTER`]).
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
            let soft = p.sharpness.beside < SOFTER;
            let cost = if dark { DARK_COST } else { 0.0 } + if soft { DARK_COST } else { 0.0 };
            (p.n, p.taste - cost)
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
            // Inside a burst, the most upright heads first ([`tilt`]); a frame whose faces
            // weren't found counts as the burst's average, so taste decides between those.
            let tilts: Vec<f32> = frames
                .iter()
                .filter_map(|c| by_n.get(&c.n).and_then(|p| p.tilt))
                .collect();
            let usual = tilts.iter().sum::<f32>() / tilts.len().max(1) as f32;
            let lean = |c: &CullPhoto| by_n.get(&c.n).and_then(|p| p.tilt).unwrap_or(usual);
            frames.sort_by(|a, b| lean(a).total_cmp(&lean(b)).then_with(|| order(a, b)));
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
