//! Lightroom's colour panels as per-pixel stages: the Tone Curve (parametric regions and
//! point curves), Calibration, Point Color and Color Grading. Each is written from the
//! panel's public description, not from Adobe's code.
//!
//! All but Calibration work on sRGB-encoded values (0..1) after the tone LUT, inside
//! [`crate::pipeline::Finish::px`]; Calibration is a 3x3 matrix folded into the colour
//! matrix, so it costs nothing per pixel.

use crate::pipeline::{hsl_to_rgb, rgb_to_hsl, LUMA};
use crate::recipe::{Adjust, Calibration, Curve, Grading, PointColor, Points};

/// Entries in each curve lookup table.
const CURVE_LUT: usize = 1024;
/// How far a parametric region moves the curve at +-100, on the encoded value.
const REGION_TRAVEL: f32 = 0.12;

/// The parametric Tone Curve as a function on the encoded value: four regions (shadows,
/// darks, lights, highlights) between the splits, each a smooth bump peaking at its region's
/// middle and fading out at its neighbours' middles, so the ends stay put.
pub fn parametric(s: f32, c: &Curve) -> f32 {
    let amounts = [c.shadows, c.darks, c.lights, c.highlights].map(|v| (v / 100.0) as f32);
    if amounts.iter().all(|&a| a == 0.0) {
        return s;
    }
    let [s1, s2, s3] = c.splits().map(|v| v as f32);
    let mid = [s1 * 0.5, (s1 + s2) * 0.5, (s2 + s3) * 0.5, (s3 + 1.0) * 0.5];
    let mut out = s;
    for (k, &a) in amounts.iter().enumerate() {
        if a == 0.0 {
            continue;
        }
        let lo = if k == 0 { 0.0 } else { mid[k - 1] };
        let hi = if k == 3 { 1.0 } else { mid[k + 1] };
        let w = if s <= mid[k] {
            bell((s - lo) / (mid[k] - lo).max(1e-4))
        } else {
            bell((hi - s) / (hi - mid[k]).max(1e-4))
        };
        out += a * REGION_TRAVEL * w;
    }
    out.clamp(0.0, 1.0)
}

/// 0 at t <= 0, 1 at t >= 1, a raised cosine between.
fn bell(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    0.5 - 0.5 * (t * std::f32::consts::PI).cos()
}

/// Make a lookup table non-decreasing, so no curve setting can invert tones.
pub fn monotone(lut: &mut [f32]) {
    for i in 1..lut.len() {
        lut[i] = lut[i].max(lut[i - 1]);
    }
}

/// A point curve (Lightroom's 0..255 points) as a lookup table on 0..1, through a monotone
/// cubic (Fritsch and Carlson 1980) so it never overshoots between points. A curve without
/// a point at either end keeps that end where it is. None when it is straight.
pub fn point_lut(points: &Points) -> Option<Vec<f32>> {
    let mut p: Vec<(f32, f32)> = points
        .iter()
        .map(|q| ((q[0] / 255.0) as f32, (q[1] / 255.0) as f32))
        .collect();
    p.sort_by(|a, b| a.0.total_cmp(&b.0));
    p.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-3);
    if p.first().is_none_or(|q| q.0 > 0.0) {
        p.insert(0, (0.0, 0.0));
    }
    if p.last().is_none_or(|q| q.0 < 1.0) {
        p.push((1.0, 1.0));
    }
    if p.iter().all(|q| (q.0 - q.1).abs() < 1e-4) {
        return None;
    }
    let n = p.len();
    let d: Vec<f32> = (0..n - 1)
        .map(|i| (p[i + 1].1 - p[i].1) / (p[i + 1].0 - p[i].0))
        .collect();
    let mut m = vec![0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 {
            0.0
        } else {
            (d[i - 1] + d[i]) * 0.5
        };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
        let r = a * a + b * b;
        if r > 9.0 {
            let t = 3.0 / r.sqrt();
            m[i] = t * a * d[i];
            m[i + 1] = t * b * d[i];
        }
    }
    let lut = (0..CURVE_LUT)
        .map(|j| {
            let x = j as f32 / (CURVE_LUT - 1) as f32;
            let i = p.iter().rposition(|q| q.0 <= x).unwrap_or(0).min(n - 2);
            let h = p[i + 1].0 - p[i].0;
            let t = ((x - p[i].0) / h).clamp(0.0, 1.0);
            let (t2, t3) = (t * t, t * t * t);
            let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p[i].1
                + (t3 - 2.0 * t2 + t) * h * m[i]
                + (-2.0 * t3 + 3.0 * t2) * p[i + 1].1
                + (t3 - t2) * h * m[i + 1];
            y.clamp(0.0, 1.0)
        })
        .collect();
    Some(lut)
}

