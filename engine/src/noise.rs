//! How noisy a RAW is, measured from its own sensor data, and the Denoise Amount that suits it
//! (Auto Denoise).
//!
//! Every sensor's noise follows the Poisson-Gaussian model: at a signal `s` (0..1 of the
//! white level, black subtracted) a site's value varies by `variance = a * s + b`, with `a`
//! growing with ISO. [`measure`] fits `a` and `b` on the mosaic itself, from the median
//! absolute difference of neighbouring same-colour sites per brightness band, so it needs no
//! per-camera table and reads a Leica Q3 the same as an A7R V. [`visible`] turns that into
//! the noise the person sees: the relative noise of the signal that ends up at mid-grey once
//! the edit's Exposure is applied (a linear push keeps the signal-to-noise ratio, so the
//! exposure only moves which signal lands mid-grey). [`auto_amount`] maps it to an AI
//! Denoise Amount, tuned on `bench/denoise/` (a clean frame given the Halloween ISO 12800
//! noise, scored against the clean original, plus a by-eye check on the real frames).

use crate::{CfaPattern, RawPhoto};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// A RAW's fitted noise: `variance = a * s + b` per site, and the median green signal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoiseModel {
    pub a: f64,
    pub b: f64,
    /// Median of the green sites, 0..1 of white: how bright the frame was exposed.
    pub median: f64,
}

/// Side of the square blocks the fit measures, in sites of one colour.
const BLOCK: usize = 16;
/// Brightness bands the fit takes a robust noise reading in.
const BANDS: usize = 40;

/// One block row's readings: (block mean, noise sigma) pairs, and green block means.
type Readings = (Vec<(f32, f32)>, Vec<f32>);

/// Fit `raw`'s noise. Reads every other block row of each colour plane, so it costs a few
/// hundred milliseconds on a 61 MP frame.
pub fn measure(raw: &RawPhoto) -> NoiseModel {
    let (cx, cy, cw, ch) = raw.crop;
    let sw = raw.sensor_width;
    // One plane per 2x2 site: (dy, dx) offsets inside the crop.
    let mut points: Vec<(f32, f32)> = Vec::new();
    let mut greens: Vec<f32> = Vec::new();
    for dy in 0..2 {
        for dx in 0..2 {
            let (sy, sx) = ((cy + dy) & 1, (cx + dx) & 1);
            let k = sy * 2 + sx;
            let scale = 1.0 / (raw.white[k] - raw.black[k]).max(1.0);
            let black = raw.black[k];
            let green = is_green(raw.cfa, sy, sx);
            let (pw, ph) = ((cw - dx) / 2, (ch - dy) / 2);
            let site = |x: usize, y: usize| -> f32 {
                let v = raw.mosaic[(cy + dy + 2 * y) * sw + cx + dx + 2 * x];
                (v as f32 - black) * scale
            };
            let rows: Vec<usize> = (0..ph / BLOCK).step_by(2).collect();
            let found: Vec<Readings> = rows
                .par_iter()
                .map(|&by| {
                    let mut pts = Vec::new();
                    let mut gs = Vec::new();
                    let mut diffs = Vec::with_capacity(BLOCK * (BLOCK - 1));
                    for bx in 0..pw / BLOCK {
                        diffs.clear();
                        let mut sum = 0f32;
                        for y in by * BLOCK..(by + 1) * BLOCK {
                            let mut prev = site(bx * BLOCK, y);
                            sum += prev;
                            for x in bx * BLOCK + 1..(bx + 1) * BLOCK {
                                let v = site(x, y);
                                sum += v;
                                diffs.push(v - prev);
                                prev = v;
                            }
                        }
                        let mean = sum / (BLOCK * BLOCK) as f32;
                        if green {
                            gs.push(mean);
                        }
                        if !(0.0005..0.6).contains(&mean) {
                            continue;
                        }
                        let mid = median(&mut diffs);
                        for d in diffs.iter_mut() {
                            *d = (*d - mid).abs();
                        }
                        // MAD of a difference of two sites: sigma * sqrt(2) * 0.6745.
                        let sigma = median(&mut diffs) / 0.6745 / std::f32::consts::SQRT_2;
                        pts.push((mean, sigma));
                    }
                    (pts, gs)
                })
                .collect();
            for (p, g) in found {
                points.extend(p);
                greens.extend(g);
            }
        }
    }
    let median_green = if greens.is_empty() {
        0.0
    } else {
        median(&mut greens).max(0.0) as f64
    };
    let (a, b) = fit(&mut points);
    NoiseModel {
        a,
        b,
        median: median_green,
    }
}

fn is_green(cfa: CfaPattern, y: usize, x: usize) -> bool {
    cfa[y][x] == 1 || cfa[y][x] == 3
}

/// Median of `v` (reorders it); 0 when empty.
fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mid = v.len() / 2;
    *v.select_nth_unstable_by(mid, f32::total_cmp).1
}

