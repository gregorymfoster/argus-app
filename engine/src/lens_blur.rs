//! Lens Blur, Lightroom's: a fake shallow depth of field from a depth map. A depth model
//! ([`crate::analyzers::depth`]) estimates how near every pixel is; the Recipe's
//! `lensBlur` picks a focal range on that depth and blurs everything outside it, more the
//! farther it is from the range, with the shape of a real lens's out-of-focus highlights.
//! Nothing is generated: every output pixel is a weighted average of the Photo's own.
//!
//! It runs on the upright linear camera RGB before tone ([`crate::local::finish`]), so a
//! bright highlight spreads into a bright disc the way light does through a lens, and
//! Boost can make those discs bloom.
//!
//! The blur is layered (the usual way to fake depth of field without halos): pixels are
//! split by how much blur they need into a few layers, each layer is blurred with its own
//! radius and kernel together with its coverage, and the layers are laid over each other
//! from far to near. So a sharp subject never smears into the blurred background behind it,
//! and a blurred foreground spreads over what's behind it as it would through a lens.
//!
//! | field        | range / values                       | means                               |
//! |--------------|--------------------------------------|-------------------------------------|
//! | `amount`     | 0..100 (0 = off)                     | Blur Amount                         |
//! | `bokeh`      | `circle` `bubble` `blade` `ring`     | shape of out-of-focus highlights    |
//! | `boost`      | 0..100 (absent = 50)                 | how much those highlights bloom     |
//! | `focus`      | `[from, to]`, 0 (far) .. 100 (near)  | Focal range; absent = the subject   |
//! | `focusBrush` | `[{x, y, r}]` brush dots             | Refine: keep these areas sharp      |
//! | `blurBrush`  | `[{x, y, r}]` brush dots             | Refine: blur these as background    |

use crate::masks::{refine, smoothstep, Dot};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// Blur radius at Amount 100 and full distance from the focal range, as a fraction of the
/// long edge, so a preview and an export blur alike.
const MAX_RADIUS: f32 = 0.03;
/// Depth distance (0..1) from the focal range at which the blur is full.
const SPAN: f32 = 0.5;
/// Layers on each side of the focal range ([`RADII`]).
const LAYERS: usize = 8;
/// Largest kernel radius run at full size; a bigger blur runs on a smaller image.
const KERNEL_MAX: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bokeh {
    /// A clean disc, like a modern fast prime.
    #[default]
    Circle,
    /// Soap bubble: a disc with a bright rim, like older lenses.
    Bubble,
    /// A five-blade aperture's pentagon, like a stopped-down kit lens.
    Blade,
    /// A ring, like a mirror lens.
    Ring,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LensBlur {
    pub amount: f64,
    pub bokeh: Bokeh,
    pub boost: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<[f64; 2]>,
    #[serde(rename = "focusBrush", skip_serializing_if = "Vec::is_empty")]
    pub focus_brush: Vec<Dot>,
    #[serde(rename = "blurBrush", skip_serializing_if = "Vec::is_empty")]
    pub blur_brush: Vec<Dot>,
}

impl Default for LensBlur {
    fn default() -> Self {
        LensBlur {
            amount: 0.0,
            bokeh: Bokeh::Circle,
            boost: 50.0,
            focus: None,
            focus_brush: Vec::new(),
            blur_brush: Vec::new(),
        }
    }
}

impl LensBlur {
    /// Whether rendering blurs anything.
    pub fn active(&self) -> bool {
        self.amount > 0.0
    }

    pub fn is_default(&self) -> bool {
        *self == LensBlur::default()
    }

    pub fn clamped(mut self) -> Self {
        self.amount = self.amount.clamp(0.0, 100.0);
        self.boost = self.boost.clamp(0.0, 100.0);
        self.focus = self.focus.map(|[a, b]| {
            let (a, b) = (a.clamp(0.0, 100.0), b.clamp(0.0, 100.0));
            [a.min(b), a.max(b)]
        });
        self
    }
}

/// A depth map: how near each pixel is, 0 (farthest in the Photo) to 1 (nearest), on the
/// upright frame at the depth model's resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct Depth {
    pub plane: Vec<f32>,
    pub width: usize,
    pub height: usize,
}