fn look_up(lut: &[f32], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * (lut.len() - 1) as f32;
    let i = (x as usize).min(lut.len() - 2);
    let t = x - i as f32;
    lut[i] * (1.0 - t) + lut[i + 1] * t
}

/// Calibration as a matrix on linear sRGB, applied after the camera matrix: each primary's
/// column is turned around the hue wheel (up to 1/8 of the way to its neighbour) and its
/// colour scaled away from or toward grey, then rows are normalised so white stays white.
pub fn calibration_matrix(c: &Calibration) -> Option<[[f32; 3]; 3]> {
    if c.red == Default::default() && c.green == Default::default() && c.blue == Default::default()
    {
        return None;
    }
    let mut cols = [[0f32; 3]; 3];
    for (j, p) in [c.red, c.green, c.blue].iter().enumerate() {
        let base: f32 = [0.0, 120.0, 240.0][j];
        let hue = base + (p.hue / 100.0) as f32 * 40.0;
        // Keep the primary's luminance, then push its colour by `sat`.
        let mut v = hsl_to_rgb(hue, 1.0, 0.5);
        let y_target = LUMA[j];
        let y = LUMA[0] * v[0] + LUMA[1] * v[1] + LUMA[2] * v[2];
        v = v.map(|x| x * y_target / y.max(1e-6));
        let k = 1.0 + (p.sat / 100.0) as f32 * 0.5;
        v = v.map(|x| y_target + (x - y_target) * k);
        cols[j] = v;
    }
    let mut m: [[f32; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| cols[j][i]));
    for row in m.iter_mut() {
        let s: f32 = row.iter().sum();
        if s.abs() > 1e-6 {
            *row = row.map(|v| v / s);
        }
    }
    Some(m)
}

/// The colour stages of one render, compiled from a Recipe's global settings.
pub struct Color {
    rgb: Option<Vec<f32>>,
    channel: [Option<Vec<f32>>; 3],
    refine_sat: f32,
    shadow_tint: f32,
    points: Vec<PointColor>,
    grading: Option<GradingOp>,
}

impl Color {
    /// None when none of these panels changes anything.
    pub fn new(a: &Adjust) -> Option<Self> {
        let c = &a.curve;
        let col = Color {
            rgb: point_lut(&c.rgb),
            channel: [point_lut(&c.red), point_lut(&c.green), point_lut(&c.blue)],
            refine_sat: (c.refine_sat.unwrap_or(100.0) / 100.0) as f32,
            shadow_tint: (a.calibration.shadows_tint / 100.0) as f32,
            points: a
                .point_color
                .iter()
                .filter(|p| p.hue_shift != 0.0 || p.sat_shift != 0.0 || p.lum_shift != 0.0)
                .copied()
                .collect(),
            grading: (!a.grading.is_identity()).then(|| GradingOp::new(&a.grading)),
        };
        let curves = col.rgb.is_some() || col.channel.iter().any(Option::is_some);
        (curves || col.shadow_tint != 0.0 || !col.points.is_empty() || col.grading.is_some())
            .then_some(col)
    }

    /// Point curves (with Refine Saturation) and the Calibration shadow tint: before HSL.
    #[inline]
    pub fn curves(&self, px: [f32; 3]) -> [f32; 3] {
        let mut t = px;
        if self.rgb.is_some() || self.channel.iter().any(Option::is_some) {
            let before = t;
            for (c, v) in t.iter_mut().enumerate() {
                if let Some(l) = &self.rgb {
                    *v = look_up(l, *v);
                }
                if let Some(l) = &self.channel[c] {
                    *v = look_up(l, *v);
                }
            }
            if self.refine_sat < 1.0 {
                t = refine(before, t, self.refine_sat);
            }
        }
        if self.shadow_tint != 0.0 {
            let y = LUMA[0] * t[0] + LUMA[1] * t[1] + LUMA[2] * t[2];
            let w = (1.0 - y).clamp(0.0, 1.0).powi(2) * 0.08 * self.shadow_tint;
            t = [t[0] + w * 0.5, t[1] - w, t[2] + w * 0.5];
        }
        t
    }

