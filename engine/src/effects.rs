//! Lightroom's Effects, Detail and Optics panels: the settings that look at a pixel's
//! neighbours, so they run on the whole image after the per-pixel stages
//! ([`crate::pipeline::Finish`]). Written from published methods, not Adobe's code:
//! dehaze from He, Sun and Tang's dark channel prior (CVPR 2009), Clarity, Texture and
//! sharpening as unsharp masks at three scales (Clarity's base edge-aware, with He et al.'s
//! guided filter), noise reduction with the guided filter on luminance and on colour, and
//! lateral chromatic aberration found by aligning the red and blue planes to green.
//!
//! Two stages: [`apply`] before the crop (Optics, Detail, Dehaze, Clarity, Texture, on the
//! sRGB-encoded floats) and [`post_crop`] after it (Lightroom's post-crop Vignette and
//! Grain, which follow the cropped frame). `scale` is this image's pixels per pixel of the
//! original, so pixel-sized settings (sharpening radius, noise) look the same in a small
//! preview as in a full-size export.

use crate::color::smooth;
use crate::masks::refine::{box_blur, guided_filter, resize};
use crate::pipeline::{srgb_decode, srgb_encode, to8, LUMA};
use crate::recipe::Adjust;
use crate::RgbImage;
use rayon::prelude::*;

/// True when [`apply`] has work: any Optics, Detail, Dehaze, Clarity or Texture setting.
pub fn active(a: &Adjust) -> bool {
    PRE_CROP.iter().any(|k| a.more(k) != 0.0)
}

const PRE_CROP: &[&str] = &[
    "texture",
    "clarity",
    "dehaze",
    "sharpen",
    "noise",
    "colorNoise",
    "removeCA",
    "defringePurple",
    "defringeGreen",
    "distortion",
    "lensVignette",
];

/// The effects a Mask can carry, as in Lightroom's masking panel.
pub const LOCAL: &[&str] = &[
    "texture",
    "clarity",
    "dehaze",
    "sharpen",
    "noise",
    "colorNoise",
    "defringePurple",
    "defringeGreen",
];

/// Only the [`LOCAL`] effects of a local adjustment's `delta`, None when it has none.
pub fn local_part(delta: &Adjust) -> Option<Adjust> {
    let mut a = Adjust::default();
    for k in LOCAL {
        if let Some(v) = delta.other.get(*k) {
            a.other.insert(k.to_string(), v.clone());
        }
    }
    active(&a).then_some(a)
}

/// Largest long edge the low-frequency parts (Clarity's base, Dehaze's haze map) are
/// worked out at; bigger images upsample them, which they don't notice.
const BASE_EDGE: usize = 2048;