impl Depth {
    /// Depth at (x, y) in 0..1 of the frame, bilinear.
    pub fn at(&self, x: f64, y: f64) -> f32 {
        let fx = (x as f32 * self.width as f32 - 0.5).clamp(0.0, self.width as f32 - 1.0);
        let fy = (y as f32 * self.height as f32 - 0.5).clamp(0.0, self.height as f32 - 1.0);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let p = |x: usize, y: usize| self.plane[y * self.width + x];
        let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * tx;
        let bot = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * tx;
        top + (bot - top) * ty
    }

    /// A share of the pixels at each depth, far to near, in `bins` steps.
    pub fn histogram(&self, bins: usize) -> Vec<f32> {
        let mut h = vec![0f32; bins];
        for v in &self.plane {
            h[((v * bins as f32) as usize).min(bins - 1)] += 1.0;
        }
        let n = self.plane.len().max(1) as f32;
        h.iter_mut().for_each(|v| *v /= n);
        h
    }
}

/// The focal range Lens Blur keeps sharp, in depth 0..1: the Recipe's `focus`, else the
/// depths the subject covers (`subject` is its Mask at the depth map's size), else what's in
/// the middle of the frame.
pub fn focus_range(lb: &LensBlur, depth: &Depth, subject: Option<&[f32]>) -> [f32; 2] {
    if let Some([a, b]) = lb.focus {
        return [a as f32 / 100.0, b as f32 / 100.0];
    }
    let mut under: Vec<f32> = match subject {
        Some(s) => depth
            .plane
            .iter()
            .zip(s)
            .filter(|(_, m)| **m > 0.5)
            .map(|(d, _)| *d)
            .collect(),
        None => Vec::new(),
    };
    if under.len() < depth.plane.len() / 100 {
        // No subject worth the name: the middle fifth of the frame.
        let (w, h) = (depth.width, depth.height);
        under = (h * 2 / 5..h * 3 / 5)
            .flat_map(|y| (w * 2 / 5..w * 3 / 5).map(move |x| (x, y)))
            .map(|(x, y)| depth.plane[y * w + x])
            .collect();
    }
    if under.is_empty() {
        return [0.0, 1.0];
    }
    under.sort_by(f32::total_cmp);
    let q = |p: f32| under[((under.len() - 1) as f32 * p) as usize];
    [(q(0.1) - 0.03).max(0.0), (q(0.9) + 0.03).min(1.0)]
}

/// The focal range around one point of the frame, as Lightroom's point focus: the depth
/// there, give or take a little, in the Recipe's 0..100.
pub fn focus_at(depth: &Depth, x: f64, y: f64) -> [f64; 2] {
    let d = depth.at(x, y) as f64 * 100.0;
    let r = |v: f64| (v.clamp(0.0, 100.0) * 10.0).round() / 10.0;
    [r(d - 6.0), r(d + 6.0)]
}

/// How out of focus each pixel is, signed: 0 in the focal range, toward -1 behind it and +1
/// in front, at `w` x `h`. `depth` is already at that size; `keep` and `push` are the
/// Refine brushes' planes (and the subject's, when the focus is the subject).
pub fn defocus(
    depth: &[f32],
    range: [f32; 2],
    keep: Option<&[f32]>,
    push: Option<&[f32]>,
) -> Vec<f32> {
    depth
        .par_iter()
        .enumerate()
        .map(|(i, &d)| {
            let push = push.map_or(0.0, |p| p[i]);
            let d = d * (1.0 - push); // the Blur brush sends a pixel to the back
            let s = if d < range[0] {
                -((range[0] - d) / SPAN).min(1.0)
            } else if d > range[1] {
                ((d - range[1]) / SPAN).min(1.0)
            } else {
                0.0
            };
            s * (1.0 - keep.map_or(0.0, |k| k[i]))
        })
        .collect()
}