    /// Point Color and Color Grading: after HSL.
    #[inline]
    pub fn grade(&self, px: [f32; 3]) -> [f32; 3] {
        let mut t = px;
        for p in &self.points {
            t = point_color(t, p);
        }
        if let Some(g) = &self.grading {
            t = g.apply(t);
        }
        t
    }
}

/// Keep the curves' change in brightness, and of their change in colourfulness only `keep`.
fn refine(before: [f32; 3], after: [f32; 3], keep: f32) -> [f32; 3] {
    let y0 = LUMA[0] * before[0] + LUMA[1] * before[1] + LUMA[2] * before[2];
    let y1 = LUMA[0] * after[0] + LUMA[1] * after[1] + LUMA[2] * after[2];
    // Colour of the "before" pixel carried to the new brightness.
    let carried = before.map(|v| y1 + (v - y0));
    std::array::from_fn(|i| carried[i] + (after[i] - carried[i]) * keep)
}

fn point_color(px: [f32; 3], p: &PointColor) -> [f32; 3] {
    let (h, s, l) = rgb_to_hsl(px);
    if s < 1e-3 {
        return px;
    }
    let range = (p.range.unwrap_or(50.0) / 100.0) as f32;
    let hue_width = 8.0 + range * 52.0; // degrees either side
    let dh = ((h - p.hue as f32 + 540.0).rem_euclid(360.0) - 180.0).abs();
    let ds = (s - (p.sat / 100.0) as f32).abs();
    let dl = (l - (p.lum / 100.0) as f32).abs();
    let w = (1.0 - dh / hue_width).max(0.0)
        * (1.0 - ds / (0.25 + range * 0.6)).max(0.0)
        * (1.0 - dl / (0.25 + range * 0.6)).max(0.0);
    if w <= 0.0 {
        return px;
    }
    let w = w * w * (3.0 - 2.0 * w);
    let h2 = h + w * (p.hue_shift / 100.0) as f32 * 30.0;
    let s2 = (s * (1.0 + w * (p.sat_shift / 100.0) as f32)).clamp(0.0, 1.0);
    let l2 = (l * (1.0 + w * (p.lum_shift / 100.0) as f32 * 0.5)).clamp(0.0, 1.0);
    hsl_to_rgb(h2, s2, l2)
}

/// Color Grading compiled: an RGB tint per wheel (luma-free, so it only colours) and a
/// lightness shift, weighted by where each pixel sits between shadows and highlights.
struct GradingOp {
    tints: [[f32; 3]; 4],
    lums: [f32; 4],
    pivot: f32,
    width: f32,
}

/// Tint strength at saturation 100, on the encoded value.
const GRADE_TINT: f32 = 0.25;
/// Lightness shift at luminance +-100.
const GRADE_LUM: f32 = 0.15;

impl GradingOp {
    fn new(g: &Grading) -> Self {
        let wheels = [g.shadows, g.midtones, g.highlights, g.global];
        GradingOp {
            tints: wheels.map(|w| {
                let c = hsl_to_rgb(w.hue as f32, 1.0, 0.5);
                let y = LUMA[0] * c[0] + LUMA[1] * c[1] + LUMA[2] * c[2];
                c.map(|v| (v - y) * GRADE_TINT * (w.sat / 100.0) as f32)
            }),
            lums: wheels.map(|w| (w.lum / 100.0) as f32 * GRADE_LUM),
            pivot: 0.5 - (g.balance / 100.0) as f32 * 0.3,
            width: 0.15 + (g.blending.unwrap_or(50.0) / 100.0) as f32 * 0.35,
        }
    }