/// The pre-crop stage on an sRGB-encoded image (`w * h * 3`), in Lightroom's order: lens
/// corrections, noise reduction, Dehaze, Clarity, Texture, then sharpening.
pub fn apply(img: &mut Vec<f32>, w: usize, h: usize, a: &Adjust, scale: f32) {
    if !active(a) || w < 4 || h < 4 {
        return;
    }
    let scale = scale.clamp(0.01, 1.0);
    let distortion = a.more("distortion");
    if distortion != 0.0 {
        *img = distort(img, w, h, (distortion / 100.0) as f32);
    }
    if a.more("removeCA") > 0.5 {
        remove_ca(img, w, h);
    }
    let (purple, green) = (a.more("defringePurple"), a.more("defringeGreen"));
    if purple > 0.0 || green > 0.0 {
        defringe(
            img,
            w,
            h,
            (purple / 20.0) as f32,
            (green / 20.0) as f32,
            scale,
        );
    }
    let lens_vignette = a.more("lensVignette");
    if lens_vignette != 0.0 {
        lens_vignetting(
            img,
            w,
            h,
            (lens_vignette / 100.0) as f32,
            (a.more("lensVignetteMidpoint") / 100.0) as f32,
        );
    }
    let colour = a.more("colorNoise");
    if colour > 0.0 {
        colour_noise(
            img,
            w,
            h,
            (colour / 100.0) as f32,
            (a.more("colorNoiseDetail") / 100.0) as f32,
            (a.more("colorNoiseSmoothness") / 100.0) as f32,
            scale,
        );
    }
    let noise = a.more("noise");
    if noise > 0.0 {
        let k = (noise / 100.0) as f32;
        let detail = (a.more("noiseDetail") / 100.0) as f32;
        let contrast = (a.more("noiseContrast") / 100.0) as f32;
        let r = ((2.5 * scale).round() as usize).max(1);
        let eps = (0.0004 + 0.012 * k * k) * (1.4 - detail);
        on_luma(img, w, h, |l| {
            let smooth = guided_filter(l, l, w, h, r, eps);
            l.iter()
                .zip(&smooth)
                .map(|(&v, &s)| s + (v - s) * (1.0 - k * (1.0 - 0.6 * contrast)))
                .collect()
        });
    }
    let dehaze = a.more("dehaze");
    if dehaze != 0.0 {
        dehaze_image(img, w, h, (dehaze / 100.0) as f32);
    }
    let clarity = a.more("clarity");
    if clarity != 0.0 {
        let k = (clarity / 100.0) as f32;
        on_luma(img, w, h, |l| {
            let base = low_pass(l, w, h, 1.0 / 40.0, true);
            l.iter()
                .zip(&base)
                .map(|(&v, &b)| {
                    // Midtones most, as in Lightroom; strong positive Clarity is capped
                    // per pixel so edges don't ring.
                    let m = (4.0 * v * (1.0 - v)).clamp(0.25, 1.0);
                    let d = (v - b) * if k > 0.0 { 1.2 * k } else { 0.9 * k };
                    v + (d * m).clamp(-0.25, 0.25)
                })
                .collect()
        });
    }
    let texture = a.more("texture");
    if texture != 0.0 {
        let k = (texture / 100.0) as f32;
        on_luma(img, w, h, |l| {
            let base = low_pass(l, w, h, 1.0 / 400.0, false);
            l.iter()
                .zip(&base)
                .map(|(&v, &b)| v + (v - b) * if k > 0.0 { 1.5 * k } else { 0.9 * k })
                .collect()
        });
    }
    let sharpen = a.more("sharpen");
    if sharpen > 0.0 {
        let k = (sharpen / 100.0) as f32 * 1.2;
        let sigma = (a.more("sharpenRadius") as f32 * scale).max(0.5);
        let detail = (a.more("sharpenDetail") / 100.0) as f32;
        let masking = (a.more("sharpenMasking") / 100.0) as f32;
        on_luma(img, w, h, |l| {
            let blur = gauss(l, w, h, sigma);
            let edges = (masking > 0.0).then(|| edge_mask(&blur, w, h, masking));
            l.par_iter()
                .zip(&blur)
                .enumerate()
                .map(|(i, (&v, &b))| {
                    let d = v - b;
                    // Low Detail damps big differences (halos), high Detail keeps them.
                    let d = d / (1.0 + d.abs() * (1.0 - detail) * 25.0);
                    let m = edges.as_ref().map_or(1.0, |e| e[i]);
                    v + k * d * m
                })
                .collect()
        });
    }
}

/// True when [`post_crop`] has work.
pub fn post_crop_active(a: &Adjust) -> bool {
    a.more("vignette") != 0.0 || a.more("grain") > 0.0
}

