//! Explicit pipeline stages. Each is a pure function over flat buffers so it can
//! be ported 1:1 to a WGSL compute shader.

use crate::{CfaPattern, Orientation, RawPhoto, RgbImage};
use rayon::prelude::*;

/// Stage 1: subtract per-site black level, divide by (white - black), clamp to 0..1.
/// Output is the mosaic cropped to `raw.crop`, as f32.
pub fn normalize(raw: &RawPhoto) -> Vec<f32> {
    let (cx, cy, cw, ch) = raw.crop;
    let sw = raw.sensor_width;
    let scale: [f32; 4] = std::array::from_fn(|i| 1.0 / (raw.white[i] - raw.black[i]).max(1.0));
    let mut out = vec![0f32; cw * ch];
    out.par_chunks_mut(cw).enumerate().for_each(|(y, row)| {
        let sy = y + cy;
        let src = &raw.mosaic[sy * sw + cx..sy * sw + cx + cw];
        for (x, (o, &s)) in row.iter_mut().zip(src).enumerate() {
            let k = (sy & 1) * 2 + ((x + cx) & 1);
            *o = ((s as f32 - raw.black[k]) * scale[k]).clamp(0.0, 1.0);
        }
    });
    out
}

/// Stage 2: multiply each CFA site by its as-shot WB multiplier, normalised to green = 1.
/// Result is clamped at 1.0 (clipped highlights stay neutral rather than tinting).
pub fn white_balance(mosaic: &mut [f32], width: usize, cfa: CfaPattern, wb: [f32; 3]) {
    let g = if wb[1] > 0.0 { wb[1] } else { 1.0 };
    let mul = wb_multipliers(wb, g);
    mosaic
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, v) in row.iter_mut().enumerate() {
                let c = cfa[y & 1][x & 1] as usize;
                *v = (*v * mul[c.min(2)]).min(1.0);
            }
        });
}

/// Stages 1+2 fused: black-subtract, scale, clamp, white-balance, clamp. Equivalent to
/// [`normalize`] followed by [`white_balance`] but touches the mosaic once.
pub fn normalize_white_balance(raw: &RawPhoto) -> Vec<f32> {
    let (cx, cy, cw, ch) = raw.crop;
    let sw = raw.sensor_width;
    let g = if raw.wb[1] > 0.0 { raw.wb[1] } else { 1.0 };
    let mul = wb_multipliers(raw.wb, g);
    // Per 2x2 site (row-major, parity of the sensor origin == parity of the crop origin).
    let black = raw.black;
    let scale: [f32; 4] = std::array::from_fn(|i| 1.0 / (raw.white[i] - raw.black[i]).max(1.0));
    let gain: [f32; 4] = std::array::from_fn(|k| mul[(raw.cfa[k >> 1][k & 1] as usize).min(2)]);
    let mut out = vec![0f32; cw * ch];
    out.par_chunks_mut(cw)
        .enumerate()
        .with_min_len(8)
        .for_each(|(y, row)| {
            let sy = y + cy;
            let src = &raw.mosaic[sy * sw + cx..sy * sw + cx + cw];
            let base = (sy & 1) * 2;
            for (x, (o, &s)) in row.iter_mut().zip(src).enumerate() {
                let k = base + (x & 1);
                *o = (((s as f32 - black[k]) * scale[k]).clamp(0.0, 1.0) * gain[k]).min(1.0);
            }
        });
    out
}

/// Pure helper: WB multipliers (R,G,B) normalised so that `norm` maps to 1.0.
pub fn wb_multipliers(wb: [f32; 3], norm: f32) -> [f32; 3] {
    [wb[0] / norm, wb[1] / norm, wb[2] / norm]
}

/// Stage 3 (full res): bilinear demosaic. Output interleaved RGB f32.
/// For every pixel, the sampled channel is copied and each missing channel is the
/// mean of the same-coloured pixels in the 3x3 neighbourhood.
pub fn demosaic_bilinear(m: &[f32], w: usize, h: usize, cfa: CfaPattern) -> Vec<f32> {
    let mut out = vec![0f32; w * h * 3];
    out.par_chunks_mut(w * 3)
        .enumerate()
        .with_min_len(8)
        .for_each(|(y, orow)| demosaic_bilinear_row(m, w, h, cfa, y, orow));
    out
}

/// One pixel of the bilinear demosaic by the general rule (any position, handles borders).
fn demosaic_pixel_generic(
    m: &[f32],
    w: usize,
    h: usize,
    cfa: CfaPattern,
    x: usize,
    y: usize,
) -> [f32; 3] {
    let (y0, y1) = (y.saturating_sub(1), (y + 1).min(h - 1));
    let (x0, x1) = (x.saturating_sub(1), (x + 1).min(w - 1));
    let mut sum = [0f32; 3];
    let mut cnt = [0u8; 3];
    for yy in y0..=y1 {
        for xx in x0..=x1 {
            let c = cfa[yy & 1][xx & 1] as usize;
            sum[c] += m[yy * w + xx];
            cnt[c] += 1;
        }
    }
    let own = cfa[y & 1][x & 1] as usize;
    std::array::from_fn(|c| {
        if c == own {
            m[y * w + x]
        } else {
            sum[c] / cnt[c].max(1) as f32
        }
    })
}

