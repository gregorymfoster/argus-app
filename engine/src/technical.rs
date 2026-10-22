//! The cull's Technical check without a model: rules on the sharpness Signal the Analyzers
//! already stored. A frame is plainly broken when it is far softer than the rest of its Shoot,
//! or far softer than a look-alike frame shot seconds before or after it (the same moment, so
//! the difference is focus or shake, not the scene). Exposure is left alone: a dark RAW can be
//! rescued, and [`crate::taste`] ranks it below its neighbours instead.
//!
//! On the damaged-twin check it rejected 18 of 30 twins (all 10 blurred, 5 of 10 shaken, 3 of 5
//! badly cropped) where Qwen 3.8 27B rejected 21, with none of Greg's keepers rejected by
//! either, in well under a second for five Shoots against Qwen's 40 minutes for one
//! (`bench/technical_rules.py`, docs/research/technical-rules.md). The thresholds were set on
//! those same twins, so read the numbers as a little optimistic.

use crate::analyzers::{embedding, Facts};
use crate::cull::{ContactSheet, CullPhoto, CullResult, Provider};
use crate::library::{Photo, Verdict};
use crate::Result;

/// Softer than this share of the Shoot's median sharpness: missed focus or heavy blur. On
/// Greg's five bench cards no real frame is under 0.15 but one he dropped; blurred twins sit
/// near 0.01.
const SHOOT_SOFT: f32 = 0.12;
/// Softer than this share of its sharpest look-alike, and under [`SOFT_IN_SHOOT`] of the
/// Shoot's median: shake or missed focus on a moment the camera also caught sharp.
const BESIDE_SOFT: f32 = 0.2;
const SOFT_IN_SHOOT: f32 = 0.4;
/// Look-alikes: the same scene ([`embedding`] similarity) within 30 s, or any frame shot within
/// 2 s that still looks roughly alike (blur and shake move a frame's embedding off its sibling).
const SAME_SCENE: f32 = 0.85;
const SCENE_S: i64 = 30;
const ROUGHLY: f32 = 0.6;
const BURST_S: i64 = 2;

/// How sharp a frame is next to its Shoot and its look-alikes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sharpness {
    /// Its sharpness over the Shoot's median (1 = a typical frame).
    pub in_shoot: f32,
    /// Its sharpness over its sharpest look-alike's, at most 1; 1 when it has none.
    pub beside: f32,
}

impl Default for Sharpness {
    fn default() -> Self {
        Self {
            in_shoot: 1.0,
            beside: 1.0,
        }
    }
}

/// Each Photo's [`Sharpness`] (`facts` in the same order, with "signals" and "embedding").
pub fn measure(photos: &[Photo], facts: &[Facts]) -> Vec<Sharpness> {
    let sharp: Vec<f32> = facts
        .iter()
        .map(|f| {
            f.get("signals")
                .map(crate::analyzers::signals::read)
                .unwrap_or_default()
                .sharpness
        })
        .collect();
    let at: Vec<i64> = photos
        .iter()
        .map(|p| crate::cull::seconds(&p.captured_at))
        .collect();
    let vecs: Vec<Option<Vec<f32>>> = facts
        .iter()
        .map(|f| f.get("embedding").and_then(embedding::vector))
        .collect();
    let mut sorted = sharp.clone();
    sorted.sort_by(f32::total_cmp);
    let median = sorted
        .get(sorted.len() / 2)
        .copied()
        .unwrap_or(0.0)
        .max(1e-6);
    (0..photos.len())
        .map(|i| {
            let alike = |j: usize| {
                let gap = (at[j] - at[i]).abs();
                let sim = match (&vecs[i], &vecs[j]) {
                    (Some(a), Some(b)) => embedding::similarity(a, b),
                    _ => return false,
                };
                (gap <= SCENE_S && sim >= SAME_SCENE) || (gap <= BURST_S && sim >= ROUGHLY)
            };
            let best = (0..photos.len())
                .filter(|&j| j != i && alike(j))
                .map(|j| sharp[j])
                .fold(sharp[i], f32::max);
            Sharpness {
                in_shoot: sharp[i] / median,
                beside: if best > 0.0 { sharp[i] / best } else { 1.0 },
            }
        })
        .collect()
}

/// Why a frame is broken, or None when it is sound.
pub fn broken(s: Sharpness) -> Option<&'static str> {
    if s.in_shoot < SHOOT_SOFT {
        Some("out of focus")
    } else if s.beside < BESIDE_SOFT && s.in_shoot < SOFT_IN_SHOOT {
        Some("blurred, a sharper frame of it exists")
    } else {
        None
    }
}

/// `result` with every frame the rules call broken rejected, when the Provider left it
/// unflagged (a model's second opinion keeps the rules' rejects; it never picks).
pub fn add_rejects(sheet: &ContactSheet, mut result: CullResult) -> CullResult {
    for c in &mut result.photos {
        let Some(p) = sheet.photos.iter().find(|p| p.n == c.n) else {
            continue;
        };
        if c.verdict == Verdict::Unflagged {
            if let Some(why) = broken(p.sharpness) {
                c.verdict = Verdict::Reject;
                c.reason = why.into();
            }
        }
    }
    result
}

/// The Technical check as a Provider: no model, every Photo answered from its Sharpness at
/// once. The default check (`cull::Check::Rules`).
pub struct Rules;

impl Provider for Rules {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        let photos: Vec<CullPhoto> = sheet
            .photos
            .iter()
            .map(|p| {
                let why = broken(p.sharpness);
                CullPhoto {
                    n: p.n,
                    verdict: if why.is_some() {
                        Verdict::Reject
                    } else {
                        Verdict::Unflagged
                    },
                    reason: why.unwrap_or("ok").into(),
                    stack: None,
                    rank: None,
                    stars: None,
                    best: None,
                }
            })
            .collect();
        let rejects = photos
            .iter()
            .filter(|c| c.verdict == Verdict::Reject)
            .count();
        Ok(CullResult {
            summary: match rejects {
                0 => "No technically broken frames.".into(),
                1 => "1 frame is blurred or out of focus.".into(),
                n => format!("{n} frames are blurred or out of focus."),
            },
            photos,
        })
    }
}