/// Post-crop Vignette and Grain on the final, cropped image.
pub fn post_crop(img: &mut RgbImage, a: &Adjust) {
    if !post_crop_active(a) || img.width < 2 || img.height < 2 {
        return;
    }
    let (w, h) = (img.width, img.height);
    let amount = (a.more("vignette") / 100.0) as f32;
    let mid = (a.more("vignetteMidpoint") / 100.0) as f32;
    let round = (a.more("vignetteRoundness") / 100.0) as f32;
    let feather = (a.more("vignetteFeather") / 100.0) as f32;
    let protect = (a.more("vignetteHighlights") / 100.0) as f32;
    let grain = (a.more("grain") / 100.0) as f32;
    let cell =
        (w.max(h) as f32 * (0.00015 + 0.0012 * (a.more("grainSize") / 100.0) as f32)).max(1.0);
    let rough = (a.more("grainRoughness") / 100.0) as f32;
    let dec: Vec<f32> = (0..256).map(|v| srgb_decode(v as f32 / 255.0)).collect();
    img.data
        .par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, px) in row.chunks_mut(3).enumerate() {
                let mut lin: [f32; 3] = std::array::from_fn(|c| dec[px[c] as usize]);
                if amount != 0.0 {
                    let wgt = vignette_weight(x, y, w, h, mid, round, feather);
                    if amount < 0.0 {
                        let luma = LUMA[0] * lin[0] + LUMA[1] * lin[1] + LUMA[2] * lin[2];
                        let keep = protect * smooth(0.5, 1.0, srgb_encode(luma));
                        let g = 1.0 + amount * wgt * (1.0 - keep);
                        lin = lin.map(|v| v * g);
                    } else {
                        lin = lin.map(|v| v + (1.0 - v) * amount * wgt * 0.8);
                    }
                }
                let mut enc = lin.map(srgb_encode);
                if grain > 0.0 {
                    let n = grain_at(x as f32, y as f32, cell, rough);
                    let luma = LUMA[0] * enc[0] + LUMA[1] * enc[1] + LUMA[2] * enc[2];
                    let m = (4.0 * luma * (1.0 - luma)).max(0.0) * 0.8 + 0.2;
                    enc = enc.map(|v| v + n * grain * 0.14 * m);
                }
                for c in 0..3 {
                    px[c] = to8(enc[c]);
                }
            }
        });
}

/// Where the post-crop vignette applies, 0 in the middle to 1 at the edge. Roundness 0
/// follows the frame's shape as an ellipse, +1 is a circle, -1 a rounded rectangle.
fn vignette_weight(
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    mid: f32,
    round: f32,
    feather: f32,
) -> f32 {
    let u = ((x as f32 + 0.5) / w as f32) * 2.0 - 1.0;
    let v = ((y as f32 + 0.5) / h as f32) * 2.0 - 1.0;
    // Toward a circle: scale the long side's coordinate up so both use short-side units.
    let (mut u, mut v) = (u.abs(), v.abs());
    if round > 0.0 {
        let aspect = w as f32 / h as f32;
        if aspect > 1.0 {
            u *= 1.0 + (aspect - 1.0) * round;
        } else {
            v *= 1.0 + (1.0 / aspect - 1.0) * round;
        }
    }
    let p = 2.0 + (-round).max(0.0) * 6.0;
    let d = (u.powf(p) + v.powf(p)).powf(1.0 / p) / 2f32.powf(1.0 / p) * 1.414;
    let centre = 0.35 + 0.75 * mid;
    let width = 0.05 + 0.9 * feather;
    smooth(centre - width * 0.5, centre + width * 0.5, d)
}

/// Film-like grain, -1..1: value noise at the grain's cell size, roughened with per-pixel
/// noise. Deterministic, so a render always looks the same.
fn grain_at(x: f32, y: f32, cell: f32, rough: f32) -> f32 {
    let (gx, gy) = (x / cell, y / cell);
    let (x0, y0) = (gx.floor(), gy.floor());
    let (tx, ty) = (gx - x0, gy - y0);
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let n = |i: f32, j: f32| hash(i as i32, j as i32);
    let a = n(x0, y0) + (n(x0 + 1.0, y0) - n(x0, y0)) * tx;
    let b = n(x0, y0 + 1.0) + (n(x0 + 1.0, y0 + 1.0) - n(x0, y0 + 1.0)) * tx;
    let smooth = a + (b - a) * ty;
    let fine = hash(x as i32 + 7919, y as i32 + 104_729);
    (smooth * (1.0 - rough * 0.6) + fine * rough * 0.6) * 1.4
}

/// Integer hash to -1..1.
fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 32767.5 - 1.0
}

