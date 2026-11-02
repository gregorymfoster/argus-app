//! Quick fixes that Auto adds by itself (Greg, 2026-11-02: "I'd love if auto automatically
//! mildly applied appropriate quick fixes, without looking weird").
//!
//! The four Quick fixes (Enhance sky, Brighten faces, Even skin, Lift shadows) are Presets
//! in `presets/builtin.json` under `fixes`. After Auto has set the light, [`choose`] reads
//! the Photo as Auto left it (the Mask models' faces, and its tones) and picks the fixes it
//! clearly needs, each at a gentle Amount, well under the 100% a click gives. Checked on 99
//! bench RAWs against Greg's Lightroom edits (`bench/quick_fixes.py`, 2026-11-02):
//!
//! - **Brighten faces** when faces sit dark (below mid-grey and darker than the frame),
//!   more the darker they are, 30% to 70% (up to +0.18 EV on face skin). A face too dark to
//!   lift cleanly (a silhouette) is left alone. Faces moved closer to Greg's (-0.26 dE on
//!   faces, 49 frames).
//! - **Even skin** at 35% when a face is big enough for skin texture to show (a portrait).
//!   Too fine for the bench's dE to see either way; natural retouching, kept light.
//! - **Lift shadows** at 40% when part of the frame is near black in an otherwise bright
//!   enough Photo and Auto left Shadows low. Never on a low-key or night frame, where
//!   lifting turns black into mud and noise. Closer to Greg (-0.07 dE whole, -0.25 faces).
//! - **Enhance sky** is never added by Auto: on the 17 bench frames with a bright sky it
//!   moved skies further from Greg's (+0.21 dE), who dims skies a touch and doesn't add
//!   colour. It stays one click away.
//!
//! They are written as ordinary applied Presets, so the Edit panel shows them as ticked
//! chips and one click takes any of them off. A fix already on the Recipe (by hand or by an
//! earlier Auto) stays at its Amount: Auto takes fixes off before measuring
//! ([`strip`]) and puts them back after ([`add`]), so a re-run never double-counts.

use crate::edit_head::{features, scene_features};
use crate::masks::Analysis;
use crate::presets;
use crate::recipe::{Adjust, Recipe};
use crate::{Result, RgbImage};
use serde::Serialize;

/// What [`choose`] reads about a Photo as Auto left it. Lightness is CIELAB L (0..100);
/// `face_l` is relative to the frame's mean.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// Mean L of the whole frame.
    pub mean: f64,
    /// 5th and 50th percentiles of L.
    pub p05: f64,
    pub p50: f64,
    pub faces: usize,
    /// Faces' total area, as a share of the frame.
    pub face_area: f64,
    /// The biggest face's area, as a share of the frame.
    pub face_max: f64,
    pub face_l: f64,
}

/// A fix Auto chose, at an Amount (0..1), and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pick {
    pub id: &'static str,
    pub amount: f64,
    pub why: &'static str,
}

/// Faces smaller than this share of the frame (all of them together) are left alone.
const FACE_AREA: f64 = 0.002;
/// Absolute face L at which Brighten faces starts, and where it reaches its most.
const FACE_START: f64 = 58.0;
const FACE_FULL: f64 = 38.0;
/// Faces darker than this are a silhouette, or too dark to lift without noise.
const FACE_FLOOR: f64 = 15.0;
/// A face at least this share of the frame (about 14% by 14%) shows its skin.
const SKIN_FACE: f64 = 0.02;

