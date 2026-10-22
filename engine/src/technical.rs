//! The cull's Technical check without a model: rules on the Signals the Analyzers already
//! stored. A frame is plainly broken when it is far softer than the rest of its Shoot, far
//! softer than a look-alike frame shot seconds before or after it (the same moment, so the
//! difference is focus or shake, not the scene), or soft with its edges all running one way
//! (camera shake). Exposure is left alone: a dark RAW can be rescued, and [`crate::taste`]
//! ranks it below its neighbours instead. Frames it keeps but isn't sure of are [`doubtful`],
//! and only those go to a model's second opinion.
//!
//! On the damaged-twin check it rejects 24 of 30 twins (all 10 blurred, all 10 shaken, 4 of 5
//! badly cropped) where Qwen 3.8 27B rejected 21, with none of Greg's keepers rejected, in
//! seconds for five Shoots against Qwen's 40 minutes for one (`bench/technical_rules.py`,
//! `bench/shake_eyes.py`, docs/research/technical-rules.md). The thresholds were set on those
//! same twins, so read the numbers as a little optimistic.

use crate::analyzers::{embedding, Facts};
use crate::cull::{ContactSheet, CullPhoto, CullResult, Provider};
use crate::library::{Photo, Verdict};
use crate::Result;

/// Softer than this share of the Shoot's median sharpness: missed focus or heavy blur. On
/// Greg's five bench cards no real frame is under 0.15 but one he dropped; blurred twins sit
/// near 0.01.
const SHOOT_SOFT: f32 = 0.12;
/// A face at least this share of the frame's height: a portrait, whose sharpness lives in a
/// small face over soft bokeh, so it reads far softer than a Shoot of scenes. Greg's 2026-10-22
/// drive test lost one favorite (a shallow-focus portrait at 0.1 of its day's median) to
/// [`SHOOT_SOFT`]; on the bench Shoots no blurred twin needed that rule when a face was this big
/// (its sharp sibling caught it).
const PORTRAIT_FACE: f32 = 0.15;
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
/// Camera shake: the sharpest tiles' edges nearly all run one way (the Signals' `streak`) and
/// the frame is well under the Shoot's median sharpness. On the five bench Shoots no frame
/// Greg kept passes both (the streakiest keepers, up to 0.74, are sharp), while 28 of 30
/// shaken copies of his picks do, shaken sideways, at 45 or at 90 degrees (bench/shake_eyes.py).
const SHAKE_STREAK: f32 = 0.6;
const SHAKE_SOFT: f32 = 0.6;
/// Doubtful, worth a model's look ([`doubtful`]): softer than this share of the Shoot's median,
/// or of a look-alike while under [`DOUBT_SOFT_BESIDE_SHOOT`] of the median, or streaky.
const DOUBT_SOFT: f32 = 0.35;
const DOUBT_BESIDE: f32 = 0.5;
const DOUBT_SOFT_BESIDE_SHOOT: f32 = 0.7;
const DOUBT_STREAK: f32 = 0.45;
const DOUBT_STREAK_SOFT: f32 = 0.8;
/// A face reads as smiling (a laugh with eyes shut is a keeper) above this.
const LAUGH: f32 = 0.4;

/// How sharp a frame is next to its Shoot and its look-alikes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sharpness {
    /// Its sharpness over the Shoot's median (1 = a typical frame).
    pub in_shoot: f32,
    /// Its sharpness over its sharpest look-alike's, at most 1; 1 when it has none.
    pub beside: f32,
    /// How much its sharpest edges all run one way ([`crate::signals::Signals::streak`]).
    pub streak: f32,
    /// A face fills at least [`PORTRAIT_FACE`] of its height: it isn't judged against the
    /// Shoot's median sharpness alone.
    pub portrait: bool,
}

impl Default for Sharpness {
    fn default() -> Self {
        Self {
            in_shoot: 1.0,
            beside: 1.0,
            streak: 0.0,
            portrait: false,
        }
    }
}

/// Each Photo's [`Sharpness`] (`facts` in the same order, with "signals" and "embedding").
pub fn measure(photos: &[Photo], facts: &[Facts]) -> Vec<Sharpness> {
    let signals: Vec<crate::signals::Signals> = facts
        .iter()
        .map(|f| {
            f.get("signals")
                .map(crate::analyzers::signals::read)
                .unwrap_or_default()
        })
        .collect();
    let sharp: Vec<f32> = signals.iter().map(|s| s.sharpness).collect();
    let at: Vec<i64> = photos
        .iter()
        .map(|p| crate::library::time::capture_seconds(&p.captured_at))
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
                streak: signals[i].streak,
                portrait: facts[i]
                    .get("faces")
                    .map(crate::analyzers::faces::read)
                    .unwrap_or_default()
                    .iter()
                    .any(|f| f.h >= PORTRAIT_FACE),
            }
        })
        .collect()
}

/// Why a frame is broken, or None when it is sound.
pub fn broken(s: Sharpness) -> Option<&'static str> {
    if s.in_shoot < SHOOT_SOFT && !s.portrait {
        Some("out of focus")
    } else if s.beside < BESIDE_SOFT && s.in_shoot < SOFT_IN_SHOOT {
        Some("blurred, a sharper frame of it exists")
    } else if s.streak > SHAKE_STREAK && s.in_shoot < SHAKE_SOFT {
        Some("camera shake")
    } else {
        None
    }
}

/// Whether a main face has its eyes shut without laughing. Not a reject on its own: on the bench
/// Shoots Greg kept 4 of the 16 frames like that, so it only makes a frame [`doubtful`].
pub fn shut_eyes(facts: &crate::analyzers::Facts) -> bool {
    use crate::analyzers::eyes;
    let reads = facts.get("eyes").map(eyes::read).unwrap_or_default();
    eyes::main_faces(&reads)
        .iter()
        .any(|r| r.eyes_closed && eyes::smile(r) < LAUGH)
}

/// Why a frame the rules keep is still worth a model's look (soft, streaky, eyes shut without
/// a laugh), or None when it is plainly sound. About one frame in fifteen on the bench Shoots:
/// a model's second opinion ([`crate::cull::Check::Model`]) sees only these.
pub fn doubtful(s: Sharpness, shut_eyes: bool) -> Option<&'static str> {
    if broken(s).is_some() {
        None
    } else if s.in_shoot < DOUBT_SOFT
        || (s.beside < DOUBT_BESIDE && s.in_shoot < DOUBT_SOFT_BESIDE_SHOOT)
    {
        Some("soft")
    } else if s.streak > DOUBT_STREAK && s.in_shoot < DOUBT_STREAK_SOFT {
        Some("streaky, maybe shake")
    } else if shut_eyes {
        Some("eyes shut")
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
                1 => "1 frame is blurred, shaken or out of focus.".into(),
                n => format!("{n} frames are blurred, shaken or out of focus."),
            },
            photos,
        })
    }
}