/// Run `f` on the image's luma and move every channel by the change, so colour stays.
fn on_luma(img: &mut [f32], w: usize, h: usize, f: impl FnOnce(&[f32]) -> Vec<f32>) {
    let l: Vec<f32> = img
        .par_chunks(3)
        .map(|p| LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2])
        .collect();
    debug_assert_eq!(l.len(), w * h);
    let out = f(&l);
    img.par_chunks_mut(3)
        .zip(l.par_iter().zip(&out))
        .for_each(|(p, (&before, &after))| {
            let d = after - before;
            for v in p {
                *v += d;
            }
        });
}

/// A low-pass of `l` with a radius of `frac` of the long edge, worked out at most at
/// [`BASE_EDGE`]. `edge_aware` uses the guided filter, so strong edges don't halo.
fn low_pass(l: &[f32], w: usize, h: usize, frac: f32, edge_aware: bool) -> Vec<f32> {
    let long = w.max(h);
    let (sw, sh) = if long > BASE_EDGE {
        let k = BASE_EDGE as f32 / long as f32;
        (
            ((w as f32 * k) as usize).max(1),
            ((h as f32 * k) as usize).max(1),
        )
    } else {
        (w, h)
    };
    let small = resize(l, w, h, sw, sh);
    let r = ((sw.max(sh) as f32 * frac).round() as usize).max(1);
    let base = if edge_aware {
        guided_filter(&small, &small, sw, sh, r, 0.02)
    } else {
        box_blur(
            &box_blur(&box_blur(&small, sw, sh, r), sw, sh, r),
            sw,
            sh,
            r,
        )
    };
    resize(&base, sw, sh, w, h)
}

/// Gaussian blur of a plane, any sigma: a direct separable kernel when small, three box
/// blurs when large.
pub fn gauss(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    if sigma > 3.0 {
        let r = ((sigma * sigma * 4.0 + 1.0).sqrt() / 2.0).round().max(1.0) as usize;
        return box_blur(&box_blur(&box_blur(src, w, h, r), w, h, r), w, h, r);
    }
    let rad = (sigma * 3.0).ceil() as isize;
    let mut k: Vec<f32> = (-rad..=rad)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let s: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= s);
    let mut tmp = vec![0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let line = &src[y * w..(y + 1) * w];
        for (x, o) in row.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (j, &kv) in k.iter().enumerate() {
                let p = (x as isize + j as isize - rad).clamp(0, w as isize - 1) as usize;
                acc += line[p] * kv;
            }
            *o = acc;
        }
    });
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (j, &kv) in k.iter().enumerate() {
                let p = (y as isize + j as isize - rad).clamp(0, h as isize - 1) as usize;
                acc += tmp[p * w + x] * kv;
            }
            *o = acc;
        }
    });
    out
}

/// 0..1 per pixel: 1 on edges, 0 on flat areas, the threshold rising with `masking`
/// (Lightroom's sharpening Masking).
fn edge_mask(l: &[f32], w: usize, h: usize, masking: f32) -> Vec<f32> {
    let t = masking * 0.08;
    (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let gx = l[y * w + (x + 1).min(w - 1)] - l[y * w + x.saturating_sub(1)];
            let gy = l[(y + 1).min(h - 1) * w + x] - l[y.saturating_sub(1) * w + x];
            smooth(t * 0.5, t * 1.5 + 1e-3, (gx * gx + gy * gy).sqrt())
        })
        .collect()
}