/// The fixes this Photo clearly needs, gentlest first in panel order. `auto` is what Auto
/// set, so Lift shadows doesn't pile onto shadows Auto already opened.
pub fn choose(f: &Facts, auto: &Adjust) -> Vec<Pick> {
    let mut out = Vec::new();
    let lerp = |x: f64, x0: f64, x1: f64, y0: f64, y1: f64| {
        let t = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
        y0 + (y1 - y0) * t
    };
    let has_faces = f.faces > 0 && f.face_area >= FACE_AREA;
    let face = f.mean + f.face_l;
    if has_faces && face < FACE_START && f.face_l < -3.0 && face >= FACE_FLOOR {
        out.push(Pick {
            id: "fix-faces",
            amount: round2(lerp(face, FACE_START, FACE_FULL, 0.3, 0.7)),
            why: "faces in shadow",
        });
    }
    if has_faces && f.face_max >= SKIN_FACE {
        out.push(Pick {
            id: "fix-skin",
            amount: 0.35,
            why: "a close face",
        });
    }
    if f.p05 < 6.0 && f.p50 > 32.0 && auto.shadows < 45.0 {
        out.push(Pick {
            id: "fix-shadows",
            amount: 0.4,
            why: "shadows near black",
        });
    }
    out
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// [`Facts`] about a render (Auto's measuring size is plenty) and the Photo's Analysis.
pub fn facts(img: &RgbImage, analysis: Option<&Analysis>) -> Facts {
    let get = |v: &[(String, f64)], k: &str| {
        v.iter()
            .find(|(n, _)| n == k)
            .map(|(_, x)| *x)
            .unwrap_or(0.0)
    };
    let tones = features(img);
    let mut f = Facts {
        mean: get(&tones, "l_mean"),
        p05: get(&tones, "l_p05"),
        p50: get(&tones, "l_p50"),
        ..Default::default()
    };
    if let Some(a) = analysis {
        let scene = scene_features(img, a);
        f.faces = a.faces.len();
        f.face_area = get(&scene, "face_area");
        f.face_max = a
            .faces
            .iter()
            .map(|x| (x.w * x.h) as f64)
            .fold(0.0, f64::max);
        f.face_l = get(&scene, "face_l");
    }
    f
}

/// The Quick fixes on a Recipe, by id with their Amount.
pub fn on(recipe: &Recipe) -> Vec<(String, f64)> {
    presets::on(recipe)
        .into_iter()
        .filter(|a| presets::fixes().iter().any(|p| p.id == a.id))
        .map(|a| (a.id, a.amount))
        .collect()
}

/// `recipe` with its Quick fixes taken off, and those fixes, so Auto measures the Photo
/// without them and [`add`] can put them back.
pub fn strip(recipe: &Recipe) -> Result<(Recipe, Vec<(String, f64)>)> {
    let kept = on(recipe);
    let mut r = recipe.clone();
    for (id, _) in &kept {
        r = presets::remove(&r, presets::find(id)?)?;
    }
    Ok((r, kept))
}

/// The fixes a Photo needs as `img` (the Photo as Auto left it) shows it.
pub fn pick(img: &RgbImage, analysis: Option<&Analysis>, auto: &Adjust) -> Vec<Pick> {
    choose(&facts(img, analysis), auto)
}

/// `auto` (a Recipe Auto just set, from a [`strip`]ped one) with the fixes that were on
/// before (`kept`, at their Amount) and the `picks` that weren't.
pub fn add(auto: Recipe, kept: &[(String, f64)], picks: &[Pick]) -> Result<Recipe> {
    let mut r = auto;
    for (id, amount) in kept {
        r = presets::apply(&r, presets::find(id)?, *amount)?;
    }
    for p in picks
        .iter()
        .filter(|p| !kept.iter().any(|(id, _)| id == p.id))
    {
        r = presets::apply(&r, presets::find(p.id)?, p.amount)?;
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daylight() -> Facts {
        Facts {
            mean: 50.0,
            p05: 12.0,
            p50: 50.0,
            ..Default::default()
        }
    }

    fn ids(p: &[Pick]) -> Vec<&str> {
        p.iter().map(|p| p.id).collect()
    }

    #[test]
    fn a_plain_frame_gets_nothing() {
        assert!(choose(&daylight(), &Adjust::default()).is_empty());
    }

    #[test]
    fn a_dark_face_is_lifted_more_the_darker_it_is() {
        let mut f = daylight();
        f.faces = 1;
        f.face_area = 0.006;
        f.face_max = 0.006;
        f.face_l = -8.0; // L 42
        let a = choose(&f, &Adjust::default());
        assert_eq!(ids(&a), ["fix-faces"]);
        let darker = Facts {
            face_l: -16.0,
            ..f.clone()
        };
        let b = choose(&darker, &Adjust::default());
        assert!(b[0].amount > a[0].amount && b[0].amount <= 0.7);
        // A face already as bright as the frame, a tiny one, or a silhouette: left alone.
        for g in [
            Facts {
                face_l: 2.0,
                ..f.clone()
            },
            Facts {
                face_area: 0.001,
                ..f.clone()
            },
            Facts {
                face_l: -40.0,
                ..f.clone()
            },
        ] {
            assert!(!ids(&choose(&g, &Adjust::default())).contains(&"fix-faces"));
        }
    }

    #[test]
    fn a_close_face_gets_gentle_even_skin() {
        let mut f = daylight();
        f.faces = 1;
        f.face_area = 0.05;
        f.face_max = 0.05;
        f.face_l = 5.0;
        let p = choose(&f, &Adjust::default());
        assert_eq!(ids(&p), ["fix-skin"]);
        assert!(p[0].amount < 0.5);
    }

    #[test]
    fn shadows_lift_only_in_a_bright_enough_frame() {
        let mut f = daylight();
        f.p05 = 3.0;
        assert_eq!(ids(&choose(&f, &Adjust::default())), ["fix-shadows"]);
        // Auto already opened them.
        let opened = Adjust {
            shadows: 60.0,
            ..Default::default()
        };
        assert!(choose(&f, &opened).is_empty());
        // Night or low key: leave the blacks black.
        f.p50 = 20.0;
        assert!(choose(&f, &Adjust::default()).is_empty());
    }

    #[test]
    fn every_amount_is_gentle() {
        let f = Facts {
            mean: 60.0,
            p05: 1.0,
            p50: 60.0,
            faces: 3,
            face_area: 0.2,
            face_max: 0.1,
            face_l: -40.0, // L 20, very dark faces
        };
        let p = choose(&f, &Adjust::default());
        assert_eq!(ids(&p), ["fix-faces", "fix-skin", "fix-shadows"]);
        assert!(p.iter().all(|p| p.amount <= 0.7), "{p:?}");
    }

    #[test]
    fn strip_and_add_keep_a_fix_that_was_on() {
        let shadows = presets::find("fix-shadows").unwrap();
        let r = presets::apply(&Recipe::default(), shadows, 0.8).unwrap();
        assert_eq!(r.global.shadows, 24.0);
        let (bare, kept) = strip(&r).unwrap();
        assert_eq!(bare.global.shadows, 0.0);
        assert_eq!(kept, [("fix-shadows".to_string(), 0.8)]);
        // Auto sets Shadows, then the fix goes back on top at its Amount.
        let mut auto = bare.clone();
        auto.global.shadows = 10.0;
        // A pick of a fix already on doesn't stack on it.
        let again = [Pick {
            id: "fix-shadows",
            amount: 0.4,
            why: "",
        }];
        let out = add(auto, &kept, &again).unwrap();
        assert_eq!(out.global.shadows, 34.0);
        assert_eq!(on(&out), kept);
    }
}
