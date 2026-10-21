//! Spot healing and cloning, Lightroom's Heal and Clone: each Spot covers a small circle
//! with pixels copied from another place in the same Photo. Clone copies them as they are;
//! Heal keeps their texture and takes its colour and light from the circle's own edge, so the
//! patch disappears into its surroundings. Nothing is generated (spec: no generative fill).
//!
//! Spots live in the Recipe ([`Spot`]), on the upright, uncropped frame like Masks, and are
//! applied after colour, tone and Masks, before the crop ([`crate::render::render_photo`]),
//! in order, so a later Spot can copy from an earlier one's result. [`find_source`] picks a
//! source when the person or the Agent doesn't give one, as Lightroom does.

use crate::RgbImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpotMode {
    /// Texture from the source, colour and light matched to the spot's surroundings.
    #[default]
    Heal,
    /// The source's pixels as they are.
    Clone,
}

/// A point as 0..1 of the upright image's width and height.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// One Heal or Clone circle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub id: String,
    #[serde(default)]
    pub mode: SpotMode,
    /// Centre of the spot, 0..1 of the upright image's width and height.
    pub x: f64,
    pub y: f64,
    /// Radius as a fraction of the image's long edge (as a brush dot's).
    pub r: f64,
    /// Centre of the circle the pixels come from.
    pub from: Point,
    /// Soft edge, 0..100: the outer share of the radius that fades out.
    #[serde(default = "default_feather")]
    pub feather: f64,
    /// 0..100.
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    /// Who made it: "ai" or "you", as on a local adjustment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub const DEFAULT_FEATHER: f64 = 50.0;

fn default_feather() -> f64 {
    DEFAULT_FEATHER
}

fn default_opacity() -> f64 {
    100.0
}

/// Largest radius a Spot may have, as a fraction of the long edge. Lightroom's brush tops
/// out about here; anything bigger is object removal, which Argus leaves out.
pub const MAX_R: f64 = 0.15;
pub const MIN_R: f64 = 0.002;

impl Spot {
    /// Clamp into range: centres inside the frame, radius, feather and opacity in bounds.
    pub fn clamped(mut self) -> Spot {
        let f = |v: f64, d: f64, lo: f64, hi: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.x = f(self.x, 0.5, 0.0, 1.0);
        self.y = f(self.y, 0.5, 0.0, 1.0);
        self.from.x = f(self.from.x, self.x, 0.0, 1.0);
        self.from.y = f(self.from.y, self.y, 0.0, 1.0);
        self.r = f(self.r, 0.02, MIN_R, MAX_R);
        self.feather = f(self.feather, DEFAULT_FEATHER, 0.0, 100.0);
        self.opacity = f(self.opacity, 100.0, 0.0, 100.0);
        self
    }
}

/// Bilinear sample with the coordinates clamped to the image, 0..255 per channel.
fn sample(img: &RgbImage, x: f32, y: f32) -> [f32; 3] {
    let (w, h) = (img.width, img.height);
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |x: usize, y: usize, c: usize| img.data[(y * w + x) * 3 + c] as f32;
    std::array::from_fn(|c| {
        let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
        let bot = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
        top * (1.0 - fy) + bot * fy
    })
}

/// Mean of a small cross of samples around (x, y), `d` pixels apart: an edge value that
/// isn't thrown by one noisy pixel.
fn local_mean(img: &RgbImage, x: f32, y: f32, d: f32) -> [f32; 3] {
    let mut s = [0f32; 3];
    for (ox, oy) in [(0.0, 0.0), (d, 0.0), (-d, 0.0), (0.0, d), (0.0, -d)] {
        let p = sample(img, x + ox, y + oy);
        for c in 0..3 {
            s[c] += p[c] / 5.0;
        }
    }
    s
}

/// Apply `spots` in order to an upright, uncropped render.
pub fn apply(img: &mut RgbImage, spots: &[Spot]) {
    if img.width < 2 || img.height < 2 {
        return;
    }
    for s in spots {
        apply_one(img, &s.clone().clamped());
    }
}