/// Colour noise reduction: smooth the two colour-difference planes against luma, keeping
/// luma itself.
fn colour_noise(
    img: &mut [f32],
    w: usize,
    h: usize,
    k: f32,
    detail: f32,
    smoothness: f32,
    scale: f32,
) {
    let n = w * h;
    let mut l = vec![0f32; n];
    let mut cr = vec![0f32; n];
    let mut cb = vec![0f32; n];
    for (i, p) in img.chunks(3).enumerate() {
        l[i] = LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2];
        cr[i] = p[0] - l[i];
        cb[i] = p[2] - l[i];
    }
    let r = (((3.0 + smoothness * 9.0) * scale.max(0.2)).round() as usize).max(1);
    let eps = 0.0004 + (1.0 - detail) * 0.01;
    let scr = guided_filter(&l, &cr, w, h, r, eps);
    let scb = guided_filter(&l, &cb, w, h, r, eps);
    img.par_chunks_mut(3).enumerate().for_each(|(i, p)| {
        let r = cr[i] + (scr[i] - cr[i]) * k;
        let b = cb[i] + (scb[i] - cb[i]) * k;
        let (rr, bb) = (l[i] + r, l[i] + b);
        let gg = (l[i] - LUMA[0] * rr - LUMA[2] * bb) / LUMA[1];
        p.copy_from_slice(&[rr, gg, bb]);
    });
}

/// Dehaze by the dark channel prior: estimate the haze (the darkest channel over a patch,
/// relative to the airlight), then take it out (`k` > 0) or add it (`k` < 0).
fn dehaze_image(img: &mut [f32], w: usize, h: usize, k: f32) {
    let long = w.max(h);
    let f = (512.0 / long as f32).min(1.0);
    let (sw, sh) = (
        ((w as f32 * f) as usize).max(4),
        ((h as f32 * f) as usize).max(4),
    );
    let dark_full: Vec<f32> = img
        .par_chunks(3)
        .map(|p| p[0].min(p[1]).min(p[2]))
        .collect();
    let small = resize(&dark_full, w, h, sw, sh);
    let rad = (sw.max(sh) / 60).max(2);
    let dark = erode(&small, sw, sh, rad);
    // Airlight: the mean colour of the haziest 0.2% of pixels.
    let mut order: Vec<usize> = (0..sw * sh).collect();
    order.sort_unstable_by(|&a, &b| dark[b].total_cmp(&dark[a]));
    let top = &order[..(sw * sh / 500).max(1)];
    let mut air = [0f32; 3];
    for &i in top {
        let (x, y) = (i % sw, i / sw);
        let fx = ((x as f32 + 0.5) / f) as usize;
        let fy = ((y as f32 + 0.5) / f) as usize;
        let j = (fy.min(h - 1) * w + fx.min(w - 1)) * 3;
        for c in 0..3 {
            air[c] += img[j + c];
        }
    }
    air = air.map(|v| (v / top.len() as f32).clamp(0.5, 1.0));
    let amin = air[0].min(air[1]).min(air[2]);
    let guide: Vec<f32> = resize(
        &img.par_chunks(3)
            .map(|p| LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2])
            .collect::<Vec<_>>(),
        w,
        h,
        sw,
        sh,
    );
    let haze: Vec<f32> = dark.iter().map(|d| d / amin).collect();
    let haze = guided_filter(&guide, &haze, sw, sh, rad * 2, 0.001);
    let haze = resize(&haze, sw, sh, w, h);
    img.par_chunks_mut(3).zip(&haze).for_each(|(p, &hz)| {
        let hz = hz.clamp(0.0, 1.0);
        if k > 0.0 {
            let t = (1.0 - 0.95 * k * hz).max(0.15);
            for c in 0..3 {
                p[c] = (p[c] - air[c]) / t + air[c];
            }
        } else {
            let m = -k * 0.7 * (0.3 + 0.7 * (1.0 - hz));
            for c in 0..3 {
                p[c] += (air[c] - p[c]) * m;
            }
        }
    });
}

/// Minimum over a (2r+1)² box (separable).
fn erode(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let (a, b) = (x.saturating_sub(r), (x + r + 1).min(w));
            *o = src[y * w + a..y * w + b]
                .iter()
                .fold(f32::MAX, |m, &v| m.min(v));
        }
    });
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let (a, b) = (y.saturating_sub(r), (y + r + 1).min(h));
        for (x, o) in row.iter_mut().enumerate() {
            *o = (a..b).fold(f32::MAX, |m, yy| m.min(tmp[yy * w + x]));
        }
    });
    out
}