/// Everything [`apply`] needs besides the image.
pub struct Plan<'a> {
    pub blur: &'a LensBlur,
    /// [`defocus`] at the image's size.
    pub defocus: &'a [f32],
}

/// The blur radius of layer `k` (0..=[`LAYERS`]) as a share of the largest: closer steps
/// where a pixel of difference shows.
fn radius(k: usize) -> f32 {
    (k as f32 / LAYERS as f32).powf(1.5)
}

/// Blur `cam` (upright linear RGB, `w` x `h`, interleaved) in place by [`Plan`].
pub fn apply(cam: &mut [f32], w: usize, h: usize, plan: &Plan) {
    let lb = plan.blur;
    let max_r = MAX_RADIUS * (lb.amount as f32 / 100.0) * w.max(h) as f32;
    if max_r < 0.5 || plan.defocus.len() != w * h {
        return;
    }
    // Boost: highlights out of focus get brighter, so their discs bloom like real ones.
    let boost = lb.boost as f32 / 100.0 * 3.0;
    let src: Vec<[f32; 3]> = cam
        .par_chunks(3)
        .zip(plan.defocus)
        .map(|(p, s)| {
            let lum = 0.3 * p[0] + 0.55 * p[1] + 0.15 * p[2];
            let g = 1.0 + boost * smoothstep(0.4, 1.0, lum) * (s.abs() * 4.0).min(1.0);
            [p[0] * g, p[1] * g, p[2] * g]
        })
        .collect();

    // Far layers first, then the focal range, then the near ones.
    let order = (1..=LAYERS)
        .rev()
        .map(|k| (k, -1.0f32))
        .chain(std::iter::once((0, 0.0)))
        .chain((1..=LAYERS).map(|k| (k, 1.0f32)));
    let mut acc = vec![[0f32; 4]; w * h];
    for (k, side) in order {
        let t = radius(k);
        let member: Vec<f32> = plan
            .defocus
            .par_iter()
            .map(|&s| {
                if k > 0 && s * side <= 0.0 {
                    return 0.0;
                }
                weight(s.abs(), k)
            })
            .collect();
        if member.par_iter().all(|m| *m < 1e-3) {
            continue;
        }
        let layer: Vec<[f32; 4]> = src
            .par_iter()
            .zip(&member)
            .map(|(p, &m)| [p[0] * m, p[1] * m, p[2] * m, m])
            .collect();
        let layer = if k == 0 {
            layer
        } else {
            bokeh_blur(&layer, w, h, t * max_r, lb.bokeh)
        };
        acc.par_iter_mut().zip(&layer).for_each(|(a, l)| {
            let keep = 1.0 - l[3].clamp(0.0, 1.0);
            for c in 0..4 {
                a[c] = l[c] + a[c] * keep;
            }
        });
    }
    cam.par_chunks_mut(3)
        .zip(&acc)
        .zip(&src)
        .for_each(|((o, a), s)| {
            if a[3] > 1e-4 {
                o.copy_from_slice(&[a[0] / a[3], a[1] / a[3], a[2] / a[3]]);
            } else {
                o.copy_from_slice(s);
            }
        });
}

/// How much of a pixel `t` out of focus (0..1) belongs to layer `k`: linear between the
/// two layers around it, so the blur grows smoothly with depth.
fn weight(t: f32, k: usize) -> f32 {
    let c = radius(k);
    if k > 0 {
        let lo = radius(k - 1);
        if t >= lo && t <= c {
            return (t - lo) / (c - lo);
        }
    }
    if k < LAYERS {
        let hi = radius(k + 1);
        if t >= c && t <= hi {
            return 1.0 - (t - c) / (hi - c);
        }
    }
    0.0
}