/// Stage 3 building block: demosaic output row `y` into `orow` (`w * 3` floats).
/// Interior pixels use closed-form neighbour means (Bayer: a green site has its two
/// horizontal neighbours of one chroma colour and its two vertical neighbours of the other;
/// a chroma site has 4 orthogonal greens and 4 diagonal opposite-chroma sites); borders use
/// [`demosaic_pixel_generic`]. Identical results to the generic rule.
pub fn demosaic_bilinear_row(
    m: &[f32],
    w: usize,
    h: usize,
    cfa: CfaPattern,
    y: usize,
    orow: &mut [f32],
) {
    let interior_row = y > 0 && y + 1 < h && w >= 3;
    if !interior_row {
        for x in 0..w {
            orow[x * 3..x * 3 + 3].copy_from_slice(&demosaic_pixel_generic(m, w, h, cfa, x, y));
        }
        return;
    }
    orow[0..3].copy_from_slice(&demosaic_pixel_generic(m, w, h, cfa, 0, y));
    orow[(w - 1) * 3..w * 3].copy_from_slice(&demosaic_pixel_generic(m, w, h, cfa, w - 1, y));
    let (r0, r1, r2) = (
        &m[(y - 1) * w..y * w],
        &m[y * w..(y + 1) * w],
        &m[(y + 1) * w..(y + 2) * w],
    );
    let py = y & 1;
    for x in 1..w - 1 {
        let own = cfa[py][x & 1] as usize;
        let c = r1[x];
        let px = &mut orow[x * 3..x * 3 + 3];
        if own != 1 {
            px[own] = c;
            px[1] = (r0[x] + r2[x] + r1[x - 1] + r1[x + 1]) * 0.25;
            px[2 - own] = (r0[x - 1] + r0[x + 1] + r2[x - 1] + r2[x + 1]) * 0.25;
        } else {
            px[1] = c;
            px[cfa[py][(x + 1) & 1] as usize] = (r1[x - 1] + r1[x + 1]) * 0.5;
            px[cfa[(y + 1) & 1][x & 1] as usize] = (r0[x] + r2[x]) * 0.5;
        }
    }
}

/// Stage 3 (fast path): half-resolution demosaic. Each 2x2 quad becomes one RGB pixel
/// (R, mean of the two G, B). Returns (rgb, width/2, height/2).
pub fn demosaic_half(m: &[f32], w: usize, h: usize, cfa: CfaPattern) -> (Vec<f32>, usize, usize) {
    let (ow, oh) = (w / 2, h / 2);
    // Which quad position holds R, B and the two Gs.
    let mut cnt = [0f32; 3];
    for row in cfa {
        for c in row {
            cnt[c as usize] += 1.0;
        }
    }
    let inv: [f32; 3] = std::array::from_fn(|c| 1.0 / cnt[c].max(1.0));
    let mut out = vec![0f32; ow * oh * 3];
    out.par_chunks_mut(ow * 3)
        .enumerate()
        .with_min_len(8)
        .for_each(|(oy, orow)| {
            let (r0, r1) = (
                &m[oy * 2 * w..(oy * 2 + 1) * w],
                &m[(oy * 2 + 1) * w..(oy * 2 + 2) * w],
            );
            for (ox, px) in orow.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let q = [r0[ox * 2], r0[ox * 2 + 1], r1[ox * 2], r1[ox * 2 + 1]];
                let mut sum = [0f32; 3];
                for (i, v) in q.into_iter().enumerate() {
                    sum[cfa[i >> 1][i & 1] as usize] += v;
                }
                for c in 0..3 {
                    px[c] = sum[c] * inv[c];
                }
            }
        });
    (out, ow, oh)
}

/// Stages 1-3 fused for the fast path: [`normalize_white_balance`] then [`demosaic_half`],
/// read straight from the u16 mosaic. Same result, but it never holds the full-resolution
/// f32 mosaic (240 MB for 61 MP) and touches each sample once.
pub fn normalize_demosaic_half(raw: &RawPhoto) -> (Vec<f32>, usize, usize) {
    let (cx, cy, cw, ch) = raw.crop;
    let sw = raw.sensor_width;
    let (ow, oh) = (cw / 2, ch / 2);
    let g = if raw.wb[1] > 0.0 { raw.wb[1] } else { 1.0 };
    let mul = wb_multipliers(raw.wb, g);
    let black = raw.black;
    let scale: [f32; 4] = std::array::from_fn(|i| 1.0 / (raw.white[i] - raw.black[i]).max(1.0));
    let gain: [f32; 4] = std::array::from_fn(|k| mul[(raw.cfa[k >> 1][k & 1] as usize).min(2)]);
    let mut cnt = [0f32; 3];
    for row in raw.cfa {
        for c in row {
            cnt[c as usize] += 1.0;
        }
    }
    let inv: [f32; 3] = std::array::from_fn(|c| 1.0 / cnt[c].max(1.0));
    // The crop origin is even, so quad position i has CFA site i and black/scale/gain[i].
    let site: [usize; 4] = std::array::from_fn(|i| raw.cfa[i >> 1][i & 1] as usize);
    let mut out = vec![0f32; ow * oh * 3];
    out.par_chunks_mut(ow * 3)
        .enumerate()
        .with_min_len(8)
        .for_each(|(oy, orow)| {
            let y0 = cy + oy * 2;
            let r0 = &raw.mosaic[y0 * sw + cx..y0 * sw + cx + cw];
            let r1 = &raw.mosaic[(y0 + 1) * sw + cx..(y0 + 1) * sw + cx + cw];
            for (ox, px) in orow.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let q = [r0[ox * 2], r0[ox * 2 + 1], r1[ox * 2], r1[ox * 2 + 1]];
                let mut sum = [0f32; 3];
                for (k, s) in q.into_iter().enumerate() {
                    let v = (((s as f32 - black[k]) * scale[k]).clamp(0.0, 1.0) * gain[k]).min(1.0);
                    sum[site[k]] += v;
                }
                for c in 0..3 {
                    px[c] = sum[c] * inv[c];
                }
            }
        });
    (out, ow, oh)
}

