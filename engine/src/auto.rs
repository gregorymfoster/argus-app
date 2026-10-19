//! Auto: Lightroom's Auto button. A fast, deterministic first pass at exposure, tone,
//! white balance and vibrance, worked out from small renders of the Photo itself (no model).
//!
//! It measures the Photo the way it will be seen, through the real pipeline with its Look,
//! Correction and crop, and sets the sliders Lightroom's Auto sets: Exposure, Contrast,
//! Highlights, Shadows, Whites, Blacks and Vibrance, plus white balance (Temp, Tint) the
//! way Lightroom's "Auto" white balance does. Every other setting (HSL, Masks, crop, the
//! Look) is kept, so Auto is a starting point the person, Suggestions and the Agent can
//! build on.

use crate::recipe::{Adjust, Recipe};
use crate::{Result, RgbImage};

/// The sliders Auto owns. Running Auto replaces these and leaves the rest of the Recipe.
pub const AUTO_SLIDERS: &[&str] = &[
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "temp",
    "tint",
    "vibrance",
];

/// Long edge of the renders Auto measures. Small enough that the ~12 renders it makes
/// cost less than the RAW decode.
pub const MEASURE_EDGE: usize = 384;

/// Brightness Auto aims for: the mean of the sRGB-encoded luma. Greg's own Lightroom edits
/// in fixtures/bench average 0.48 (0.35 for a moody scene to 0.65 for a bright beach).
const TARGET_MEAN: f64 = 0.47;
/// How much of a dark Photo's own key Auto keeps: it lifts a dark Photo halfway, so a dark
/// scene stays darker than a bright one, as in Greg's edits.
const KEEP_KEY: f64 = 0.5;
// A bright Photo keeps its exposure (Highlights recover clipping; a blown-out one comes down a little): Greg
// rarely darkens, and pulling bright Photos toward mid-grey cost up to 12 L* against his
// edits. Tuned with `python3 bench/score.py --only edit --auto` (2026-10-18) on a cloud
// bench where 11 of the 14 cases were camera JPEGs: dE 8.97 -> 6.74 on those.
//
// A RAW is different: the pipeline's Default Look already lifts it toward Greg's edits
// (+0.15 EV, Shadows +35, Highlights -20), so the JPEG rescue on top of that overshot. On
// all 14 bench RAWs (Greg's Mac, d58f0dd) no edit was dE 7.36 and that Auto 8.02, closer on
// only 3; the worst cases brightened an already-bright develop. So on a RAW Auto keeps the
// Look's brightness and tone and only fixes what the Look can't know about the Photo:
// white balance, clipped highlights, the white and black points and vibrance.
// On the 3 RAWs in fixtures/raw that took Auto from dE 8.32 to 7.23 (no edit 7.32), the
// camera JPEGs unchanged; the other 11 RAWs still need a re-score on the Mac.

/// `recipe` with Auto's sliders cleared: what Auto measures from.
pub fn cleared(recipe: &Recipe) -> Recipe {
    let mut r = recipe.clone();
    let g = &mut r.global;
    for v in [
        &mut g.exposure,
        &mut g.contrast,
        &mut g.highlights,
        &mut g.shadows,
        &mut g.whites,
        &mut g.blacks,
        &mut g.temp,
        &mut g.tint,
        &mut g.vibrance,
    ] {
        *v = 0.0;
    }
    r
}

/// `recipe` with Auto's sliders set to `auto`'s (the rest of the Recipe kept).
pub fn applied(recipe: &Recipe, auto: &Adjust) -> Recipe {
    let mut r = cleared(recipe);
    let g = &mut r.global;
    g.exposure = auto.exposure;
    g.contrast = auto.contrast;
    g.highlights = auto.highlights;
    g.shadows = auto.shadows;
    g.whites = auto.whites;
    g.blacks = auto.blacks;
    g.temp = auto.temp;
    g.tint = auto.tint;
    g.vibrance = auto.vibrance;
    r.global = r.global.clone().clamped();
    r
}

/// Auto white balance (Temp, Tint) for a render made with neither set: the mid-tones'
/// average pulled a third of the way toward grey. None for a Photo without mid-tones.
pub fn white_balance(img: &RgbImage) -> Option<(f64, f64)> {
    let [r, g, b] = Stats::of(img).mid_mean?;
    // +1 of temp moves R/B by one stop (pipeline::wb_gains); +1 of tint moves G by half a stop.
    let temp = (35.0 * (b / r).log2()).clamp(-25.0, 25.0).round();
    let m = (r * 2f64.powf(temp / 200.0) + b * 2f64.powf(-temp / 200.0)) / 2.0;
    let tint = (70.0 * (g / m).log2()).clamp(-15.0, 15.0).round();
    Some((temp, tint))
}