/// The bokeh kernel of radius `r` (in pixels) as offsets and weights summing to 1.
fn kernel(r: f32, bokeh: Bokeh) -> Vec<(isize, isize, f32)> {
    let n = r.ceil() as isize + 1;
    let mut k = Vec::new();
    for dy in -n..=n {
        for dx in -n..=n {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let edge = match bokeh {
                Bokeh::Blade => {
                    // A regular pentagon, one point up.
                    let a = (dy as f32).atan2(dx as f32) + std::f32::consts::FRAC_PI_2;
                    let seg = std::f32::consts::TAU / 5.0;
                    let off = a.rem_euclid(seg) - seg / 2.0;
                    r * (seg / 2.0).cos() / off.cos()
                }
                _ => r,
            };
            let mut v = (edge + 0.5 - d).clamp(0.0, 1.0);
            match bokeh {
                Bokeh::Bubble => v *= 0.45 + 0.55 * (d / r.max(1.0)).min(1.0).powi(2),
                Bokeh::Ring => v *= (d - r * 0.62 + 0.5).clamp(0.0, 1.0),
                _ => {}
            }
            if v > 0.0 {
                k.push((dx, dy, v));
            }
        }
    }
    let sum: f32 = k.iter().map(|t| t.2).sum();
    k.iter_mut().for_each(|t| t.2 /= sum.max(1e-6));
    k
}

/// Blur a premultiplied RGBA image with a bokeh kernel of `radius` pixels. Big radii run on
/// a smaller copy (a blur that size has no fine detail to lose) and come back bilinear.
fn bokeh_blur(img: &[[f32; 4]], w: usize, h: usize, radius: f32, bokeh: Bokeh) -> Vec<[f32; 4]> {
    if radius < 0.5 {
        return img.to_vec();
    }
    let f = (radius / KERNEL_MAX).ceil().max(1.0) as usize;
    let (small, sw, sh) = if f > 1 {
        downsample(img, w, h, f)
    } else {
        (img.to_vec(), w, h)
    };
    let k = kernel(radius / f as f32, bokeh);
    let blurred: Vec<[f32; 4]> = (0..sw * sh)
        .into_par_iter()
        .map(|i| {
            let (x, y) = ((i % sw) as isize, (i / sw) as isize);
            let mut o = [0f32; 4];
            for &(dx, dy, wt) in &k {
                let sx = (x + dx).clamp(0, sw as isize - 1) as usize;
                let sy = (y + dy).clamp(0, sh as isize - 1) as usize;
                let p = &small[sy * sw + sx];
                for c in 0..4 {
                    o[c] += p[c] * wt;
                }
            }
            o
        })
        .collect();
    if f == 1 {
        blurred
    } else {
        upsample(&blurred, sw, sh, w, h)
    }
}

fn downsample(img: &[[f32; 4]], w: usize, h: usize, f: usize) -> (Vec<[f32; 4]>, usize, usize) {
    let (sw, sh) = (w.div_ceil(f), h.div_ceil(f));
    let out = (0..sw * sh)
        .into_par_iter()
        .map(|i| {
            let (x0, y0) = ((i % sw) * f, (i / sw) * f);
            let mut o = [0f32; 4];
            let mut n = 0.0;
            for y in y0..(y0 + f).min(h) {
                for x in x0..(x0 + f).min(w) {
                    let p = &img[y * w + x];
                    for c in 0..4 {
                        o[c] += p[c];
                    }
                    n += 1.0;
                }
            }
            o.map(|v| v / n)
        })
        .collect();
    (out, sw, sh)
}