/// Box (area) downscale of linear RGB so the long edge is `target`. Returns (rgb, w, h).
/// Separable: per output row, each contributing source row is box-filtered horizontally
/// (precomputed spans) and accumulated with its vertical weight.
pub fn resize_area(src: &[f32], w: usize, h: usize, target: usize) -> (Vec<f32>, usize, usize) {
    let s = target as f64 / w.max(h) as f64;
    let ow = ((w as f64 * s).round() as usize).max(1);
    let oh = ((h as f64 * s).round() as usize).max(1);
    // Horizontal spans: (first source column, weights normalised to sum 1).
    let xspans: Vec<(usize, Vec<f32>)> = (0..ow)
        .map(|ox| {
            let xs = ox as f64 * w as f64 / ow as f64;
            let xe = (ox + 1) as f64 * w as f64 / ow as f64;
            let first = xs.floor() as usize;
            let ws: Vec<f64> = (first..(xe.ceil() as usize).min(w))
                .map(|xx| (xe.min(xx as f64 + 1.0) - xs.max(xx as f64)).max(0.0))
                .collect();
            let sum = ws.iter().sum::<f64>().max(1e-9);
            (first, ws.into_iter().map(|v| (v / sum) as f32).collect())
        })
        .collect();
    let mut out = vec![0f32; ow * oh * 3];
    out.par_chunks_mut(ow * 3).enumerate().for_each_init(
        || vec![0f32; ow * 3],
        |acc, (oy, orow)| {
            let ys = oy as f64 * h as f64 / oh as f64;
            let ye = (oy + 1) as f64 * h as f64 / oh as f64;
            acc.fill(0.0);
            let mut wsum = 0f64;
            for yy in ys.floor() as usize..(ye.ceil() as usize).min(h) {
                let wy = (ye.min(yy as f64 + 1.0) - ys.max(yy as f64)).max(0.0);
                wsum += wy;
                let wy = wy as f32;
                let row = &src[yy * w * 3..(yy + 1) * w * 3];
                for (a, (first, ws)) in acc.as_chunks_mut::<3>().0.iter_mut().zip(&xspans) {
                    let mut t = [0f32; 3];
                    for (i, &wx) in ws.iter().enumerate() {
                        let p = &row[(first + i) * 3..(first + i) * 3 + 3];
                        t[0] += p[0] * wx;
                        t[1] += p[1] * wx;
                        t[2] += p[2] * wx;
                    }
                    a[0] += t[0] * wy;
                    a[1] += t[1] * wy;
                    a[2] += t[2] * wy;
                }
            }
            let inv = (1.0 / wsum.max(1e-9)) as f32;
            for (o, a) in orow.iter_mut().zip(acc.iter()) {
                *o = a * inv;
            }
        },
    );
    (out, ow, oh)
}

const SRGB_TO_XYZ: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192, 0.950_304_1],
];

fn mul33(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut r = [[0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

pub fn invert33(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let m: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| m[i][j] as f64));
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = 1.0 / det;
    let r = [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * d,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * d,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * d,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * d,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * d,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * d,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * d,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * d,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * d,
        ],
    ];
    std::array::from_fn(|i| std::array::from_fn(|j| r[i][j] as f32))
}

/// Build the camera-RGB -> linear sRGB matrix from the camera's XYZ->camera matrix.
///
/// camera = xyz_to_cam * sRGB_to_XYZ * srgb. Rows of that product are normalised to
/// sum to 1 so that a WB-corrected neutral (1,1,1) in camera space maps to (1,1,1)
/// in sRGB (white -> white); then the matrix is inverted.
pub fn camera_to_srgb_matrix(xyz_to_cam: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut rgb_to_cam = mul33(&xyz_to_cam, &SRGB_TO_XYZ);
    for row in rgb_to_cam.iter_mut() {
        let s: f32 = row.iter().sum();
        if s != 0.0 {
            row.iter_mut().for_each(|v| *v /= s);
        }
    }
    invert33(&rgb_to_cam)
}

/// Stage 4: apply a 3x3 matrix to interleaved RGB, clamping negatives to 0.
pub fn apply_matrix(rgb: &mut [f32], m: &[[f32; 3]; 3]) {
    rgb.par_chunks_mut(3 * 4096).for_each(|chunk| {
        for px in chunk.as_chunks_mut::<3>().0.iter_mut() {
            let (r, g, b) = (px[0], px[1], px[2]);
            for i in 0..3 {
                px[i] = (m[i][0] * r + m[i][1] * g + m[i][2] * b).max(0.0);
            }
        }
    });
}

/// IEC 61966-2-1 sRGB encode (linear 0..1 -> encoded 0..1).
pub fn srgb_encode(l: f32) -> f32 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// IEC 61966-2-1 sRGB decode (encoded 0..1 -> linear 0..1).
pub fn srgb_decode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The default Look: global, photo-independent tone and colour parameters applied after
/// the colour matrix. Fit to the Sony A7R II's embedded JPEGs; see [`Look::DEFAULT`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Exposure gain in stops, applied in linear light before the highlight shoulder.
    pub exposure_ev: f32,
    /// Highlight shoulder knee in 0.3..1: linear values (after gain) above this roll off
    /// smoothly so the gained white maps exactly to 1.0. Lower = softer, longer rolloff.
    pub shoulder_knee: f32,
    /// Shadow lift on the sRGB-encoded value, `s += lift * s * (1-s)^2`. 0 = none;
    /// endpoints fixed, monotonic for lift in -0.5..=3.
    pub shadow_lift: f32,
    /// Strength of the smoothstep S-curve on the encoded value: 0 = none, 1 = full smoothstep,
    /// negative = flatter than linear in the mids (-0.5..=1, monotonic throughout).
    pub contrast: f32,
    /// Global saturation on encoded values; 1.0 = unchanged (see [`saturate`]).
    pub saturation: f32,
    /// Tone sliders built into the Look, under any Recipe's own (see [`tone_sliders`]).
    pub tone: ToneSliders,
}

impl Look {
    /// The look used before 2026-10-17: hand-tuned +0.8 EV, 0.3 S-curve, no lift, no
    /// saturation stage. Kept as the baseline for `argus fit-look` before/after reports.
    pub const LEGACY: Look = Look {
        exposure_ev: 0.8,
        shoulder_knee: 0.7,
        shadow_lift: 0.0,
        contrast: 0.3,
        saturation: 1.0,
        tone: ToneSliders::NONE,
    };

    /// The camera's look: fit on 2026-10-17 to the embedded JPEGs of the three Sony A7R II
    /// fixtures (`argus fit-look`, Nelder-Mead on 8x8 block averages in linear light,
    /// objective = mean-channel MAE + 0.5 * mean CIELAB dE76, shared across fixtures).
    /// Full-resolution error vs the camera JPEG, averaged over the fixtures (0..255 scale):
    /// MAE 22.2 -> 16.0, dE76 12.1 -> 9.1, against the legacy look (per photo MAE
    /// 21.5/29.0/16.1 -> 16.9/23.8/7.3). The remaining error is
    /// mostly what a global model cannot express: the camera's sharpening, local contrast
    /// and per-hue colour rendering. Fit is shallow (many parameter sets score within
    /// ~0.3 of the optimum); do not read the individual values as physical.
    pub const CAMERA: Look = Look {
        exposure_ev: 0.220,
        shoulder_knee: 0.802,
        shadow_lift: 1.118,
        contrast: 1.0,
        saturation: 1.238,
        tone: ToneSliders::NONE,
    };