fn apply_one(img: &mut RgbImage, s: &Spot) {
    let (w, h) = (img.width, img.height);
    let long = w.max(h) as f32;
    let rad = (s.r as f32 * long).max(1.0);
    let (cx, cy) = (s.x as f32 * w as f32, s.y as f32 * h as f32);
    let (dx, dy) = (
        (s.from.x - s.x) as f32 * w as f32,
        (s.from.y - s.y) as f32 * h as f32,
    );
    let opacity = (s.opacity / 100.0) as f32;
    if opacity <= 0.0 || (dx == 0.0 && dy == 0.0) {
        return;
    }
    let inner = 1.0 - (s.feather / 100.0) as f32;

    // Heal: the difference between the spot's edge and the source's edge, around the circle,
    // spread inward as the smooth (harmonic) surface those edge values pin down. On a disc
    // that surface is the Poisson integral of the edge, so no solver is needed.
    let ring: Vec<([f32; 2], [f32; 3])> = if s.mode == SpotMode::Heal {
        let n = ((std::f32::consts::TAU * rad / 2.0) as usize).clamp(16, 128);
        let d = (rad * 0.08).max(1.0);
        let inside =
            |x: f32, y: f32| x >= 0.0 && y >= 0.0 && x <= (w - 1) as f32 && y <= (h - 1) as f32;
        // Edge points off the frame (a spot at the border) say nothing; leave them out.
        let mut diff: Vec<Option<[f32; 3]>> = (0..n)
            .map(|k| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                let (bx, by) = (cx + rad * a.cos(), cy + rad * a.sin());
                if !inside(bx, by) || !inside(bx + dx, by + dy) {
                    return None;
                }
                let t = local_mean(img, bx, by, d);
                let f = local_mean(img, bx + dx, by + dy, d);
                Some([t[0] - f[0], t[1] - f[1], t[2] - f[2]])
            })
            .collect();
        // Smooth around the ring so a single bright pixel on the edge doesn't streak inward.
        for _ in 0..2 {
            let prev = diff.clone();
            for k in 0..n {
                let Some(b) = prev[k] else { continue };
                let a = prev[(k + n - 1) % n].unwrap_or(b);
                let c = prev[(k + 1) % n].unwrap_or(b);
                diff[k] = Some(std::array::from_fn(|i| (a[i] + 2.0 * b[i] + c[i]) / 4.0));
            }
        }
        diff.into_iter()
            .enumerate()
            .filter_map(|(k, v)| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                v.map(|v| ([rad * a.cos(), rad * a.sin()], v))
            })
            .collect()
    } else {
        Vec::new()
    };

    let x0 = (cx - rad).floor().max(0.0) as usize;
    let x1 = ((cx + rad).ceil() as usize).min(w - 1);
    let y0 = (cy - rad).floor().max(0.0) as usize;
    let y1 = ((cy + rad).ceil() as usize).min(h - 1);
    if x0 > x1 || y0 > y1 {
        return;
    }
    let src: &RgbImage = img;
    let rows: Vec<(usize, Vec<u8>)> = (y0..=y1)
        .into_par_iter()
        .map(|y| {
            let mut row = src.data[(y * w + x0) * 3..(y * w + x1 + 1) * 3].to_vec();
            for x in x0..=x1 {
                let (qx, qy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let t = (qx * qx + qy * qy).sqrt() / rad;
                if t >= 1.0 {
                    continue;
                }
                let a = if t <= inner {
                    1.0
                } else {
                    let u = (1.0 - t) / (1.0 - inner).max(1e-6);
                    u * u * (3.0 - 2.0 * u)
                } * opacity;
                let mut v = sample(src, x as f32 + dx, y as f32 + dy);
                if !ring.is_empty() {
                    let q2 = qx * qx + qy * qy;
                    let num = (rad * rad - q2).max(0.0);
                    let (mut acc, mut wsum) = ([0f32; 3], 0f32);
                    for ([bx, by], d) in &ring {
                        let e2 = ((bx - qx).powi(2) + (by - qy).powi(2)).max(1e-3);
                        let wk = num / e2;
                        wsum += wk;
                        for c in 0..3 {
                            acc[c] += wk * d[c];
                        }
                    }
                    for c in 0..3 {
                        v[c] += acc[c] / wsum.max(1e-6);
                    }
                }
                let i = (x - x0) * 3;
                for c in 0..3 {
                    let was = row[i + c] as f32;
                    row[i + c] = (was + (v[c] - was) * a).round().clamp(0.0, 255.0) as u8;
                }
            }
            (y, row)
        })
        .collect();
    for (y, row) in rows {
        img.data[(y * w + x0) * 3..(y * w + x1 + 1) * 3].copy_from_slice(&row);
    }
}