fn upsample(img: &[[f32; 4]], sw: usize, sh: usize, w: usize, h: usize) -> Vec<[f32; 4]> {
    let (fx, fy) = (sw as f32 / w as f32, sh as f32 / h as f32);
    (0..w * h)
        .into_par_iter()
        .map(|i| {
            let x = ((i % w) as f32 + 0.5) * fx - 0.5;
            let y = ((i / w) as f32 + 0.5) * fy - 0.5;
            let (x, y) = (x.clamp(0.0, sw as f32 - 1.0), y.clamp(0.0, sh as f32 - 1.0));
            let (x0, y0) = (x.floor() as usize, y.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
            let (tx, ty) = (x - x0 as f32, y - y0 as f32);
            let mut o = [0f32; 4];
            for c in 0..4 {
                let a = img[y0 * sw + x0][c] * (1.0 - tx) + img[y0 * sw + x1][c] * tx;
                let b = img[y1 * sw + x0][c] * (1.0 - tx) + img[y1 * sw + x1][c] * tx;
                o[c] = a * (1.0 - ty) + b * ty;
            }
            o
        })
        .collect()
}

/// The depth map at `w` x `h`, its edges snapped to the image's (`guide` is the image's
/// encoded luma at that size), so the focal range ends where the subject does.
pub fn depth_at_size(depth: &Depth, guide: &[f32], w: usize, h: usize) -> Vec<f32> {
    refine::refine(
        &depth.plane,
        depth.width,
        depth.height,
        guide,
        w,
        h,
        1.0 / 160.0,
    )
}

/// Lightroom's Visualize depth: the depth map in a warm (near) to cool (far) ramp, with
/// what's outside the focal range dimmed. `range` is in 0..1. 8-bit RGB, `w` x `h`.
pub fn visualize(depth: &[f32], range: [f32; 2]) -> Vec<u8> {
    // A few stops of a perceptual ramp from deep blue-violet through magenta to warm yellow.
    const RAMP: [[f32; 3]; 5] = [
        [0.10, 0.06, 0.28],
        [0.36, 0.11, 0.48],
        [0.72, 0.22, 0.42],
        [0.96, 0.52, 0.28],
        [0.99, 0.88, 0.55],
    ];
    depth
        .par_iter()
        .flat_map_iter(|&d| {
            let t = d.clamp(0.0, 1.0) * (RAMP.len() - 1) as f32;
            let i = (t.floor() as usize).min(RAMP.len() - 2);
            let f = t - i as f32;
            let inside = d >= range[0] && d <= range[1];
            let dim = if inside { 1.0 } else { 0.45 };
            (0..3).map(move |c| {
                let v = RAMP[i][c] + (RAMP[i + 1][c] - RAMP[i][c]) * f;
                (v * dim * 255.0).round() as u8
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(w: usize, h: usize, f: impl Fn(usize, usize) -> f32) -> Vec<f32> {
        (0..w * h).map(|i| f(i % w, i / w)).collect()
    }

    #[test]
    fn layer_weights_split_each_pixel_between_two_layers() {
        for t in [0.0, 0.03, 0.2, 0.5, 0.77, 1.0] {
            let sum: f32 = (0..=LAYERS).map(|k| weight(t, k)).sum();
            assert!((sum - 1.0).abs() < 1e-5, "t {t}: {sum}");
        }
    }

    #[test]
    fn kernels_sum_to_one_and_take_their_shape() {
        for b in [Bokeh::Circle, Bokeh::Bubble, Bokeh::Blade, Bokeh::Ring] {
            let k = kernel(5.0, b);
            let s: f32 = k.iter().map(|t| t.2).sum();
            assert!((s - 1.0).abs() < 1e-4, "{b:?}");
        }
        let at = |k: &[(isize, isize, f32)], x: isize, y: isize| {
            k.iter().find(|t| t.0 == x && t.1 == y).map_or(0.0, |t| t.2)
        };
        let ring = kernel(5.0, Bokeh::Ring);
        assert!(
            at(&ring, 0, 0) == 0.0 && at(&ring, 5, 0) > 0.0,
            "a ring is hollow"
        );
        let bubble = kernel(5.0, Bokeh::Bubble);
        assert!(
            at(&bubble, 4, 0) > at(&bubble, 0, 0),
            "a bubble has a bright rim"
        );
        let blade = kernel(6.0, Bokeh::Blade);
        assert!(
            at(&blade, 0, -6) > 0.0 && at(&blade, 0, 6) == 0.0,
            "a pentagon points up"
        );
    }

    #[test]
    fn the_focal_range_stays_sharp_and_the_background_blurs() {
        // A checkerboard: the left half is in focus (near), the right half far behind it.
        let (w, h) = (96, 64);
        let mut cam: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let v = if ((i % w) / 2 + (i / w) / 2) % 2 == 0 {
                    0.8
                } else {
                    0.1
                };
                [v, v, v]
            })
            .collect();
        let before = cam.clone();
        let depth = plane(w, h, |x, _| if x < w / 2 { 0.9 } else { 0.1 });
        let lb = LensBlur {
            amount: 100.0,
            boost: 0.0,
            ..Default::default()
        };
        let s = defocus(&depth, [0.8, 1.0], None, None);
        apply(
            &mut cam,
            w,
            h,
            &Plan {
                blur: &lb,
                defocus: &s,
            },
        );
        let px = |img: &[f32], x: usize, y: usize| img[(y * w + x) * 3];
        // Sharp side: untouched away from the boundary.
        for y in 10..50 {
            for x in 4..40 {
                assert!(
                    (px(&cam, x, y) - px(&before, x, y)).abs() < 1e-4,
                    "({x},{y})"
                );
            }
        }
        // Far side: the checkerboard averages out toward its mean.
        for y in 20..44 {
            for x in 70..90 {
                assert!(
                    (px(&cam, x, y) - 0.45).abs() < 0.12,
                    "({x},{y}) {}",
                    px(&cam, x, y)
                );
            }
        }
    }

    #[test]
    fn a_sharp_subject_does_not_bleed_into_the_background_behind_it() {
        // A bright square in focus over a dark, far, flat background.
        let (w, h) = (80, 80);
        let inside = |x: usize, y: usize| (30..50).contains(&x) && (30..50).contains(&y);
        let mut cam: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let v = if inside(i % w, i / w) { 1.0 } else { 0.05 };
                [v, v, v]
            })
            .collect();
        let depth = plane(w, h, |x, y| if inside(x, y) { 0.9 } else { 0.0 });
        let lb = LensBlur {
            amount: 100.0,
            boost: 0.0,
            ..Default::default()
        };
        let s = defocus(&depth, [0.8, 1.0], None, None);
        apply(
            &mut cam,
            w,
            h,
            &Plan {
                blur: &lb,
                defocus: &s,
            },
        );
        // Just outside the square the background stays dark: no glow of the subject.
        assert!(cam[(40 * w + 27) * 3] < 0.1, "{}", cam[(40 * w + 27) * 3]);
        assert!((cam[(40 * w + 40) * 3] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn brushes_keep_or_push_and_the_subject_sets_the_range() {
        let depth = vec![0.5f32; 4];
        let keep = [1.0, 0.0, 0.0, 0.0];
        let push = [0.0, 1.0, 0.0, 0.0];
        let s = defocus(&depth, [0.4, 0.6], Some(&keep), Some(&push));
        assert_eq!(s[0], 0.0);
        assert_eq!(s[1], -0.8, "pushed to the back");
        assert_eq!(s[2], 0.0);

        let d = Depth {
            plane: plane(10, 10, |x, _| x as f32 / 10.0),
            width: 10,
            height: 10,
        };
        let subject = plane(10, 10, |x, _| if x >= 7 { 1.0 } else { 0.0 });
        let r = focus_range(&LensBlur::default(), &d, Some(&subject));
        assert!(r[0] > 0.6 && r[0] < 0.7 && r[1] > 0.89, "{r:?}");
        let set = LensBlur {
            focus: Some([20.0, 40.0]),
            ..Default::default()
        };
        assert_eq!(focus_range(&set, &d, Some(&subject)), [0.2, 0.4]);
        assert_eq!(focus_at(&d, 0.55, 0.5), [44.0, 56.0]);
    }
}