/// Least squares `sigma^2 = a * mean + b` over per-band medians: robust to the minority
/// of textured blocks a variance fit would read as noise.
fn fit(points: &mut [(f32, f32)]) -> (f64, f64) {
    if points.len() < BANDS * 10 {
        return (0.0, 0.0);
    }
    points.sort_unstable_by(|p, q| p.0.total_cmp(&q.0));
    let per = points.len() / BANDS;
    let bands: Vec<(f64, f64)> = points
        .chunks(per)
        .filter(|c| c.len() >= per / 2)
        .map(|c| {
            let mean = c.iter().map(|p| p.0 as f64).sum::<f64>() / c.len() as f64;
            let mut s: Vec<f32> = c.iter().map(|p| p.1).collect();
            let sigma = median(&mut s) as f64;
            (mean, sigma * sigma)
        })
        .collect();
    let n = bands.len() as f64;
    let (sx, sy) = bands
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
    let (mx, my) = (sx / n, sy / n);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for (x, y) in &bands {
        sxx += (x - mx) * (x - mx);
        sxy += (x - mx) * (y - my);
    }
    let a = if sxx > 0.0 { (sxy / sxx).max(0.0) } else { 0.0 };
    let b = (my - a * mx).max(0.0);
    (a, b)
}

/// The relative noise (sigma over signal) of the signal that the edit shows at mid-grey:
/// the frame's median, darkened by the Exposure the edit adds (a +1 stop push shows half
/// the signal at the same brightness). 0 when nothing was measured.
pub fn visible(model: &NoiseModel, exposure: f64) -> f64 {
    if model.median <= 0.0 || model.a <= 0.0 {
        return 0.0;
    }
    let s = (model.median * (-exposure.clamp(-5.0, 5.0)).exp2()).max(1e-4);
    (model.a * s + model.b).sqrt() / s
}

/// Below this visible noise a frame is left alone: daylight frames read 0.01-0.05 and a
/// dim ISO 1000-2000 room 0.07-0.09, and each denoised frame costs the Mac half a minute.
const CLEAN: f64 = 0.08;
/// Visible noise of the Halloween ISO 12800 frames (about 0.3).
const PARTY: f64 = 0.3;
/// Amount where Denoise starts on a frame just over `CLEAN`, the one it reaches at `PARTY`,
/// and its ceiling. On the bench the best score (PSNR and SSIM against the clean frame) sat
/// at 35-65 for ISO 3200 and 50-75 for ISO 8000-12800, lower on fine texture; by eye skin
/// and hair start to look waxy past about 70, so Auto stays a little under the scores.
const LEAST: f64 = 30.0;
const AT_PARTY: f64 = 60.0;
const MOST: f64 = 70.0;

/// The AI Denoise Amount for a frame whose visible noise is `v`: 0 for a clean frame, then
/// rising with the noise, never past [`MOST`]. Whole numbers, like the slider.
pub fn auto_amount(v: f64) -> f64 {
    if !v.is_finite() || v < CLEAN {
        return 0.0;
    }
    let t = (v - CLEAN) / (PARTY - CLEAN);
    (LEAST + t * (AT_PARTY - LEAST)).clamp(LEAST, MOST).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat-ish synthetic mosaic with known Poisson-Gaussian noise.
    fn mosaic(a: f64, b: f64, levels: &[f32]) -> RawPhoto {
        let (w, h) = (1024usize, 1024usize);
        let mut seed = 12345u64;
        let mut gauss = || {
            // Box-Muller from a tiny LCG: deterministic, no crate.
            let mut u = || {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
            };
            let (u1, u2) = (u(), u());
            (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
        };
        let mut m = vec![0u16; w * h];
        for y in 0..h {
            // Bands of brightness down the frame, so the fit sees many levels.
            let s = levels[y * levels.len() / h] as f64;
            for x in 0..w {
                let v = s + gauss() * (a * s + b).sqrt();
                m[y * w + x] = (512.0 + v * (16383.0 - 512.0)).clamp(0.0, 16383.0) as u16;
            }
        }
        RawPhoto {
            make: String::new(),
            model: String::new(),
            sensor_width: w,
            sensor_height: h,
            crop: (0, 0, w, h),
            mosaic: m,
            cfa: [[0, 1], [1, 2]],
            black: [512.0; 4],
            white: [16383.0; 4],
            wb: [2.0, 1.0, 1.5],
            xyz_to_cam: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            orientation: crate::Orientation::Normal,
            lens: None,
        }
    }

    #[test]
    fn measure_recovers_the_sensor_noise() {
        let levels: Vec<f32> = (0..32).map(|i| 0.005 + i as f32 * 0.01).collect();
        let raw = mosaic(0.0035, 1e-5, &levels);
        let m = measure(&raw);
        assert!((m.a / 0.0035 - 1.0).abs() < 0.15, "{m:?}");
        assert!((m.median - 0.165).abs() < 0.03, "{m:?}");
        let clean = measure(&mosaic(0.00003, 0.0, &levels));
        assert!(clean.a < 0.0001, "{clean:?}");
    }

    #[test]
    fn auto_amount_leaves_clean_frames_and_caps_noisy_ones() {
        let party = NoiseModel {
            a: 0.0036,
            b: 1e-5,
            median: 0.036,
        };
        let daylight = NoiseModel {
            a: 0.00004,
            b: 0.0,
            median: 0.1,
        };
        assert_eq!(auto_amount(visible(&daylight, 0.0)), 0.0);
        let at_party = auto_amount(visible(&party, 0.0));
        assert!((50.0..=65.0).contains(&at_party), "{at_party}");
        // Pushing a frame brighter shows more noise, so it gets more Denoise.
        assert!(auto_amount(visible(&party, 1.0)) > at_party);
        assert!(auto_amount(visible(&party, 4.0)) <= MOST);
        assert_eq!(auto_amount(f64::NAN), 0.0);
        assert_eq!(
            visible(
                &NoiseModel {
                    a: 0.0,
                    b: 0.0,
                    median: 0.0
                },
                0.0
            ),
            0.0
        );
    }
}