/// Sample channel `c` of `img` at (x, y) bilinearly, clamped to the frame.
fn sample(img: &[f32], w: usize, h: usize, x: f32, y: f32, c: usize) -> f32 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let at = |xx: usize, yy: usize| img[(yy * w + xx) * 3 + c];
    let a = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
    let b = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
    a + (b - a) * ty
}

/// Manual lens Distortion, `k` -1..1: + straightens barrel distortion (pulls the middle in),
/// - straightens pincushion. Zooms just enough that no empty edge shows.
fn distort(img: &[f32], w: usize, h: usize, k: f32) -> Vec<f32> {
    let k = k * 0.15;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let rn = (cx * cx + cy * cy).sqrt();
    // Source radius for an output radius r (both / rn): r * (1 - k r^2), scaled so the
    // farthest sample (a corner for -k, the middle of the long edge for +k) stays inside.
    let f = |r: f32| r * (1.0 - k * r * r);
    let edge = cx.max(cy) / rn;
    let zoom = (1.0 / f(1.0)).min(edge / f(edge)).min(1.0);
    let mut out = vec![0f32; w * h * 3];
    out.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let (dx, dy) = ((x as f32 + 0.5 - cx) / rn, (y as f32 + 0.5 - cy) / rn);
            let r = (dx * dx + dy * dy).sqrt();
            let s = if r > 0.0 { zoom * f(r) / r } else { zoom };
            let (sx, sy) = (cx + dx * s * rn - 0.5, cy + dy * s * rn - 0.5);
            for c in 0..3 {
                row[x * 3 + c] = sample(img, w, h, sx, sy, c);
            }
        }
    });
    out
}

/// Lens vignetting correction, `k` -1..1 (+ brightens the corners), in linear light.
fn lens_vignetting(img: &mut [f32], w: usize, h: usize, k: f32, mid: f32) {
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let rn = (cx * cx + cy * cy).sqrt();
    let start = 0.1 + 0.6 * mid;
    img.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_mut(3).enumerate() {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let r = (dx * dx + dy * dy).sqrt() / rn;
            let g = 2f32.powf(k * smooth(start, 1.0, r) * 1.2);
            for v in p.iter_mut() {
                *v = srgb_encode(srgb_decode(v.clamp(0.0, 1.0)) * g);
            }
        }
    });
}

/// Remove lateral chromatic aberration: find the radial scale that best lines the red and
/// the blue plane up with green on the image's edges, then resample them.
fn remove_ca(img: &mut [f32], w: usize, h: usize) {
    let long = w.max(h);
    let f = (1024.0 / long as f32).min(1.0);
    let (sw, sh) = (
        ((w as f32 * f) as usize).max(8),
        ((h as f32 * f) as usize).max(8),
    );
    let plane = |c: usize| -> Vec<f32> {
        let p: Vec<f32> = img.chunks(3).map(|q| q[c]).collect();
        resize(&p, w, h, sw, sh)
    };
    let (r, g, b) = (plane(0), plane(1), plane(2));
    // Edge pixels away from the centre, where lateral CA shows.
    let (cx, cy) = (sw as f32 / 2.0, sh as f32 / 2.0);
    let rn = (cx * cx + cy * cy).sqrt();
    let mut pts: Vec<(usize, usize)> = Vec::new();
    for y in 1..sh - 1 {
        for x in 1..sw - 1 {
            let gx = g[y * sw + x + 1] - g[y * sw + x - 1];
            let gy = g[(y + 1) * sw + x] - g[(y - 1) * sw + x];
            let rr = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / rn;
            if gx * gx + gy * gy > 0.01 && rr > 0.3 {
                pts.push((x, y));
            }
        }
    }
    if pts.len() < 200 {
        return;
    }
    let one = |src: &[f32], s: f32| -> f32 {
        pts.iter()
            .map(|&(x, y)| {
                let sx = cx + (x as f32 - cx) * (1.0 + s);
                let sy = cy + (y as f32 - cy) * (1.0 + s);
                let v = sample_plane(src, sw, sh, sx, sy);
                (v - g[y * sw + x]).powi(2)
            })
            .sum()
    };
    let best = |src: &[f32]| -> f32 {
        let mut best = (0.0, one(src, 0.0));
        for i in -12..=12 {
            let s = i as f32 * 0.00025;
            let e = one(src, s);
            if e < best.1 * 0.995 {
                best = (s, e);
            }
        }
        best.0
    };
    let (sr, sb) = (best(&r), best(&b));
    if sr == 0.0 && sb == 0.0 {
        return;
    }
    let src = img.to_vec();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    img.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            for (c, s) in [(0, sr), (2, sb)] {
                if s != 0.0 {
                    let sx = cx + (x as f32 + 0.5 - cx) * (1.0 + s) - 0.5;
                    let sy = cy + (y as f32 + 0.5 - cy) * (1.0 + s) - 0.5;
                    row[x * 3 + c] = sample(&src, w, h, sx, sy, c);
                }
            }
        }
    });
}