    /// Default look: [`Look::CAMERA`] moved toward Greg's own Lightroom edits (2026-10-18,
    /// `bench/fit_recipe.py` and `just bench --only edit`). His edits are ~5 L* brighter
    /// than the camera JPEG across all 14 bench cases, and his stored develop settings lift
    /// Shadows about +50 and pull Highlights down. +0.15 EV, Shadows +35, Highlights -20:
    /// block dE to his edits 7.5 -> 7.3 on the 3 RAWs in fixtures/raw (dL -3.5 -> +0.4),
    /// and 8.4 -> 7.3 when the same sliders are applied to the camera JPEGs of all 14
    /// cases. Larger lifts help some Photos and hurt others: per-Photo choices belong to
    /// Corrections, not here.
    pub const DEFAULT: Look = Look {
        exposure_ev: 0.370,
        tone: ToneSliders {
            contrast: 0.0,
            highlights: -0.2,
            shadows: 0.35,
            whites: 0.0,
            blacks: 0.0,
        },
        ..Look::CAMERA
    };
}

impl Look {
    /// No tone or colour change: for images that are already rendered (JPEG Photos).
    pub const NEUTRAL: Look = Look {
        exposure_ev: 0.0,
        shoulder_knee: 1.0,
        shadow_lift: 0.0,
        contrast: 0.0,
        saturation: 1.0,
        tone: ToneSliders::NONE,
    };
}

impl Default for Look {
    fn default() -> Self {
        Look::DEFAULT
    }
}

/// Smoothstep S-curve on the sRGB-encoded value: blend between identity and smoothstep.
/// `strength` 0 = identity, 1 = full smoothstep. Endpoints fixed (0->0, 1->1), monotonic
/// for strength in 0..=1.
pub fn base_curve(s: f32, strength: f32) -> f32 {
    let s = s.clamp(0.0, 1.0);
    let sm = s * s * (3.0 - 2.0 * s);
    s * (1.0 - strength) + sm * strength
}

/// Shadow lift on the encoded value: `s + lift * s * (1-s)^2`. Endpoints fixed; the bump
/// peaks near s = 1/3 so highlights are untouched.
pub fn shadow_lift(s: f32, lift: f32) -> f32 {
    let s = s.clamp(0.0, 1.0);
    let t = 1.0 - s;
    (s + lift * s * t * t).clamp(0.0, 1.0)
}

const LUT_SIZE: usize = 16384;

/// Linear gain by `ev` stops with a soft highlight shoulder above `knee` that maps the
/// gained white (2^ev) exactly to 1.0 (monotonic, 0 -> 0). The rolloff width is `1 - knee`.
pub fn expose_with_shoulder(l: f32, ev: f32, knee: f32) -> f32 {
    let w = (1.0 - knee).max(1e-3);
    let xmax = 2f32.powf(ev).max(1.0);
    let x = l.clamp(0.0, 1.0) * 2f32.powf(ev);
    if x <= knee || xmax <= knee {
        return x.min(1.0);
    }
    let norm = 1.0 - (-(xmax - knee) / w).exp();
    knee + (1.0 - knee) * (1.0 - (-(x - knee) / w).exp()) / norm
}

/// Stage 5: exposure gain + shoulder, linear -> sRGB transfer, shadow lift, S-curve, as a
/// per-channel lookup table (encoded 0..1) indexed by linear value * (LUT_SIZE-1).
pub fn tone_lut(look: &Look) -> Vec<f32> {
    tone_lut_with(look, 0.0, &ToneSliders::default())
}

/// Rec. 709 luma weights, applied to sRGB-encoded values.
pub const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Stage 6: global saturation on an encoded RGB triple (0..1). Scales each channel's
/// distance from the pixel's own luma by `amount`: 1 = identity, 0 = greyscale, >1 boosts
/// colour. Greys (r = g = b) are fixed points; luma is preserved before clamping.
pub fn saturate(px: [f32; 3], amount: f32) -> [f32; 3] {
    let y = LUMA[0] * px[0] + LUMA[1] * px[1] + LUMA[2] * px[2];
    px.map(|c| y + (c - y) * amount)
}

/// Tone sliders (Lightroom Basic panel), each -1..1 (slider / 100).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ToneSliders {
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
}

impl ToneSliders {
    pub const NONE: ToneSliders = ToneSliders {
        contrast: 0.0,
        highlights: 0.0,
        shadows: 0.0,
        whites: 0.0,
        blacks: 0.0,
    };
}

/// Peak shift of the highlights and shadows bumps, on the encoded value, at +-100.
/// `s^2(1-s)` and `s(1-s)^2` peak at 4/27, so the curve moves by up to ~0.15 (38 levels).
const BUMP: f32 = 1.0;
/// White and black point travel at +-100 on the encoded value.
const WHITES_TRAVEL: f32 = 0.2;
const BLACKS_TRAVEL: f32 = 0.15;
/// Smoothstep strength at contrast +-100.
const CONTRAST_STRENGTH: f32 = 0.5;

/// Apply the tone sliders to an encoded value. Endpoints stay fixed for contrast,
/// highlights and shadows; whites and blacks move the end points (positive whites clip,
/// negative blacks crush, as in Lightroom). Monotonic for every slider in -1..1.
pub fn tone_sliders(s: f32, t: &ToneSliders) -> f32 {
    let mut s = s.clamp(0.0, 1.0);
    if t.contrast != 0.0 {
        s = base_curve(s, t.contrast.clamp(-1.0, 1.0) * CONTRAST_STRENGTH);
    }
    if t.highlights != 0.0 {
        s += t.highlights.clamp(-1.0, 1.0) * BUMP * s * s * (1.0 - s);
    }
    if t.shadows != 0.0 {
        s += t.shadows.clamp(-1.0, 1.0) * BUMP * s * (1.0 - s) * (1.0 - s);
    }
    if t.whites != 0.0 {
        s += t.whites.clamp(-1.0, 1.0) * WHITES_TRAVEL * s.powi(4);
    }
    if t.blacks != 0.0 {
        s += t.blacks.clamp(-1.0, 1.0) * BLACKS_TRAVEL * (1.0 - s).max(0.0).powi(4);
    }
    s.clamp(0.0, 1.0)
}

