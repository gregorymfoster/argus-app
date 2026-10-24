//! Shoot match: "apply to the rest" without a model call. The person's hand edits in a
//! Shoot set its style; each other Photo gets that style moved the way the person's edits
//! move from frame to frame inside a Shoot (docs/research/auto-edit-research.md).
//!
//! A mixed model fitted by `bench/shoot_style.py export` to Greg's RAW and edit pairs: every
//! Shoot has its own offset from Auto (its style), and a ridge learned only from differences
//! inside Shoots says how that offset follows the scene (the same cheap and scene facts the
//! Edit head reads). For a Photo, the Shoot's offset comes from the hand-edited Photos (the
//! anchors), the ones whose scene is closer counting more, shrunk toward the average Shoot
//! while there are few. Held out by day and rendered, one hero edit lands at 8.7 dE from
//! Greg's own edit against 10.0 for copying the hero's sliders (Sync) and 9.2 for the head.

use crate::edit_head::{Head, Sample, SLIDERS};
use crate::library::{Author, Library, Photo, PhotoId};
use crate::recipe::{Adjust, Recipe};
use crate::Result;
use serde::Deserialize;
use std::sync::OnceLock;

/// At most this many hand-edited Photos of the Shoot are read as anchors (the hero first,
/// then the most recently edited), so matching a big Shoot stays quick.
pub const MAX_ANCHORS: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// The within-Shoot regression; its intercept is the average Shoot's offset.
    within: Head,
    /// Per slider, the frame-to-frame noise over how much Shoots differ: k anchors count
    /// k / (k + ratio) against the average Shoot.
    ratio: Vec<f64>,
    /// Anchors weigh exp(-d / (2 width²)), d the mean squared z distance of their facts.
    width: f64,
}

impl Model {
    pub fn shipped() -> &'static Model {
        static M: OnceLock<Model> = OnceLock::new();
        M.get_or_init(|| {
            let m: Model = serde_json::from_str(include_str!("shoot_match.json"))
                .expect("shoot_match.json parses");
            assert_eq!(m.ratio.len(), SLIDERS.len(), "shoot_match.json ratio");
            m
        })
    }

    /// The offset from plain Auto (in [`SLIDERS`] order) for a Photo with these facts,
    /// given the Shoot's hand-edited `anchors`. Without anchors it is the average Shoot's.
    pub fn offset(&self, facts: &[(String, f64)], anchors: &[Sample]) -> [f64; 9] {
        let own = self.within.predict(facts);
        if anchors.is_empty() {
            return std::array::from_fn(|j| own[j]);
        }
        let z = self.within.z(facts);
        let w: Vec<f64> = anchors
            .iter()
            .map(|a| {
                let za = self.within.z(&a.facts);
                let d = z.iter().zip(&za).map(|(x, y)| (x - y).powi(2)).sum::<f64>()
                    / z.len().max(1) as f64;
                (-d / (2.0 * self.width * self.width)).exp()
            })
            .collect();
        let total: f64 = w.iter().sum::<f64>().max(1e-12);
        let intercept = self.within.intercept();
        let k = anchors.len() as f64;
        std::array::from_fn(|j| {
            // the Shoot's own offset, read off its anchors with the scene's part taken out
            let shoot = anchors
                .iter()
                .zip(&w)
                .map(|(a, wi)| {
                    wi * (a.offset[j] - (self.within.predict(&a.facts)[j] - intercept[j]))
                })
                .sum::<f64>()
                / total;
            let trust = k / (k + self.ratio[j]);
            own[j] + trust * (shoot - intercept[j])
        })
    }
}

/// A Photo's facts and plain Auto (no Edit head) as the model reads them: measured on its
/// unedited render, with no Look.
fn measure(lib: &Library, p: &Photo) -> Result<(Vec<(String, f64)>, Adjust)> {
    let base = Recipe::default();
    let render = |s: &Adjust| {
        let r = Recipe {
            global: s.clone(),
            ..base.clone()
        };
        crate::render::render_photo(p, &r, Some(crate::auto::MEASURE_EDGE), None)
    };
    let plain = crate::auto::Params {
        edit_head: false,
        ..crate::auto::Params::current()?
    };
    let auto = crate::auto::auto_adjust_with(render, true, &plain)?;
    let analysis = crate::render::analysis(lib, p).ok();
    let facts = crate::edit_head::facts(&render(&Adjust::default())?, &auto, analysis.as_deref());
    Ok((facts, auto))
}