fn sample_plane(p: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let a = p[y0 * w + x0] + (p[y0 * w + x1] - p[y0 * w + x0]) * tx;
    let b = p[y1 * w + x0] + (p[y1 * w + x1] - p[y1 * w + x0]) * tx;
    a + (b - a) * ty
}

/// Defringe: take the colour out of purple (and green) fringes next to strong edges.
fn defringe(img: &mut [f32], w: usize, h: usize, purple: f32, green: f32, scale: f32) {
    let l: Vec<f32> = img
        .par_chunks(3)
        .map(|p| LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2])
        .collect();
    let grad: Vec<f32> = (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let gx = l[y * w + (x + 1).min(w - 1)] - l[y * w + x.saturating_sub(1)];
            let gy = l[(y + 1).min(h - 1) * w + x] - l[y.saturating_sub(1) * w + x];
            if gx * gx + gy * gy > 0.04 {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    let r = ((4.0 * scale).round() as usize).max(1);
    let near = box_blur(&grad, w, h, r);
    img.par_chunks_mut(3).zip(&near).for_each(|(p, &n)| {
        if n <= 0.0 {
            return;
        }
        let (hue, s, _) = crate::pipeline::rgb_to_hsl([p[0], p[1], p[2]]);
        let k = if (260.0..=330.0).contains(&hue) {
            purple
        } else if (75.0..=160.0).contains(&hue) {
            green
        } else {
            0.0
        };
        if k <= 0.0 || s < 0.05 {
            return;
        }
        let m = (n * 4.0).min(1.0) * k;
        let y = LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2];
        for v in p.iter_mut() {
            *v += (y - *v) * m;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn adjust(v: serde_json::Value) -> Adjust {
        serde_json::from_value(v).unwrap()
    }

    /// A soft gradient with a hard edge in the middle and a little noise.
    fn scene(w: usize, h: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let base = if x < w / 2 { 0.3 } else { 0.7 };
                let n = hash(x as i32, y as i32) * 0.02;
                let v = base + 0.1 * (y as f32 / h as f32) + n;
                out.extend_from_slice(&[v, v * 0.95, v * 0.9]);
            }
        }
        out
    }

    fn luma(img: &[f32]) -> Vec<f32> {
        img.chunks(3)
            .map(|p| LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2])
            .collect()
    }

    fn roughness(l: &[f32], w: usize) -> f32 {
        l.windows(2)
            .enumerate()
            .filter(|(i, _)| (i % w) + 1 < w && (i % w) != w / 2 - 1)
            .map(|(_, p)| (p[1] - p[0]).abs())
            .sum::<f32>()
    }

    #[test]
    fn neutral_settings_do_nothing() {
        let (w, h) = (64, 48);
        let mut img = scene(w, h);
        let before = img.clone();
        apply(&mut img, w, h, &Adjust::default(), 1.0);
        assert_eq!(img, before);
        assert!(!active(&adjust(
            json!({ "sharpenRadius": 2, "grainSize": 80 })
        )));
    }

    #[test]
    fn sharpening_and_texture_add_detail_noise_reduction_takes_it_away() {
        let (w, h) = (96, 64);
        let base = roughness(&luma(&scene(w, h)), w);
        let run = |v| {
            let mut img = scene(w, h);
            apply(&mut img, w, h, &adjust(v), 1.0);
            roughness(&luma(&img), w)
        };
        assert!(run(json!({ "sharpen": 100 })) > base * 1.2);
        assert!(run(json!({ "texture": 80 })) > base * 1.1);
        assert!(run(json!({ "texture": -80 })) < base);
        assert!(run(json!({ "noise": 80 })) < base * 0.8);
    }

    #[test]
    fn clarity_raises_contrast_across_an_edge() {
        let (w, h) = (128, 64);
        let mut img = scene(w, h);
        apply(&mut img, w, h, &adjust(json!({ "clarity": 100 })), 1.0);
        let l = luma(&img);
        let row = h / 2 * w;
        let (dark, light) = (l[row + w / 2 - 3], l[row + w / 2 + 3]);
        let l0 = luma(&scene(w, h));
        assert!(light - dark > l0[row + w / 2 + 3] - l0[row + w / 2 - 3]);
    }

    #[test]
    fn dehaze_deepens_a_hazy_image_and_negative_adds_haze() {
        let (w, h) = (64, 64);
        let hazy: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let v = 0.55 + 0.2 * ((i % w) as f32 / w as f32);
                [v, v, v + 0.05]
            })
            .collect();
        let spread = |img: &[f32]| {
            let l = luma(img);
            l.iter().cloned().fold(f32::MIN, f32::max) - l.iter().cloned().fold(f32::MAX, f32::min)
        };
        let mut more = hazy.clone();
        apply(&mut more, w, h, &adjust(json!({ "dehaze": 80 })), 1.0);
        assert!(spread(&more) > spread(&hazy) * 1.2);
        let mut less = hazy.clone();
        apply(&mut less, w, h, &adjust(json!({ "dehaze": -80 })), 1.0);
        assert!(spread(&less) < spread(&hazy));
    }

    #[test]
    fn colour_noise_reduction_keeps_luma() {
        let (w, h) = (48, 48);
        let mut img: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let n = hash(i as i32, 3) * 0.1;
                [0.5 + n, 0.5 - n * 0.5, 0.5 + n * 0.3]
            })
            .collect();
        let l0 = luma(&img);
        apply(&mut img, w, h, &adjust(json!({ "colorNoise": 100 })), 1.0);
        let l1 = luma(&img);
        assert!(l0.iter().zip(&l1).all(|(a, b)| (a - b).abs() < 1e-4));
        let chroma: f32 = img.chunks(3).map(|p| (p[0] - p[2]).abs()).sum();
        assert!(chroma / ((w * h) as f32) < 0.05);
    }

    #[test]
    fn distortion_fills_the_frame() {
        let (w, h) = (80, 60);
        for k in [-100.0, 100.0] {
            let mut img = vec![0.5f32; w * h * 3];
            apply(&mut img, w, h, &adjust(json!({ "distortion": k })), 1.0);
            assert!(img.iter().all(|&v| (v - 0.5).abs() < 1e-4));
        }
    }

    #[test]
    fn post_crop_vignette_darkens_corners_and_grain_is_deterministic() {
        let mk = || RgbImage {
            width: 120,
            height: 80,
            data: vec![128; 120 * 80 * 3],
        };
        let mut v = mk();
        post_crop(&mut v, &adjust(json!({ "vignette": -60 })));
        let at = |img: &RgbImage, x: usize, y: usize| img.data[(y * img.width + x) * 3];
        assert!(at(&v, 1, 1) < 100);
        assert_eq!(at(&v, 60, 40), 128);
        let mut g1 = mk();
        let mut g2 = mk();
        post_crop(&mut g1, &adjust(json!({ "grain": 50 })));
        post_crop(&mut g2, &adjust(json!({ "grain": 50 })));
        assert_eq!(g1.data, g2.data);
        assert_ne!(g1.data, mk().data);
    }
}
