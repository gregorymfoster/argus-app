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
//! With `gp` and `fallback` set (the research lab's E4 and E8, docs/research/lab-notebook.md)
//! the anchors are read as a Gaussian process and a Photo unlike all of them leans toward the
//! Edit head: 8.58 with one hero and 7.79 with three, against 8.71 and 7.90.

use crate::edit_head::{Head, Sample, SLIDERS};
use crate::library::{Author, Library, Photo, PhotoId};
use crate::recipe::{Adjust, Recipe};
use crate::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

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
    /// When set, the anchors are read as a Gaussian process instead of a weighted mean
    /// (docs/research/lab-notebook.md E4): each anchor's miss is the Shoot's offset, plus a
    /// part that varies smoothly with the scene (share `rho` of the frame-to-frame spread,
    /// RBF of `length` on the same distance), plus noise.
    #[serde(default)]
    gp: Option<Gp>,
    /// When set, a Photo unlike every anchor leans toward the Edit head:
    /// weight exp(-d_nearest / fallback) on the match (lab notebook E8).
    #[serde(default)]
    fallback: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Gp {
    rho: f64,
    length: f64,
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
        self.offset_with(facts, anchors, true)
    }

    /// [`Model::offset`], with `smart` false for the first shipped rule (weighted mean, no
    /// Gaussian process or Edit head fallback): Settings › AI › Smarter Shoot match off.
    pub fn offset_with(
        &self,
        facts: &[(String, f64)],
        anchors: &[Sample],
        smart: bool,
    ) -> [f64; 9] {
        let own = self.within.predict(facts);
        if anchors.is_empty() {
            return std::array::from_fn(|j| own[j]);
        }
        let z = self.within.z(facts);
        let za: Vec<Vec<f64>> = anchors.iter().map(|a| self.within.z(&a.facts)).collect();
        let dist = |x: &[f64], y: &[f64]| {
            x.iter().zip(y).map(|(p, q)| (p - q).powi(2)).sum::<f64>() / x.len().max(1) as f64
        };
        let d: Vec<f64> = za.iter().map(|a| dist(&z, a)).collect();
        let matched = match self.gp.as_ref().filter(|_| smart) {
            Some(gp) => self.gp_offset(gp, &own, anchors, &za, &d),
            None => self.mean_offset(&own, anchors, &d),
        };
        match self.fallback.filter(|_| smart) {
            Some(tau) => {
                let near = d.iter().cloned().fold(f64::INFINITY, f64::min);
                let w = (-near / tau).exp();
                let head = Head::shipped().predict(facts);
                std::array::from_fn(|j| w * matched[j] + (1.0 - w) * head[j])
            }
            None => matched,
        }
    }

    /// How well the anchors cover a frame with these facts, 1 for a frame just like one of
    /// them, toward 0 for a scene unlike all of them (where the match leans on the Edit
    /// head, so the person should look at it).
    pub fn reach(&self, facts: &[(String, f64)], anchors: &[Sample]) -> f64 {
        let z = self.within.z(facts);
        let near = anchors
            .iter()
            .map(|a| {
                let za = self.within.z(&a.facts);
                z.iter().zip(&za).map(|(p, q)| (p - q).powi(2)).sum::<f64>() / z.len().max(1) as f64
            })
            .fold(f64::INFINITY, f64::min);
        if !near.is_finite() {
            return 0.0;
        }
        match self.fallback {
            Some(tau) => (-near / tau).exp(),
            None => (-near / (2.0 * self.width * self.width)).exp(),
        }
    }

    /// The shipped-first rule: the anchors' similarity-weighted mean miss, shrunk.
    fn mean_offset(&self, own: &[f64], anchors: &[Sample], d: &[f64]) -> [f64; 9] {
        let w: Vec<f64> = d
            .iter()
            .map(|d| (-d / (2.0 * self.width * self.width)).exp())
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

    /// Per slider, the posterior mean of the Shoot's offset plus the smooth part at this
    /// Photo, in units of the frame-to-frame spread: anchors covary 1/ratio + rho K + (1 - rho) I.
    fn gp_offset(
        &self,
        gp: &Gp,
        own: &[f64],
        anchors: &[Sample],
        za: &[Vec<f64>],
        d: &[f64],
    ) -> [f64; 9] {
        let n = anchors.len();
        let kern = |d: f64| (-d / (2.0 * gp.length * gp.length)).exp();
        let dist = |x: &[f64], y: &[f64]| {
            x.iter().zip(y).map(|(p, q)| (p - q).powi(2)).sum::<f64>() / x.len().max(1) as f64
        };
        let kaa: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|l| kern(dist(&za[i], &za[l]))).collect())
            .collect();
        let miss: Vec<[f64; 9]> = anchors
            .iter()
            .map(|a| {
                let p = self.within.predict(&a.facts);
                std::array::from_fn(|j| a.offset[j] - p[j])
            })
            .collect();
        std::array::from_fn(|j| {
            let u = 1.0 / self.ratio[j];
            let c: Vec<Vec<f64>> = (0..n)
                .map(|i| {
                    (0..n)
                        .map(|l| u + gp.rho * kaa[i][l] + if i == l { 1.0 - gp.rho } else { 0.0 })
                        .collect()
                })
                .collect();
            let r: Vec<f64> = miss.iter().map(|m| m[j]).collect();
            let alpha = solve(c, r);
            own[j]
                + d.iter()
                    .zip(&alpha)
                    .map(|(d, a)| (u + gp.rho * kern(*d)) * a)
                    .sum::<f64>()
        })
    }
}