/// The hero `first` and the other Photos of `shoot` the person edited by hand (their newest
/// Recipe is theirs), most recently edited first; RAWs only, at most [`MAX_ANCHORS`].
pub fn anchors_of(
    lib: &Library,
    shoot: crate::library::ShootId,
    first: PhotoId,
    skip: PhotoId,
) -> Result<Vec<Photo>> {
    let mut edited = Vec::new();
    for p in lib.list_photos(&crate::library::PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })? {
        if p.id == skip || p.kind != "raw" {
            continue;
        }
        if let Some(v) = lib.read_recipe(p.id)? {
            if p.id == first || v.author == Author::Person {
                edited.push((p.id != first, std::cmp::Reverse(v.at.clone()), p));
            }
        }
    }
    edited.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    Ok(edited
        .into_iter()
        .take(MAX_ANCHORS)
        .map(|(_, _, p)| p)
        .collect())
}

/// The anchors as the model reads them: each one's facts and how far its own Basic sliders
/// (Look included) sit from plain Auto. An anchor that can't be measured is left out.
pub fn samples(lib: &Library, anchors: &[Photo]) -> Result<Vec<Sample>> {
    let mut samples = Vec::new();
    for p in anchors {
        let own = crate::tools::with_look(lib, p, &crate::tools::current_recipe(lib, p.id)?)?;
        // An odd file costs that anchor, not the match.
        let Ok((facts, auto)) = measure(lib, p) else {
            continue;
        };
        let mine = crate::edit_head::sliders(&own.global);
        let theirs = crate::edit_head::sliders(&auto);
        samples.push(Sample {
            facts,
            offset: std::array::from_fn(|j| mine[j] - theirs[j]),
        });
    }
    Ok(samples)
}

/// The Shoot match for Photo `target`: its Basic sliders, as they render (Look included),
/// from the hand-edited `anchors`. None when it isn't a RAW or nothing could be measured.
pub fn sliders(
    lib: &Library,
    target: &Photo,
    anchors: &[Photo],
) -> Result<Option<(Adjust, usize)>> {
    if target.kind != "raw" {
        return Ok(None);
    }
    sliders_from(lib, target, &samples(lib, anchors)?)
}

/// [`sliders`] from anchors already measured with [`samples`], so a batch measures them once.
pub fn sliders_from(
    lib: &Library,
    target: &Photo,
    samples: &[Sample],
) -> Result<Option<(Adjust, usize)>> {
    if target.kind != "raw" || samples.is_empty() {
        return Ok(None);
    }
    let (facts, auto) = measure(lib, target)?;
    let off = Model::shipped().offset(&facts, samples);
    let a = crate::edit_head::sliders(&auto);
    let v: [f64; 9] = std::array::from_fn(|j| a[j] + off[j]);
    let r = |x: f64| x.round();
    let out = Adjust {
        exposure: (v[0] * 20.0).round() / 20.0,
        contrast: r(v[1]),
        highlights: r(v[2]),
        shadows: r(v[3]),
        whites: r(v[4]),
        blacks: r(v[5]),
        temp: r(v[6]),
        tint: r(v[7]),
        vibrance: r(v[8]),
        ..Adjust::default()
    }
    .clamped();
    Ok(Some((out, samples.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_pull_the_offset_toward_the_shoots_style() {
        let m = Model::shipped();
        let f = m.within.mean_facts();
        let none = m.offset(&f, &[]);
        let warm = Sample {
            facts: f.clone(),
            offset: std::array::from_fn(|j| {
                none[j] + if SLIDERS[j] == "temp" { 30.0 } else { 0.0 }
            }),
        };
        let one = m.offset(&f, std::slice::from_ref(&warm));
        let three = m.offset(&f, &[warm.clone(), warm.clone(), warm.clone()]);
        let t = SLIDERS.iter().position(|s| *s == "temp").unwrap();
        assert!(
            one[t] > none[t] + 5.0,
            "one anchor warms it: {} -> {}",
            none[t],
            one[t]
        );
        assert!(
            three[t] > one[t] && three[t] < none[t] + 30.0,
            "more anchors, more trust, never past them"
        );
        let e = SLIDERS.iter().position(|s| *s == "exposure").unwrap();
        assert!(
            (one[e] - none[e]).abs() < 1e-9,
            "an anchor that matches the average moves nothing else"
        );
    }
}