/// Work out Auto's sliders. `render` renders the Photo with Auto's sliders set to the
/// given ones (on top of everything else in its Recipe), small. `raw`: the Photo is a RAW,
/// whose render already carries the Default Look's tone fit.
pub fn auto_adjust(render: impl Fn(&Adjust) -> Result<RgbImage>, raw: bool) -> Result<Adjust> {
    let mut a = Adjust::default();

    // White balance: pull the average of the mid-tones a third of the way toward grey, as
    // camera white balance is usually close and a sunset should stay warm (on Greg's bench
    // Photos, going halfway overshot his own white balance).
    if let Some((temp, tint)) = white_balance(&render(&a)?) {
        (a.temp, a.tint) = (temp, tint);
    }

    // Tone, from where the light sits once the brightness is about right.
    let start = Stats::of(&render(&a)?).mean;
    // A bright Photo keeps its brightness, blown-out sky included: on the bench Greg kept
    // even a quarter of a frame clipped rather than darken it.
    let target = if !raw && start <= TARGET_MEAN {
        TARGET_MEAN + KEEP_KEY * (start - TARGET_MEAN)
    } else {
        start
    };
    // On a RAW, Exposure stays where the Look put it: re-solving it after Highlights pulled
    // the top down brightened the mid-tones of Photos Greg had kept darker.
    let solve = |a: &Adjust| {
        if raw {
            Ok(0.0)
        } else {
            solve_exposure(&render, a, target)
        }
    };
    a.exposure = solve(&a)?;
    let s = Stats::of(&render(&a)?);
    a.highlights = -(s.above(0.93) * 500.0).clamp(0.0, 60.0).round();
    if !raw {
        a.shadows = (s.below(0.10) * 250.0).clamp(0.0, 50.0).round();
        a.contrast = ((0.21 - s.std) * 250.0).clamp(-25.0, 30.0).round();
    }

    // White and black points: stretch toward the ends without clipping.
    a.exposure = solve(&a)?;
    let s = Stats::of(&render(&a)?);
    let (lo, hi) = (s.pct(0.005), s.pct(0.995));
    a.whites = if hi < 0.93 {
        ((0.96 - hi) * 300.0).clamp(0.0, 40.0).round()
    } else {
        0.0
    };
    a.blacks = if lo > 0.04 {
        -((lo - 0.02) * 300.0).clamp(0.0, 40.0).round()
    } else if s.below(0.01) > 0.02 {
        (s.below(0.01) * 300.0).clamp(0.0, 25.0).round()
    } else {
        0.0
    };
    a.vibrance = ((0.22 - s.chroma) * 120.0).clamp(0.0, 25.0).round();

    a.exposure = solve(&a)?;
    Ok(a)
}