impl Model {
    /// Which Photos to edit by hand first so the Shoot match learns the most from them
    /// (research lab E6 and E11, docs/research/lab-notebook.md): indexes into `facts`, in
    /// order, `k` of them, none from `edited` (Photos already edited by hand, which count as
    /// heroes). With nothing edited and one to choose, the Shoot's most typical frame (the
    /// one nearest the centre of its facts). Otherwise greedily the Photo that leaves the
    /// least total uncertainty over the Shoot under the Gaussian-process model (Bychkovsky
    /// et al. 2011's active choice): held out by day, three heroes chosen this way bring the
    /// Shoot match from 8.50 to 8.32 dE against three edited at random.
    pub fn heroes(&self, facts: &[Vec<(String, f64)>], edited: &[usize], k: usize) -> Vec<usize> {
        let n = facts.len();
        let z: Vec<Vec<f64>> = facts.iter().map(|f| self.within.z(f)).collect();
        let dist = |x: &[f64], y: &[f64]| {
            x.iter().zip(y).map(|(p, q)| (p - q).powi(2)).sum::<f64>() / x.len().max(1) as f64
        };
        let free: Vec<usize> = (0..n).filter(|i| !edited.contains(i)).collect();
        if edited.is_empty() && k == 1 {
            let dims = z.first().map_or(0, |v| v.len());
            let centre: Vec<f64> = (0..dims)
                .map(|j| z.iter().map(|v| v[j]).sum::<f64>() / n.max(1) as f64)
                .collect();
            return free
                .iter()
                .copied()
                .min_by(|&a, &b| dist(&z[a], &centre).total_cmp(&dist(&z[b], &centre)))
                .into_iter()
                .collect();
        }
        // The bench's kernel (bench/lab_edit.py pick_mi): Shoot intercept 1, smooth part
        // rho * 1.5, noise (1 - rho) * 1.5, RBF of the model's length.
        let (rho, length) = self.gp.as_ref().map_or((0.8, 0.45), |g| (g.rho, g.length));
        let (vu, ve) = (1.0, 1.5);
        let cov = |a: usize, b: usize| {
            vu + rho * ve * (-dist(&z[a], &z[b]) / (2.0 * length * length)).exp()
                + if a == b { (1.0 - rho) * ve } else { 0.0 }
        };
        let left = |chosen: &[usize]| -> f64 {
            let css: Vec<Vec<f64>> = chosen
                .iter()
                .map(|&a| chosen.iter().map(|&b| cov(a, b)).collect())
                .collect();
            (0..n)
                .map(|i| {
                    let c: Vec<f64> = chosen.iter().map(|&a| cov(i, a)).collect();
                    let w = solve(css.clone(), c.clone());
                    cov(i, i) - c.iter().zip(&w).map(|(p, q)| p * q).sum::<f64>()
                })
                .sum()
        };
        let mut chosen: Vec<usize> = edited.to_vec();
        let mut out = Vec::new();
        while out.len() < k {
            let best = free
                .iter()
                .copied()
                .filter(|i| !out.contains(i))
                .map(|i| {
                    let mut s = chosen.clone();
                    s.push(i);
                    (left(&s), i)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, i)) = best else { break };
            chosen.push(i);
            out.push(i);
        }
        out
    }
}