    fn apply(&self, px: [f32; 3]) -> [f32; 3] {
        let y = (LUMA[0] * px[0] + LUMA[1] * px[1] + LUMA[2] * px[2]).clamp(0.0, 1.0);
        let shadows = 1.0 - smooth(self.pivot - self.width, self.pivot, y);
        let highlights = smooth(self.pivot, self.pivot + self.width, y);
        let mids = (1.0 - shadows - highlights).max(0.0);
        let w = [shadows, mids, highlights, 1.0];
        let mut t = px;
        // Tints fade toward black and white, so pure ends stay pure.
        let room = 4.0 * y * (1.0 - y);
        for ((&wk, tint), &lum) in w.iter().zip(&self.tints).zip(&self.lums) {
            if wk <= 0.0 {
                continue;
            }
            for (v, &tc) in t.iter_mut().zip(tint) {
                *v += wk * (tc * room.max(0.15) + lum * room);
            }
        }
        t
    }
}

/// Smoothstep from `a` to `b`, safe when they meet.
pub(crate) fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a).max(1e-4)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Primary, Wheel};

    #[test]
    fn parametric_moves_its_own_region_and_keeps_the_ends() {
        let c = Curve {
            lights: 100.0,
            ..Default::default()
        };
        assert_eq!(parametric(0.0, &c), 0.0);
        assert_eq!(parametric(1.0, &c), 1.0);
        let lift = |s: f32| parametric(s, &c) - s;
        assert!(lift(0.62) > lift(0.2));
        assert!(lift(0.62) > lift(0.95));
        assert!(lift(0.62) > 0.1);
    }

    #[test]
    fn point_curve_goes_through_its_points_without_overshoot() {
        let lut = point_lut(&vec![
            [0.0, 0.0],
            [64.0, 40.0],
            [192.0, 220.0],
            [255.0, 255.0],
        ])
        .unwrap();
        assert!((look_up(&lut, 64.0 / 255.0) - 40.0 / 255.0).abs() < 0.01);
        assert!((look_up(&lut, 192.0 / 255.0) - 220.0 / 255.0).abs() < 0.01);
        assert!(lut.windows(2).all(|w| w[1] >= w[0] - 1e-6));
        assert!(point_lut(&vec![[0.0, 0.0], [255.0, 255.0]]).is_none());
        assert!(point_lut(&vec![]).is_none());
    }

    #[test]
    fn calibration_keeps_white_and_moves_its_primary() {
        let c = Calibration {
            red: Primary {
                hue: 100.0,
                sat: 0.0,
            },
            ..Default::default()
        };
        let m = calibration_matrix(&c).unwrap();
        let white: [f32; 3] = std::array::from_fn(|i| m[i].iter().sum());
        assert!(white.iter().all(|v| (v - 1.0).abs() < 1e-5));
        // Pure red turns toward orange: more green than blue.
        let red = [m[0][0], m[1][0], m[2][0]];
        assert!(red[1] > red[2]);
    }

    #[test]
    fn grading_tints_shadows_not_highlights_and_greys_shift_by_hue() {
        let mut a = Adjust::default();
        a.grading.shadows = Wheel {
            hue: 220.0,
            sat: 80.0,
            lum: 0.0,
        };
        let col = Color::new(&a).unwrap();
        let dark = col.grade([0.2, 0.2, 0.2]);
        let light = col.grade([0.9, 0.9, 0.9]);
        assert!(dark[2] > dark[0] + 0.02, "shadows turn blue: {dark:?}");
        assert!(
            (light[2] - light[0]).abs() < 0.01,
            "highlights stay neutral: {light:?}"
        );
    }

    #[test]
    fn point_color_only_touches_nearby_colours() {
        let p = PointColor {
            hue: 30.0,
            sat: 60.0,
            lum: 50.0,
            sat_shift: -100.0,
            ..Default::default()
        };
        let orange = hsl_to_rgb(30.0, 0.6, 0.5);
        let blue = hsl_to_rgb(220.0, 0.6, 0.5);
        let (_, s, _) = rgb_to_hsl(point_color(orange, &p));
        assert!(s < 0.1);
        assert_eq!(point_color(blue, &p), blue);
    }

    #[test]
    fn refine_sat_zero_keeps_the_old_colour_at_the_new_brightness() {
        let before = [0.6, 0.4, 0.3];
        let after = [0.8, 0.5, 0.3];
        let r = refine(before, after, 0.0);
        let y = |p: [f32; 3]| LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2];
        assert!((y(r) - y(after)).abs() < 1e-5);
        assert!(((r[0] - r[2]) - (before[0] - before[2])).abs() < 1e-5);
    }
}