/// Stage 5 with a Recipe: like [`tone_lut`] but with `extra_ev` stops of exposure on top of
/// the Look's and the tone sliders applied after the Look's curve.
pub fn tone_lut_with(look: &Look, extra_ev: f32, sliders: &ToneSliders) -> Vec<f32> {
    let st = look.contrast.clamp(-0.5, 1.0);
    let ev = look.exposure_ev + extra_ev;
    (0..LUT_SIZE)
        .map(|i| {
            let l = i as f32 / (LUT_SIZE - 1) as f32;
            let e = srgb_encode(expose_with_shoulder(l, ev, look.shoulder_knee));
            let s = tone_sliders(base_curve(shadow_lift(e, look.shadow_lift), st), &look.tone);
            tone_sliders(s, sliders)
        })
        .collect()
}

/// White balance gains (R, G, B) in linear sRGB for relative `temp` and `tint` in -1..1:
/// +temp warms (R up, B down, 1 stop apart at +1), +tint goes magenta (G down half a stop at +1). Normalised
/// so a grey keeps its luminance.
pub fn wb_gains(temp: f32, tint: f32) -> [f32; 3] {
    let t = temp.clamp(-1.0, 1.0) * 0.5;
    let g = [
        2f32.powf(t),
        2f32.powf(-tint.clamp(-1.0, 1.0) * 0.5),
        2f32.powf(-t),
    ];
    let y = g[0] * 0.2126 + g[1] * 0.7152 + g[2] * 0.0722;
    g.map(|v| v / y)
}

/// Lightroom's eight HSL bands as per-pixel shifts (hue in degrees, sat and lum as -1..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HslBands {
    /// Band centres in degrees, ascending, with (hue_deg, sat, lum) per band.
    pub centres: [f32; 8],
    pub shifts: [[f32; 3]; 8],
}

/// Hue shift at slider +-100, in degrees.
const HSL_HUE_DEG: f32 = 30.0;

impl HslBands {
    pub fn from_recipe(h: &crate::recipe::Hsl) -> Option<Self> {
        if h.is_neutral() {
            return None;
        }
        let b = h.bands();
        Some(HslBands {
            centres: std::array::from_fn(|i| crate::recipe::Hsl::CENTRES[i] as f32),
            shifts: std::array::from_fn(|i| {
                [
                    (b[i].hue / 100.0) as f32 * HSL_HUE_DEG,
                    (b[i].sat / 100.0) as f32,
                    (b[i].lum / 100.0) as f32,
                ]
            }),
        })
    }

    /// Shifts at `hue` (degrees), linearly interpolated between the two nearest band
    /// centres around the colour wheel, so neighbouring bands blend without seams.
    pub fn at(&self, hue: f32) -> [f32; 3] {
        let c = &self.centres;
        let n = c.len();
        let i = c.iter().rposition(|&x| x <= hue).unwrap_or(n - 1);
        let j = (i + 1) % n;
        let span = (c[j] - c[i]).rem_euclid(360.0).max(1e-3);
        let t = ((hue - c[i]).rem_euclid(360.0) / span).clamp(0.0, 1.0);
        std::array::from_fn(|k| self.shifts[i][k] * (1.0 - t) + self.shifts[j][k] * t)
    }

    /// Apply to one encoded RGB pixel. Greys are fixed points.
    pub fn apply(&self, px: [f32; 3]) -> [f32; 3] {
        let (h, s, l) = rgb_to_hsl(px);
        if s < 1e-4 {
            return px;
        }
        let [dh, ds, dl] = self.at(h);
        let h = (h + dh).rem_euclid(360.0);
        let s2 = (s * (1.0 + ds)).clamp(0.0, 1.0);
        // Luminance moves in proportion to how coloured the pixel is (Lightroom-like).
        let l2 = (l * (1.0 + dl * 0.5 * s)).clamp(0.0, 1.0);
        hsl_to_rgb(h, s2, l2)
    }
}

/// RGB (0..1) -> (hue degrees 0..360, saturation 0..1, lightness 0..1).
pub fn rgb_to_hsl(px: [f32; 3]) -> (f32, f32, f32) {
    let [r, g, b] = px.map(|v| v.clamp(0.0, 1.0));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    let d = max - min;
    if d <= 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6);
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s.min(1.0), l)
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c * 0.5;
    [r + m, g + m, b + m]
}

/// Vibrance on an encoded pixel, `v` in -1..1: like saturation but strongest on muted
/// colours and half strength on skin hues (orange, 15..50 degrees) when boosting.
pub fn vibrance(px: [f32; 3], v: f32) -> [f32; 3] {
    let max = px[0].max(px[1]).max(px[2]);
    let min = px[0].min(px[1]).min(px[2]);
    if max <= 1e-6 || max == min {
        return px;
    }
    let sat = (max - min) / max;
    let mut k = v * (1.0 - sat);
    if v > 0.0 {
        let (h, _, _) = rgb_to_hsl(px);
        if (15.0..=50.0).contains(&h) {
            k *= 0.5;
        }
    }
    saturate(px, 1.0 + k)
}

/// Stages 4-7 compiled for one render: camera RGB -> linear sRGB matrix with white
/// balance folded in, tone LUT (Look + exposure + tone sliders), HSL, vibrance and
/// saturation. Build once per render with [`Finish::new`].
pub struct Finish {
    pub matrix: [[f32; 3]; 3],
    pub lut: Vec<f32>,
    pub saturation: f32,
    pub vibrance: f32,
    pub hsl: Option<HslBands>,
    /// Point curves, Point Color, Color Grading and the Calibration shadow tint.
    pub color: Option<crate::color::Color>,
}