/// [`Model::heroes`] for a Shoot: the RAWs that aren't rejected, measured as the Shoot match
/// reads them, with the ones the person already edited by hand counted as heroes. Returns
/// the suggested Photos in order, and how many were edited already.
pub fn suggest_heroes(
    lib: &Library,
    shoot: crate::library::ShootId,
    k: usize,
) -> Result<(Vec<Photo>, usize)> {
    let mut photos = Vec::new();
    let mut facts = Vec::new();
    let mut edited = Vec::new();
    for p in lib.list_photos(&crate::library::PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })? {
        if p.kind != "raw" || p.verdict == crate::library::Verdict::Reject {
            continue;
        }
        let Ok((f, _)) = measure(lib, &p) else {
            continue;
        };
        if matches!(lib.read_recipe(p.id)?, Some(v) if v.author == Author::Person) {
            edited.push(photos.len());
        }
        facts.push(f);
        photos.push(p);
    }
    let order = Model::shipped().heroes(&facts, &edited, k);
    let n = edited.len();
    Ok((order.into_iter().map(|i| photos[i].clone()).collect(), n))
}

/// Solves A x = b for a small symmetric positive-definite A (Gaussian elimination with
/// partial pivoting; at most MAX_ANCHORS rows).
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n)
            .max_by(|&i, &l| a[i][c].abs().total_cmp(&a[l][c].abs()))
            .unwrap_or(c);
        a.swap(c, p);
        b.swap(c, p);
        let piv = if a[c][c].abs() < 1e-12 {
            1e-12
        } else {
            a[c][c]
        };
        for r in c + 1..n {
            let f = a[r][c] / piv;
            let (top, rest) = a.split_at_mut(r);
            for (x, y) in rest[0][c..].iter_mut().zip(&top[c][c..]) {
                *x -= f * y;
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for c in (0..n).rev() {
        let s: f64 = (c + 1..n).map(|k| a[c][k] * x[k]).sum();
        let piv = if a[c][c].abs() < 1e-12 {
            1e-12
        } else {
            a[c][c]
        };
        x[c] = (b[c] - s) / piv;
    }
    x
}

/// A Photo's facts and plain Auto (no Edit head) as the model reads them: measured on its
/// unedited render, with no Look. Kept in memory by file and Auto's settings, so the live
/// Shoot look preview re-matches a whole Shoot from a slider drag without rendering.
pub fn measure(lib: &Library, p: &Photo) -> Result<(Vec<(String, f64)>, Adjust)> {
    type Measured = (Vec<(String, f64)>, Adjust);
    static MEASURED: OnceLock<Mutex<HashMap<String, Measured>>> = OnceLock::new();
    let plain = crate::auto::Params {
        edit_head: false,
        ..crate::auto::Params::current()?
    };
    let stamp = |path: &str| {
        std::fs::metadata(path)
            .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
            .unwrap_or_default()
    };
    let key = format!(
        "{}|{}|{}|{:?}",
        p.path,
        stamp(&p.path),
        stamp(&p.preview.to_string_lossy()),
        plain
    );
    let cache = MEASURED.get_or_init(Default::default);
    if let Some(m) = cache.lock().unwrap().get(&key) {
        return Ok(m.clone());
    }
    let base = Recipe::default();
    let render = |s: &Adjust| {
        let r = Recipe {
            global: s.clone(),
            ..base.clone()
        };
        crate::render::render_photo(p, &r, Some(crate::auto::MEASURE_EDGE), None)
    };
    let auto = crate::auto::auto_adjust_with(render, true, &plain)?;
    let analysis = crate::render::analysis(lib, p).ok();
    let facts = crate::edit_head::facts(&render(&Adjust::default())?, &auto, analysis.as_deref());
    cache
        .lock()
        .unwrap()
        .insert(key, (facts.clone(), auto.clone()));
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
        let own = crate::tools::current_recipe(lib, p.id)?;
        if let Some(s) = sample_with(lib, p, &own)? {
            samples.push(s);
        }
    }
    Ok(samples)
}

/// One anchor as the model reads it, edited as `recipe` (saved or not, so a slider drag on
/// the hero re-matches the Shoot before it is saved). None when it can't be measured.
pub fn sample_with(lib: &Library, p: &Photo, recipe: &Recipe) -> Result<Option<Sample>> {
    let own = crate::tools::with_look(lib, p, recipe)?;
    // An odd file costs that anchor, not the match.
    let Ok((facts, auto)) = measure(lib, p) else {
        return Ok(None);
    };
    let mine = crate::edit_head::sliders(&own.global);
    let theirs = crate::edit_head::sliders(&auto);
    Ok(Some(Sample {
        facts,
        offset: std::array::from_fn(|j| mine[j] - theirs[j]),
    }))
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
    let smart = crate::jobs::config(lib)?.shoot_match_smart;
    Ok(Some((
        matched(&facts, &auto, samples, smart),
        samples.len(),
    )))
}