/// Pick where a spot at (`x`, `y`) with radius `r` (same units as [`Spot`]) should copy
/// from: a nearby circle, fully inside the frame and clear of the spot, whose surroundings
/// continue the spot's own and whose inside is as plain as those surroundings, so the
/// patch neither breaks a line nor carries another blemish in. Returns 0..1 coordinates.
pub fn find_source(img: &RgbImage, x: f64, y: f64, r: f64) -> Point {
    find_source_except(img, x, y, r, &[])
}

/// [`find_source`], skipping sources within a radius of any in `avoid`: Lightroom's "/"
/// for a new source, which tries somewhere else each time.
pub fn find_source_except(img: &RgbImage, x: f64, y: f64, r: f64, avoid: &[Point]) -> Point {
    let (w, h) = (img.width as f32, img.height as f32);
    let long = w.max(h);
    let rad = (r.clamp(MIN_R, MAX_R) as f32 * long).max(1.0);
    let (cx, cy) = (x as f32 * w, y as f32 * h);

    // Samples on a band just outside the circle and inside it, as offsets from a centre.
    const ANGLES: usize = 24;
    let at = |k: usize, f: f32| {
        let a = k as f32 / ANGLES as f32 * std::f32::consts::TAU;
        (f * rad * a.cos(), f * rad * a.sin())
    };
    let band: Vec<(f32, f32)> = [1.15, 1.35, 1.6]
        .iter()
        .flat_map(|&f| (0..ANGLES).map(move |k| at(k, f)))
        .collect();
    let inside: Vec<(f32, f32)> = std::iter::once((0.0, 0.0))
        .chain(
            [0.3, 0.6, 0.9]
                .iter()
                .flat_map(|&f| (0..ANGLES).map(move |k| at(k, f))),
        )
        .collect();
    // Colour counts, not just light: a cyan stripe on a navy vest is as dark as the vest.
    type Px = [f32; 3];
    let read = |ox: f32, oy: f32, pts: &[(f32, f32)]| -> Vec<Px> {
        pts.iter()
            .map(|&(px, py)| sample(img, ox + px, oy + py))
            .collect()
    };
    let mean = |v: &[Px]| -> Px {
        let n = v.len() as f32;
        std::array::from_fn(|c| v.iter().map(|p| p[c]).sum::<f32>() / n)
    };
    let d2 = |a: Px, b: Px| (0..3).map(|c| (a[c] - b[c]).powi(2)).sum::<f32>() / 3.0;
    let var = |v: &[Px]| {
        let m = mean(v);
        v.iter().map(|&p| d2(p, m)).sum::<f32>() / v.len() as f32
    };

    let here = read(cx, cy, &band);
    let here_mean = mean(&here);
    let here_var = var(&here);

    let mut best: Option<(f32, f32, f32)> = None;
    for dist in [2.2f32, 2.8, 3.6, 4.6, 6.0] {
        for k in 0..32 {
            let a = k as f32 / 32.0 * std::f32::consts::TAU;
            let (sx, sy) = (cx + dist * rad * a.cos(), cy + dist * rad * a.sin());
            if sx - rad < 0.0 || sy - rad < 0.0 || sx + rad > w || sy + rad > h {
                continue;
            }
            if avoid
                .iter()
                .any(|p| (p.x as f32 * w - sx).hypot(p.y as f32 * h - sy) < rad)
            {
                continue;
            }
            let there = read(sx, sy, &band);
            let there_mean = mean(&there);
            // Texture: the bands' shapes compared with their means taken out.
            let texture = here
                .iter()
                .zip(&there)
                .map(|(&a, &b)| {
                    let a: Px = std::array::from_fn(|c| a[c] - here_mean[c]);
                    let b: Px = std::array::from_fn(|c| b[c] - there_mean[c]);
                    d2(a, b)
                })
                .sum::<f32>()
                / here.len() as f32;
            // Plain inside: no more going on in the source than around it, and no less
            // like its own surroundings than the spot's are.
            let core = read(sx, sy, &inside);
            let busy =
                (var(&core) - here_var.min(var(&there))).max(0.0) + d2(mean(&core), there_mean);
            let tone = d2(there_mean, here_mean);
            let score = texture + 2.0 * busy + 0.5 * tone + dist * 2.0;
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, sx, sy));
            }
        }
    }
    // A spot too big for any circle to fit beside it: copy from straight across the frame.
    let (sx, sy) = best.map_or_else(
        || {
            (
                (w - cx).clamp(rad, (w - rad).max(rad)),
                (h - cy).clamp(rad, (h - rad).max(rad)),
            )
        },
        |b| (b.1, b.2),
    );
    Point {
        x: (sx / w) as f64,
        y: (sy / h) as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A soft diagonal gradient with fine deterministic grain, 400 x 300.
    fn ground() -> RgbImage {
        let (w, h) = (400, 300);
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let g = 90.0 + 60.0 * (x as f32 / w as f32) + 30.0 * (y as f32 / h as f32);
                let n = (((x * 7919 + y * 104_729) % 13) as f32 - 6.0) * 1.5;
                for c in 0..3 {
                    data.push((g + n + c as f32 * 10.0).clamp(0.0, 255.0) as u8);
                }
            }
        }
        RgbImage {
            width: w,
            height: h,
            data,
        }
    }

    fn blot(img: &mut RgbImage, cx: usize, cy: usize, r: usize) {
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                if (x as f32 - cx as f32).hypot(y as f32 - cy as f32) <= r as f32 {
                    let i = (y * img.width + x) * 3;
                    img.data[i..i + 3].copy_from_slice(&[20, 15, 10]);
                }
            }
        }
    }

    fn mean_in(img: &RgbImage, cx: f32, cy: f32, r: f32) -> [f32; 3] {
        let (mut s, mut n) = ([0f32; 3], 0f32);
        for y in 0..img.height {
            for x in 0..img.width {
                if (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) < r {
                    let i = (y * img.width + x) * 3;
                    for (acc, &v) in s.iter_mut().zip(&img.data[i..i + 3]) {
                        *acc += v as f32;
                    }
                    n += 1.0;
                }
            }
        }
        s.map(|v| v / n)
    }

    fn spot(mode: SpotMode, from: Point) -> Spot {
        Spot {
            id: "spot".into(),
            mode,
            x: 0.5,
            y: 0.5,
            r: 15.0 / 400.0,
            from,
            feather: 30.0,
            opacity: 100.0,
            source: None,
        }
    }

    #[test]
    fn heal_hides_a_blot_and_leaves_the_rest_alone() {
        let clean = ground();
        let mut img = clean.clone();
        blot(&mut img, 200, 150, 8);
        let before = img.clone();
        let from = find_source(&img, 0.5, 0.5, 15.0 / 400.0);
        apply(&mut img, &[spot(SpotMode::Heal, from)]);
        let want = mean_in(&clean, 200.0, 150.0, 12.0);
        let got = mean_in(&img, 200.0, 150.0, 12.0);
        for c in 0..3 {
            assert!((want[c] - got[c]).abs() < 4.0, "{want:?} vs {got:?}");
        }
        // Outside the circle nothing moved.
        for y in 0..img.height {
            for x in 0..img.width {
                if (x as f32 + 0.5 - 200.0).hypot(y as f32 + 0.5 - 150.0) > 16.0 {
                    let i = (y * img.width + x) * 3;
                    assert_eq!(img.data[i..i + 3], before.data[i..i + 3], "{x},{y}");
                }
            }
        }
    }

    #[test]
    fn heal_matches_the_light_around_the_spot_where_clone_does_not() {
        // A source well to the right is brighter (the gradient); Heal brings it to the
        // spot's own level, Clone carries it over.
        let clean = ground();
        let from = Point { x: 0.85, y: 0.5 };
        let mut healed = clean.clone();
        apply(&mut healed, &[spot(SpotMode::Heal, from)]);
        let mut cloned = clean.clone();
        apply(&mut cloned, &[spot(SpotMode::Clone, from)]);
        let want = mean_in(&clean, 200.0, 150.0, 6.0)[0];
        let heal = mean_in(&healed, 200.0, 150.0, 6.0)[0];
        let clone = mean_in(&cloned, 200.0, 150.0, 6.0)[0];
        let there = mean_in(&clean, 340.0, 150.0, 6.0)[0];
        assert!((heal - want).abs() < 3.0, "heal {heal} vs {want}");
        assert!(
            (clone - there).abs() < 3.0,
            "clone {clone} vs source {there}"
        );
    }

    #[test]
    fn a_spot_over_the_frame_edge_ignores_what_lies_beyond_it() {
        // A bright strip along the bottom edge, as a label cut off by the frame would be.
        let mut clean = ground();
        let w = clean.width;
        for y in 296..300 {
            for x in 0..w {
                let i = (y * w + x) * 3;
                clean.data[i..i + 3].copy_from_slice(&[240, 120, 200]);
            }
        }
        let mut img = clean.clone();
        blot(&mut img, 200, 285, 6);
        let s = Spot {
            y: 290.0 / 300.0,
            from: Point { x: 0.5, y: 0.8 },
            ..spot(SpotMode::Heal, Point::default())
        };
        apply(&mut img, &[s]);
        let want = mean_in(&clean, 200.0, 283.0, 5.0);
        let got = mean_in(&img, 200.0, 283.0, 5.0);
        for c in 0..3 {
            assert!((want[c] - got[c]).abs() < 12.0, "{want:?} vs {got:?}");
        }
    }

    #[test]
    fn opacity_zero_and_a_spot_on_itself_change_nothing() {
        let clean = ground();
        let mut img = clean.clone();
        let mut s = spot(SpotMode::Clone, Point { x: 0.2, y: 0.2 });
        s.opacity = 0.0;
        let still = spot(SpotMode::Heal, Point { x: 0.5, y: 0.5 });
        apply(&mut img, &[s, still]);
        assert_eq!(img.data, clean.data);
    }

    #[test]
    fn the_source_avoids_other_blots_and_stays_in_frame() {
        let mut img = ground();
        blot(&mut img, 200, 150, 8);
        // Busy neighbours on three sides; the clean side is down.
        blot(&mut img, 245, 150, 8);
        blot(&mut img, 155, 150, 8);
        blot(&mut img, 200, 105, 8);
        let r = 15.0 / 400.0;
        let p = find_source(&img, 0.5, 0.5, r);
        let (sx, sy) = (p.x as f32 * 400.0, p.y as f32 * 300.0);
        let rad = 15.0;
        assert!(sx >= rad && sy >= rad && sx <= 400.0 - rad && sy <= 300.0 - rad);
        assert!(
            (sx - 200.0).hypot(sy - 150.0) >= 2.0 * rad,
            "clear of the spot"
        );
        for (bx, by) in [(245.0, 150.0), (155.0, 150.0), (200.0, 105.0)] {
            assert!(
                (sx - bx).hypot(sy - by) > rad + 8.0,
                "source {sx},{sy} takes in the blot at {bx},{by}"
            );
        }
    }

    #[test]
    fn a_new_source_is_somewhere_else() {
        let img = ground();
        let r = 15.0 / 400.0;
        let first = find_source(&img, 0.5, 0.5, r);
        let second = find_source_except(&img, 0.5, 0.5, r, &[first]);
        let apart = ((first.x - second.x) * 400.0).hypot((first.y - second.y) * 300.0);
        assert!(apart >= 15.0, "{first:?} then {second:?}");
    }

    #[test]
    fn spots_round_trip_with_defaults() {
        let s: Spot = serde_json::from_value(serde_json::json!({
            "id": "spot", "x": 0.4, "y": 0.6, "r": 0.01, "from": { "x": 0.45, "y": 0.6 }
        }))
        .unwrap();
        assert_eq!(s.mode, SpotMode::Heal);
        assert_eq!((s.feather, s.opacity), (DEFAULT_FEATHER, 100.0));
        let big = Spot { r: 0.9, ..s }.clamped();
        assert_eq!(big.r, MAX_R);
    }
}