impl Finish {
    /// `adjust` is the Recipe's global settings (see [`crate::recipe::Recipe::render_adjust`]).
    pub fn new(matrix: [[f32; 3]; 3], look: &Look, adjust: &crate::recipe::Adjust) -> Self {
        let f = |v: f64| (v / 100.0) as f32;
        let gains = wb_gains(f(adjust.temp), f(adjust.tint));
        let matrix = match crate::color::calibration_matrix(&adjust.calibration) {
            Some(c) => mul33(&c, &matrix),
            None => matrix,
        };
        let matrix = std::array::from_fn(|i| matrix[i].map(|v| v * gains[i]));
        let sliders = ToneSliders {
            contrast: f(adjust.contrast),
            highlights: f(adjust.highlights),
            shadows: f(adjust.shadows),
            whites: f(adjust.whites),
            blacks: f(adjust.blacks),
        };
        let mut lut = tone_lut_with(look, adjust.exposure as f32, &sliders);
        if !adjust.curve.is_neutral() {
            for v in lut.iter_mut() {
                *v = crate::color::parametric(*v, &adjust.curve);
            }
            crate::color::monotone(&mut lut);
        }
        Finish {
            matrix,
            lut,
            saturation: look.saturation * (1.0 + f(adjust.saturation)),
            vibrance: f(adjust.vibrance),
            hsl: HslBands::from_recipe(&adjust.hsl),
            color: crate::color::Color::new(adjust),
        }
    }

    /// The Look alone, no Recipe.
    pub fn plain(matrix: [[f32; 3]; 3], look: &Look) -> Self {
        Self::new(matrix, look, &crate::recipe::Adjust::default())
    }
}

impl Finish {
    /// Stages 4-7 on one camera-RGB pixel: matrix (negatives clamp to 0), tone LUT, HSL,
    /// vibrance, saturation. Returns encoded sRGB, not yet clamped or quantised.
    #[inline]
    pub fn px(&self, px: [f32; 3]) -> [f32; 3] {
        let m = &self.matrix;
        let lut: &[f32; LUT_SIZE] = self
            .lut
            .as_slice()
            .try_into()
            .expect("tone LUT has LUT_SIZE entries");
        let q = |v: f32| {
            lut[((v.clamp(0.0, 1.0) * (LUT_SIZE - 1) as f32 + 0.5) as usize) & (LUT_SIZE - 1)]
        };
        let (r, g, b) = (px[0], px[1], px[2]);
        let mut t = [
            q(m[0][0] * r + m[0][1] * g + m[0][2] * b),
            q(m[1][0] * r + m[1][1] * g + m[1][2] * b),
            q(m[2][0] * r + m[2][1] * g + m[2][2] * b),
        ];
        if let Some(c) = &self.color {
            t = c.curves(t);
        }
        if let Some(h) = &self.hsl {
            t = h.apply(t);
        }
        if self.vibrance != 0.0 {
            t = vibrance(t, self.vibrance);
        }
        if self.saturation != 1.0 {
            t = saturate(t, self.saturation);
        }
        if let Some(c) = &self.color {
            t = c.grade(t);
        }
        t
    }
}

/// Encoded 0..1 to 8-bit.
#[inline]
pub fn to8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Stages 4-7 fused for one row ([`Finish::px`]), quantised to 8-bit. `rgb` and `out`
/// have the same length (multiple of 3).
pub fn color_tone_row(rgb: &[f32], fin: &Finish, out: &mut [u8]) {
    for (px, o) in rgb
        .as_chunks::<3>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<3>().0.iter_mut())
    {
        *o = fin.px(*px).map(to8);
    }
}

/// Stages 4-7 fused over a whole f32 RGB image.
pub fn color_tone(rgb: &[f32], width: usize, fin: &Finish) -> Vec<u8> {
    let mut out = vec![0u8; rgb.len()];
    out.par_chunks_mut(width * 3)
        .zip(rgb.par_chunks(width * 3))
        .with_min_len(8)
        .for_each(|(o, i)| color_tone_row(i, fin, o));
    out
}

/// Stage 8: apply orientation to 8-bit RGB.
pub fn orient(data: Vec<u8>, w: usize, h: usize, o: Orientation) -> RgbImage {
    let (data, w, h) = orient_pixels(data, w, h, o);
    RgbImage {
        width: w,
        height: h,
        data,
    }
}