/// The Basic sliders (as they render, Look included) for a frame measured as `facts` and
/// `auto`, from the anchors' `samples`: no rendering, so a whole Shoot re-matches in
/// milliseconds.
pub fn matched(facts: &[(String, f64)], auto: &Adjust, samples: &[Sample], smart: bool) -> Adjust {
    let off = Model::shipped().offset_with(facts, samples, smart);
    let a = crate::edit_head::sliders(auto);
    let v: [f64; 9] = std::array::from_fn(|j| a[j] + off[j]);
    let r = |x: f64| x.round();
    Adjust {
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
    .clamped()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heroes_start_typical_then_cover_the_odd_scenes() {
        let m = Model::shipped();
        let f = m.within.mean_facts();
        let shift = |by: f64| -> Vec<(String, f64)> {
            f.iter().map(|(k, v)| (k.clone(), v + by)).collect()
        };
        // five frames of one scene (frame 2 is its centre) and two of a very different one
        let shoot: Vec<_> = [-0.02, -0.01, 0.0, 0.01, 0.02, 1e3, 1e3]
            .into_iter()
            .map(shift)
            .collect();
        assert_eq!(
            m.heroes(&shoot[..5], &[], 1),
            vec![2],
            "one hero: the most typical frame"
        );
        assert!(m.heroes(&shoot, &[], 1)[0] < 5, "from the main scene");
        let two = m.heroes(&shoot, &[], 2);
        assert_eq!(two.len(), 2);
        assert!(
            two.iter().any(|&i| i >= 5),
            "two heroes cover the odd scene: {two:?}"
        );
        assert!(two.iter().any(|&i| i < 5), "and the main one: {two:?}");
        let next = m.heroes(&shoot, &[2], 1);
        assert!(
            next[0] >= 5,
            "with the centre edited, the odd scene comes next: {next:?}"
        );
        assert!(!m
            .heroes(&shoot, &[2, 5], 3)
            .iter()
            .any(|i| [2, 5].contains(i)));
    }

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

    #[test]
    fn the_gp_reads_lookalike_anchors_as_a_closed_form_and_far_ones_lean_on_the_head() {
        let m = Model::shipped();
        let gp = m.gp.as_ref().expect("shipped model is a GP");
        let f = m.within.mean_facts();
        let none = m.within.predict(&f);
        let t = SLIDERS.iter().position(|s| *s == "temp").unwrap();
        let warm = Sample {
            facts: f.clone(),
            offset: std::array::from_fn(|j| none[j] + if j == t { 30.0 } else { 0.0 }),
        };
        // n anchors at the Photo's own facts: the miss carries n s / (n s + 1 - rho), s = 1/ratio + rho
        for n in [1usize, 3] {
            let got = m.offset(&f, &vec![warm.clone(); n]);
            let s = 1.0 / m.ratio[t] + gp.rho;
            let want = none[t] + 30.0 * n as f64 * s / (n as f64 * s + 1.0 - gp.rho);
            assert!(
                (got[t] - want).abs() < 1e-6,
                "{n} anchors: {} vs {want}",
                got[t]
            );
        }
        // an anchor from a very different scene (facts clip at 3 SD): the match leans toward the Edit head
        let near = m.offset(&f, std::slice::from_ref(&warm));
        let far: Vec<(String, f64)> = f.iter().map(|(k, v)| (k.clone(), v + 1e3)).collect();
        let odd = Sample {
            facts: far,
            offset: warm.offset,
        };
        let got = m.offset(&f, &[odd]);
        let head = Head::shipped().predict(&f);
        assert!(
            (got[t] - head[t]).abs() < 0.5 * (near[t] - head[t]).abs(),
            "far anchor leans to the head: {} vs head {} (look-alike anchor {})",
            got[t],
            head[t],
            near[t]
        );
    }

    #[test]
    fn the_off_switch_goes_back_to_the_weighted_mean() {
        let m = Model::shipped();
        let f = m.within.mean_facts();
        let none = m.within.predict(&f);
        let t = SLIDERS.iter().position(|s| *s == "temp").unwrap();
        let warm = Sample {
            facts: f.clone(),
            offset: std::array::from_fn(|j| none[j] + if j == t { 30.0 } else { 0.0 }),
        };
        let plain = m.offset_with(&f, std::slice::from_ref(&warm), false);
        let want = none[t] + 30.0 / (1.0 + m.ratio[t]);
        assert!((plain[t] - want).abs() < 1e-6, "{} vs {want}", plain[t]);
        let smart = m.offset_with(&f, std::slice::from_ref(&warm), true);
        assert!(
            (smart[t] - plain[t]).abs() > 0.1,
            "the smart match differs: {} vs {}",
            smart[t],
            plain[t]
        );
    }
}