/// The exposure (stops, to 0.05) that puts the mean luma on `target`, the other sliders
/// as in `a`.
fn solve_exposure(
    render: &impl Fn(&Adjust) -> Result<RgbImage>,
    a: &Adjust,
    target: f64,
) -> Result<f64> {
    let mean = |ev: f64| -> Result<f64> {
        Ok(Stats::of(&render(&Adjust {
            exposure: ev,
            ..a.clone()
        })?)
        .mean)
    };
    let (mut lo, mut hi) = (-3.0, 3.0);
    for _ in 0..7 {
        let mid = (lo + hi) / 2.0;
        if mean(mid)? < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok((((lo + hi) / 2.0) * 20.0).round() / 20.0)
}

/// What Auto reads from a render: the luma histogram (sRGB-encoded) and colour.
struct Stats {
    hist: [u32; 256],
    n: u32,
    mean: f64,
    std: f64,
    /// Mean chroma (max - min channel), encoded.
    chroma: f64,
    /// Mean linear R, G, B of the unclipped mid-tones, if there are enough of them.
    mid_mean: Option<[f64; 3]>,
}

impl Stats {
    fn of(img: &RgbImage) -> Stats {
        let lin = |v: u8| {
            let c = v as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let mut hist = [0u32; 256];
        let (mut sum, mut sq, mut chroma) = (0.0, 0.0, 0.0);
        let (mut mid, mut mid_n) = ([0.0f64; 3], 0u32);
        let px = img.data.chunks_exact(3);
        let n = px.len() as u32;
        for p in px {
            let y = (0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64) / 255.0;
            hist[(y * 255.0).round() as usize] += 1;
            sum += y;
            sq += y * y;
            let (mx, mn) = (p.iter().max().unwrap(), p.iter().min().unwrap());
            chroma += (mx - mn) as f64 / 255.0;
            if (0.15..0.85).contains(&y) && *mx < 250 {
                for c in 0..3 {
                    mid[c] += lin(p[c]);
                }
                mid_n += 1;
            }
        }
        let nf = n.max(1) as f64;
        let mean = sum / nf;
        Stats {
            hist,
            n,
            mean,
            std: (sq / nf - mean * mean).max(0.0).sqrt(),
            chroma: chroma / nf,
            mid_mean: (mid_n as f64 > nf * 0.05).then(|| mid.map(|v| v / mid_n as f64)),
        }
    }

    /// Fraction of pixels with luma above `t`.
    fn above(&self, t: f64) -> f64 {
        let from = (t * 255.0).ceil() as usize;
        self.hist[from.min(255)..].iter().sum::<u32>() as f64 / self.n.max(1) as f64
    }

    /// Fraction of pixels with luma below `t`.
    fn below(&self, t: f64) -> f64 {
        let to = (t * 255.0).floor() as usize;
        self.hist[..to.min(256)].iter().sum::<u32>() as f64 / self.n.max(1) as f64
    }

    /// The luma at percentile `p` (0..1).
    fn pct(&self, p: f64) -> f64 {
        let want = (p * self.n as f64) as u32;
        let mut acc = 0;
        for (i, &c) in self.hist.iter().enumerate() {
            acc += c;
            if acc > want {
                return i as f64 / 255.0;
            }
        }
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develop_rgb8;

    /// A soft gradient scene with a colour cast, `gain` darker or brighter than it should be.
    fn scene(gain: f64, cast: [f64; 3]) -> RgbImage {
        let (w, h) = (96, 64);
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let base = 0.08 + 0.8 * (x as f64 / w as f64) * (0.6 + 0.4 * (y as f64 / h as f64));
                let hue = [
                    1.0,
                    0.9 + 0.2 * (y as f64 / h as f64),
                    0.8 + 0.3 * (x as f64 / w as f64),
                ];
                for c in 0..3 {
                    let v = (base * hue[c] * cast[c] * gain).clamp(0.0, 1.0);
                    data.push((v * 255.0).round() as u8);
                }
            }
        }
        RgbImage {
            width: w,
            height: h,
            data,
        }
    }

    fn auto_on(img: &RgbImage) -> (Adjust, Stats) {
        auto_with(img, false)
    }

    fn auto_with(img: &RgbImage, raw: bool) -> (Adjust, Stats) {
        let render = |a: &Adjust| Ok(develop_rgb8(img, &applied(&Recipe::default(), a)));
        let a = auto_adjust(render, raw).unwrap();
        let s = Stats::of(&render(&a).unwrap());
        (a, s)
    }

    #[test]
    fn dark_photo_is_lifted_and_bright_one_keeps_its_key() {
        let img = scene(0.55, [1.0; 3]);
        let before = Stats::of(&develop_rgb8(&img, &Recipe::default())).mean;
        let (dark, s) = auto_on(&img);
        assert!(dark.exposure > 0.5, "{dark:?}");
        // Most of the way to the target, keeping a little of the dark key.
        assert!(
            s.mean > before + 0.5 * (TARGET_MEAN - before),
            "{before} -> {} {dark:?}",
            s.mean
        );
        assert!(s.mean < TARGET_MEAN + 0.02);
        // A bright Photo keeps its brightness; Highlights pull back what's near clipping.
        let (bright, _) = auto_on(&scene(1.3, [1.0; 3]));
        assert!(bright.highlights < 0.0, "{bright:?}");
        assert!(bright.exposure.abs() <= 0.3, "{bright:?}");
    }

    #[test]
    fn raw_keeps_the_default_looks_brightness_and_tone() {
        let img = scene(0.55, [0.8, 0.95, 1.25]);
        let before = Stats::of(&develop_rgb8(&img, &Recipe::default())).mean;
        let (a, s) = auto_with(&img, true);
        assert_eq!((a.shadows, a.contrast), (0.0, 0.0), "{a:?}");
        assert!(
            (s.mean - before).abs() < 0.02,
            "{before} -> {} {a:?}",
            s.mean
        );
        // What the Look can't know is still fixed.
        assert!(a.temp > 5.0, "{a:?}");
    }

    #[test]
    fn blue_cast_is_warmed() {
        let (a, _) = auto_on(&scene(1.0, [0.8, 0.95, 1.25]));
        assert!(a.temp > 5.0, "{a:?}");
    }

    #[test]
    fn auto_keeps_the_rest_of_the_recipe() {
        let mut r = Recipe::default();
        r.global.saturation = 12.0;
        r.global.exposure = 1.5;
        let out = applied(
            &r,
            &Adjust {
                exposure: 0.4,
                shadows: 20.0,
                ..Default::default()
            },
        );
        assert_eq!(out.global.saturation, 12.0);
        assert_eq!(out.global.exposure, 0.4);
        assert_eq!(out.global.shadows, 20.0);
    }
}