/// Orientation for any interleaved 3-channel buffer. Returns (data, width, height).
pub fn orient_pixels<T: Copy + Default + Send + Sync>(
    data: Vec<T>,
    w: usize,
    h: usize,
    o: Orientation,
) -> (Vec<T>, usize, usize) {
    match o {
        Orientation::Normal => (data, w, h),
        Orientation::Rotate180 => {
            let mut out = vec![T::default(); data.len()];
            out.par_chunks_mut(w * 3)
                .enumerate()
                .for_each(|(ny, orow)| {
                    let src = &data[(h - 1 - ny) * w * 3..(h - ny) * w * 3];
                    for (d, s) in orow
                        .as_chunks_mut::<3>()
                        .0
                        .iter_mut()
                        .zip(src.as_chunks::<3>().0.iter().rev())
                    {
                        *d = *s;
                    }
                });
            (out, w, h)
        }
        Orientation::Rotate90 | Orientation::Rotate270 => {
            let (nw, nh) = (h, w);
            let mut out = vec![T::default(); data.len()];
            // clockwise 90: (x,y) -> (h-1-y, x); 270: (x,y) -> (y, w-1-x). Gather per output row.
            out.par_chunks_mut(nw * 3)
                .enumerate()
                .for_each(|(ny, orow)| {
                    for (nx, d) in orow.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                        let (x, y) = if o == Orientation::Rotate90 {
                            (ny, h - 1 - nx)
                        } else {
                            (w - 1 - ny, nx)
                        };
                        d.copy_from_slice(&data[(y * w + x) * 3..(y * w + x) * 3 + 3]);
                    }
                });
            (out, nw, nh)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wb_scaling_normalises_to_green() {
        let m = wb_multipliers([2.0, 1.0, 1.5], 1.0);
        assert_eq!(m, [2.0, 1.0, 1.5]);
        let mut mosaic = vec![0.25, 0.5, 0.5, 0.4]; // RGGB 2x2
        white_balance(&mut mosaic, 2, [[0, 1], [1, 2]], [2.0, 1.0, 1.5]);
        assert!((mosaic[0] - 0.5).abs() < 1e-6);
        assert!((mosaic[1] - 0.5).abs() < 1e-6);
        assert!((mosaic[3] - 0.6).abs() < 1e-6);
        // clipping
        let mut hi = vec![0.9, 0.9, 0.9, 0.9];
        white_balance(&mut hi, 2, [[0, 1], [1, 2]], [2.0, 1.0, 1.5]);
        assert_eq!(hi[0], 1.0);
    }

    #[test]
    fn matrix_maps_white_to_white() {
        // Arbitrary but plausible xyz->cam matrix.
        let x2c = [[0.9, -0.2, -0.1], [-0.4, 1.3, 0.1], [0.0, 0.2, 0.7]];
        let m = camera_to_srgb_matrix(x2c);
        for row in m {
            let s: f32 = row.iter().sum();
            assert!((s - 1.0).abs() < 1e-4, "row sum {s}");
        }
        let mut px = [1.0f32, 1.0, 1.0];
        apply_matrix(&mut px, &m);
        for v in px {
            assert!((v - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn transfer_curve_endpoints_and_monotonic() {
        for look in [
            Look {
                exposure_ev: 0.0,
                ..Look::DEFAULT
            },
            Look::DEFAULT,
            Look::LEGACY,
        ] {
            let lut = tone_lut(&look);
            assert_eq!(lut[0], 0.0);
            assert!((lut.last().unwrap() - 1.0).abs() < 1e-6);
            assert!(lut.windows(2).all(|w| w[0] <= w[1] + 1e-7));
        }
        assert_eq!(srgb_encode(0.0), 0.0);
        assert!((srgb_encode(1.0) - 1.0).abs() < 1e-6);
        assert!((base_curve(0.5, 1.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn demosaic_flat_field_is_flat() {
        let m = vec![0.5f32; 16];
        let rgb = demosaic_bilinear(&m, 4, 4, [[0, 1], [1, 2]]);
        assert!(rgb.iter().all(|v| (v - 0.5).abs() < 1e-6));
        let (h, w, hh) = demosaic_half(&m, 4, 4, [[0, 1], [1, 2]]);
        assert_eq!((w, hh), (2, 2));
        assert!(h.iter().all(|v| (v - 0.5).abs() < 1e-6));
    }

    #[test]
    fn orient_rotate90_swaps_dims() {
        let img = orient(vec![1, 1, 1, 2, 2, 2], 2, 1, Orientation::Rotate90);
        assert_eq!((img.width, img.height), (1, 2));
        assert_eq!(img.data, vec![1, 1, 1, 2, 2, 2]);
    }

    fn pseudo(n: usize, seed: u32) -> Vec<f32> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 8) as f32 / (1u32 << 24) as f32
            })
            .collect()
    }

    #[test]
    fn fast_demosaic_matches_generic_rule() {
        let (w, h) = (9, 7);
        let m = pseudo(w * h, 7);
        for cfa in [
            [[0u8, 1], [1, 2]],
            [[1, 0], [2, 1]],
            [[1, 2], [0, 1]],
            [[2, 1], [1, 0]],
        ] {
            let got = demosaic_bilinear(&m, w, h, cfa);
            for y in 0..h {
                for x in 0..w {
                    let want = demosaic_pixel_generic(&m, w, h, cfa, x, y);
                    for c in 0..3 {
                        assert!(
                            (got[(y * w + x) * 3 + c] - want[c]).abs() < 1e-5,
                            "{cfa:?} ({x},{y}) c{c}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fused_normalize_wb_matches_separate_stages() {
        let (w, h) = (8usize, 6usize);
        let mosaic: Vec<u16> = pseudo(w * h, 3)
            .iter()
            .map(|v| (v * 4000.0) as u16 + 100)
            .collect();
        let raw = RawPhoto {
            make: String::new(),
            model: String::new(),
            sensor_width: w,
            sensor_height: h,
            crop: (2, 2, 6, 4),
            mosaic,
            cfa: [[0, 1], [1, 2]],
            black: [512.0, 500.0, 504.0, 520.0],
            white: [3800.0, 3900.0, 3850.0, 3700.0],
            wb: [2.1, 1.0, 1.6],
            xyz_to_cam: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            orientation: Orientation::Normal,
        };
        let mut want = normalize(&raw);
        white_balance(&mut want, 6, raw.cfa, raw.wb);
        assert_eq!(normalize_white_balance(&raw), want);
    }

    #[test]
    fn fused_half_demosaic_matches_separate_stages() {
        let (sw, sh) = (36, 22);
        let mosaic: Vec<u16> = (0..sw * sh)
            .map(|i| ((i as u32 * 2654435761) >> 18) as u16)
            .collect();
        for cfa in [[[0u8, 1], [1, 2]], [[1, 2], [0, 1]]] {
            let raw = RawPhoto {
                make: String::new(),
                model: String::new(),
                sensor_width: sw,
                sensor_height: sh,
                crop: (4, 2, 28, 18),
                mosaic: mosaic.clone(),
                cfa,
                black: [512.0, 500.0, 520.0, 510.0],
                white: [16000.0; 4],
                wb: [2.1, 1.0, 1.6],
                xyz_to_cam: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                orientation: Orientation::Normal,
            };
            let (cw, ch) = raw.crop_size();
            let want = demosaic_half(&normalize_white_balance(&raw), cw, ch, cfa);
            assert_eq!(normalize_demosaic_half(&raw), want);
        }
    }

    #[test]
    fn resize_area_preserves_flat_field_and_size() {
        let src = vec![0.4f32; 10 * 6 * 3];
        let (out, w, h) = resize_area(&src, 10, 6, 5);
        assert_eq!((w, h), (5, 3));
        assert!(out.iter().all(|v| (v - 0.4).abs() < 1e-6));
    }

    #[test]
    fn saturate_identity_at_one() {
        for px in [[0.2, 0.5, 0.9], [0.0, 1.0, 0.3], [0.7, 0.7, 0.1]] {
            let out = saturate(px, 1.0);
            for c in 0..3 {
                assert!((out[c] - px[c]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn saturate_keeps_grey_grey_and_zero_is_luma() {
        for amount in [0.0, 0.5, 1.0, 1.4] {
            let out = saturate([0.4, 0.4, 0.4], amount);
            assert!(out.iter().all(|v| (v - 0.4).abs() < 1e-6));
        }
        let g = saturate([0.9, 0.2, 0.4], 0.0);
        assert!((g[0] - g[1]).abs() < 1e-6 && (g[1] - g[2]).abs() < 1e-6);
        let y = LUMA[0] * 0.9 + LUMA[1] * 0.2 + LUMA[2] * 0.4;
        assert!((g[0] - y).abs() < 1e-6);
        let b = saturate([0.9, 0.2, 0.4], 1.3);
        assert!(b[0] - b[1] > 0.9 - 0.2);
    }

    #[test]
    fn shadow_lift_fixes_endpoints_and_raises_shadows() {
        assert_eq!(shadow_lift(0.0, 1.0), 0.0);
        assert!((shadow_lift(1.0, 1.0) - 1.0).abs() < 1e-6);
        assert_eq!(shadow_lift(0.3, 0.0), 0.3);
        assert!(shadow_lift(0.2, 0.5) > 0.2);
        let mut prev = 0.0;
        for i in 0..=100 {
            let v = shadow_lift(i as f32 / 100.0, 2.0);
            assert!(v >= prev);
            prev = v;
        }
    }

    #[test]
    fn shoulder_maps_gained_white_to_one() {
        for (ev, knee) in [(0.8, 0.7), (1.2, 0.5), (0.3, 0.9)] {
            assert!((expose_with_shoulder(1.0, ev, knee) - 1.0).abs() < 1e-5);
            assert_eq!(expose_with_shoulder(0.0, ev, knee), 0.0);
        }
    }

    fn sliders(c: f32, h: f32, sh: f32, w: f32, b: f32) -> ToneSliders {
        ToneSliders {
            contrast: c,
            highlights: h,
            shadows: sh,
            whites: w,
            blacks: b,
        }
    }

    #[test]
    fn tone_sliders_neutral_is_identity_and_all_are_monotonic() {
        for i in 0..=100 {
            let s = i as f32 / 100.0;
            assert_eq!(tone_sliders(s, &ToneSliders::default()), s);
        }
        for v in [-1.0, -0.5, 0.5, 1.0] {
            for t in [
                sliders(v, 0., 0., 0., 0.),
                sliders(0., v, 0., 0., 0.),
                sliders(0., 0., v, 0., 0.),
                sliders(0., 0., 0., v, 0.),
                sliders(0., 0., 0., 0., v),
                sliders(v, v, -v, v, -v),
            ] {
                let mut prev = 0.0;
                for i in 0..=1000 {
                    let o = tone_sliders(i as f32 / 1000.0, &t);
                    assert!(o >= prev - 1e-6, "{t:?} at {i}");
                    prev = o;
                }
            }
        }
    }

    #[test]
    fn tone_sliders_move_the_right_part_of_the_range() {
        let lo = 0.2;
        let hi = 0.8;
        let sh = sliders(0., 0., 1., 0., 0.);
        assert!(tone_sliders(lo, &sh) - lo > tone_sliders(hi, &sh) - hi);
        let hl = sliders(0., -1., 0., 0., 0.);
        assert!(hi - tone_sliders(hi, &hl) > lo - tone_sliders(lo, &hl));
        assert!(tone_sliders(0.0, &sliders(0., 0., 0., 0., 1.)) > 0.1);
        assert_eq!(tone_sliders(1.0, &sliders(0., 0., 0., 1., 0.)), 1.0);
        assert!(tone_sliders(1.0, &sliders(0., 0., 0., -1., 0.)) < 0.85);
        let c = sliders(1., 0., 0., 0., 0.);
        assert!(tone_sliders(0.25, &c) < 0.25 && tone_sliders(0.75, &c) > 0.75);
    }

    #[test]
    fn wb_gains_keep_grey_luminance_and_warm_with_temp() {
        assert_eq!(wb_gains(0.0, 0.0), [1.0, 1.0, 1.0]);
        for (t, n) in [(1.0, 0.0), (-0.4, 0.3), (0.2, -1.0)] {
            let g = wb_gains(t, n);
            let y = g[0] * LUMA[0] + g[1] * LUMA[1] + g[2] * LUMA[2];
            assert!((y - 1.0).abs() < 1e-5);
        }
        let w = wb_gains(0.5, 0.0);
        assert!(w[0] > w[2]);
        let m = wb_gains(0.0, 0.5);
        assert!(m[1] < m[0] && (m[0] - m[2]).abs() < 1e-6);
    }

    #[test]
    fn hsl_round_trip_and_band_blending() {
        for px in [
            [0.9, 0.2, 0.1],
            [0.1, 0.6, 0.8],
            [0.5, 0.5, 0.5],
            [0.3, 0.1, 0.7],
        ] {
            let (h, s, l) = rgb_to_hsl(px);
            let back = hsl_to_rgb(h, s, l);
            for c in 0..3 {
                assert!((back[c] - px[c]).abs() < 1e-5, "{px:?} -> {back:?}");
            }
        }
        let mut hsl = crate::recipe::Hsl::default();
        hsl.blue.sat = -100.0;
        let b = HslBands::from_recipe(&hsl).unwrap();
        assert_eq!(b.at(240.0)[1], -1.0);
        assert_eq!(b.at(120.0)[1], 0.0);
        assert!((b.at(180.0 + 30.0)[1] - -0.5).abs() < 1e-6);
        // Blue desaturates to grey, red and grey are untouched.
        let out = b.apply([0.1, 0.1, 0.9]);
        assert!((out[0] - out[2]).abs() < 1e-5);
        let red = b.apply([0.9, 0.1, 0.1]);
        assert!((red[0] - 0.9).abs() < 1e-5);
        assert_eq!(b.apply([0.4, 0.4, 0.4]), [0.4, 0.4, 0.4]);
        assert!(HslBands::from_recipe(&crate::recipe::Hsl::default()).is_none());
    }

    #[test]
    fn vibrance_boosts_muted_more_than_vivid() {
        let muted = [0.5, 0.45, 0.4];
        let vivid = [0.9, 0.1, 0.5];
        let gain = |p: [f32; 3], o: [f32; 3]| (o[0] - o[1]) / (p[0] - p[1]);
        assert!(gain(muted, vibrance(muted, 1.0)) > gain(vivid, vibrance(vivid, 1.0)));
        assert_eq!(vibrance([0.3, 0.3, 0.3], 1.0), [0.3, 0.3, 0.3]);
    }

    #[test]
    fn neutral_look_lut_is_identity() {
        let lut = tone_lut(&Look::NEUTRAL);
        for i in (0..LUT_SIZE).step_by(97) {
            let l = i as f32 / (LUT_SIZE - 1) as f32;
            assert!((srgb_decode(lut[i]) - l).abs() < 1e-4);
        }
    }
}
