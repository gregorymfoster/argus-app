//! Auto straighten, as Lightroom's Crop › Auto: find how far a Photo's verticals or its
//! horizon lean and turn it level. A Hough transform over the strong edges finds long
//! straight lines within 12° of plumb or level. On its own (2026-10-22) that turned too many
//! of Greg's indoor people shots, whose long lines are mostly tables, shelves and walls
//! receding from a wide lens. So the scene model ([`Scene`], DeepLab on ADE20K) decides which
//! edges may vote: verticals only on architecture (walls, buildings, doors, windows, columns,
//! poles), never on people; a horizon only where sky meets open water. Verticals (walls, posts, door frames) go first,
//! since a table or counter running away from the camera isn't level in the picture even
//! when the camera is; their weighted median shrugs off the lean of converging verticals.
//! Without enough verticals on both sides, one horizontal line across most of the frame
//! (a horizon) decides. It only ever proposes a turn of up to 5°. A Photo with
//! neither, or whose lines disagree, is left alone rather than guessed at.

use crate::masks::models::SCENE_OUT;
use crate::RgbImage;

/// The largest tilt it looks for, in degrees: past this a lean is a choice, not a mistake.
pub const MAX_TILT: f64 = 12.0;
/// Below this the Photo is already level.
pub const LEVEL: f64 = 0.15;

/// What Auto straighten found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Level {
    /// Turn it by this many degrees (the Recipe's crop angle: positive turns it clockwise).
    Turn(f64),
    /// Lines found, and they're level already.
    Level,
    /// No clear horizon or verticals to go by.
    Unsure,
}

const BIN: f64 = 0.1;
/// Each edge pixel votes for angles this close to its own direction.
const SPREAD: f64 = 2.0;

/// What the scene model says is where, at [`SCENE_OUT`]² cells over the whole frame.
pub struct Scene {
    pub labels: Vec<u8>,
}

/// ADE20K classes whose edges stand plumb in the world: wall, building, windowpane, cabinet,
/// door, house, mirror, wardrobe, column, skyscraper, fireplace, refrigerator, screen door,
/// bookcase, blind, streetlight, tower, pole.
const PLUMB: [u8; 18] = [
    1, 2, 9, 11, 15, 26, 28, 36, 43, 49, 50, 51, 59, 63, 64, 85, 88, 94,
];
/// Open water: water, sea, river, lake.
const WATER: [u8; 4] = [22, 27, 61, 129];
const SKY: u8 = 3;
const PERSON: u8 = 13;

impl Scene {
    /// Classes within one cell of pixel (x, y) of a `w`×`h` image.
    fn near(&self, x: usize, y: usize, w: usize, h: usize) -> impl Iterator<Item = u8> + '_ {
        let n = SCENE_OUT;
        let cx = (x * n / w).min(n - 1) as i64;
        let cy = (y * n / h).min(n - 1) as i64;
        (-1i64..=1).flat_map(move |dy| {
            (-1i64..=1).filter_map(move |dx| {
                let (u, v) = (cx + dx, cy + dy);
                ((0..n as i64).contains(&u) && (0..n as i64).contains(&v))
                    .then(|| self.labels[v as usize * n + u as usize])
            })
        })
    }
    /// An edge here may vote for a vertical: on architecture and off people.
    fn plumb(&self, x: usize, y: usize, w: usize, h: usize) -> bool {
        let mut arch = false;
        for c in self.near(x, y, w, h) {
            if c == PERSON {
                return false;
            }
            arch |= PLUMB.contains(&c);
        }
        arch
    }
    /// An edge here may vote for a horizon: where sky meets open water.
    fn horizon(&self, x: usize, y: usize, w: usize, h: usize) -> bool {
        let (mut sky, mut water) = (false, false);
        for c in self.near(x, y, w, h) {
            sky |= c == SKY;
            water |= WATER.contains(&c);
        }
        sky && water
    }
}

/// How much evidence [`find`] wants before it answers. The defaults are the loosest of the
/// settings `bench/straighten_bench.py` tried (2026-11-01): on 152 of Greg's edits tilted
/// ±3°, 40 of the 41 copies it answered were put right, and its turns on the originals
/// looked right by eye up to 3.5° (bigger ones were misreads; [`crate::framing::MAX_TURN`]).
#[derive(Debug, Clone, Copy)]
pub struct Gates {
    /// Separate upright things (posts, frames) the verticals must come from.
    pub objects: usize,
    /// The share of vertical line length each half of the frame must hold.
    pub balance: f64,
    /// Total vertical line length, in short sides of the frame.
    pub reach: f64,
    /// A horizon must span this share of the width.
    pub horizon: f64,
    /// Past this lean (degrees) the answer is Unsure.
    pub max_tilt: f64,
    /// Median disagreement (degrees) of the verticals with their fit.
    pub misfit: f64,
}

impl Default for Gates {
    fn default() -> Self {
        Gates {
            objects: 2,
            balance: 0.1,
            reach: 0.4,
            horizon: 0.4,
            max_tilt: 6.0,
            misfit: 1.0,
        }
    }
}

impl std::str::FromStr for Gates {
    type Err = String;
    /// "objects,balance,reach,horizon,max_tilt,misfit", for the bench.
    fn from_str(s: &str) -> Result<Self, String> {
        let v: Vec<f64> = s
            .split(',')
            .map(|x| x.trim().parse::<f64>().map_err(|e| format!("{x}: {e}")))
            .collect::<Result<_, _>>()?;
        let [o, b, r, h, m, f] = v[..] else {
            return Err(format!("want 6 numbers, got {}", v.len()));
        };
        Ok(Gates {
            objects: o as usize,
            balance: b,
            reach: r,
            horizon: h,
            max_tilt: m,
            misfit: f,
        })
    }
}

/// How far `img` leans, as the crop angle that levels it. With a `scene`, only edges on
/// architecture vote for verticals and only a sky-over-water edge for a horizon.
pub fn find(img: &RgbImage, scene: Option<&Scene>) -> Level {
    find_with(img, scene, &Gates::default())
}

/// [`find`] with other [`Gates`].
pub fn find_with(img: &RgbImage, scene: Option<&Scene>, g: &Gates) -> Level {
    let (w, h) = (img.width, img.height);
    if w < 64 || h < 64 {
        return Level::Unsure;
    }
    let luma: Vec<f32> = img
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
        .collect();
    // Soften first: on a slightly tilted line the pixel grid makes a staircase whose flat
    // treads would vote for level.
    let luma = blur(&blur(&luma, w, h), w, h);
    let at = |x: usize, y: usize| luma[y * w + x];
    let mut edges = Vec::with_capacity(w * h / 8);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let gx = (at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1))
                - (at(x - 1, y - 1) + 2.0 * at(x - 1, y) + at(x - 1, y + 1));
            let gy = (at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1))
                - (at(x - 1, y - 1) + 2.0 * at(x, y - 1) + at(x + 1, y - 1));
            let m = (gx * gx + gy * gy).sqrt();
            if m > 20.0 {
                edges.push((x, y, gx, gy, m));
            }
        }
    }
    if edges.len() < 200 {
        return Level::Unsure;
    }
    let mut ms: Vec<f32> = edges.iter().map(|e| e.4).collect();
    let cut = ms.len() * 3 / 4;
    let top = ms.iter().copied().fold(0.0f32, f32::max);
    let floor = ms
        .select_nth_unstable_by(cut, f32::total_cmp)
        .1
        .max(top * 0.15);
    let short = w.min(h) as f64;
    // Lines in each family: (tilt, length in pixels, where across the frame it sits).
    let mut families = [Vec::new(), Vec::new()];
    for (family, lines) in families.iter_mut().enumerate() {
        // Family 1 (verticals) is the same search with x and y swapped.
        let pts: Vec<(f64, f64, f64)> = edges
            .iter()
            .filter(|e| e.4 >= floor)
            .filter(|e| match scene {
                None => true,
                Some(s) if family == 0 => s.horizon(e.0, e.1, w, h),
                Some(s) => s.plumb(e.0, e.1, w, h),
            })
            .filter_map(|&(x, y, gx, gy, _)| {
                // With y up, a line rising to the right has a positive angle, and turning
                // the picture clockwise by that much levels it.
                let line = (gx as f64).atan2(gy as f64).to_degrees();
                let (dev, u, v) = if family == 0 {
                    (fold(line), x as f64, (h - 1 - y) as f64)
                } else {
                    // Measured from the u (up) axis, so a lean to the left is positive here.
                    (-fold(line - 90.0), (h - 1 - y) as f64, x as f64)
                };
                (dev.abs() <= MAX_TILT + SPREAD).then_some((dev, u, v))
            })
            .collect();
        let found = hough(&pts, w.max(h), short);
        // A vertical leaning left at the top needs a clockwise turn: flip back to that.
        let sign = if family == 0 { 1.0 } else { -1.0 };
        lines.extend(found.into_iter().map(|(a, l, at)| (a * sign, l, at)));
    }
    let [horizontal, vertical] = families;
    if std::env::var_os("ARGUS_LEVEL_DEBUG").is_some() {
        eprintln!("level: horizontal {horizontal:?}\nlevel: vertical {vertical:?}");
    }
    let total = |l: &[(f64, f64, f64)]| l.iter().map(|x| x.1).sum::<f64>();
    // Both edges of one post are one object: count things, not edges.
    let mut xs: Vec<f64> = vertical.iter().map(|l| l.2).collect();
    xs.sort_by(f64::total_cmp);
    let objects = xs
        .windows(2)
        .filter(|p| p[1] - p[0] > w as f64 * 0.04)
        .count()
        + 1;
    // ...and they must stand on both sides of the middle, so the fit below reads the
    // roll between them instead of guessing it past the edge of a bunch on one side.
    let side = |left: bool| {
        vertical
            .iter()
            .filter(|l| (l.2 < w as f64 / 2.0) == left)
            .map(|l| l.1)
            .sum::<f64>()
    };
    let balanced = side(true).min(side(false)) >= total(&vertical) * g.balance;
    let tilt =
        if objects as f64 >= g.objects as f64 && balanced && total(&vertical) >= short * g.reach {
            // A camera tipped up or down makes verticals converge: each one leans by the roll
            // plus an amount that grows with its distance from the middle. Fit that line and
            // keep its value in the middle, the roll.
            match roll(&vertical, (w - 1) as f64 / 2.0, w as f64, g.misfit) {
                Some(t) => t,
                None => return Level::Unsure,
            }
        } else {
            // A horizon: one line across most of the frame, and no other long one disagreeing.
            let Some(&(angle, len, _)) = horizontal.iter().max_by(|a, b| a.1.total_cmp(&b.1))
            else {
                return Level::Unsure;
            };
            let disagree = horizontal
                .iter()
                .any(|l| l.1 >= w as f64 * 0.3 && (l.0 - angle).abs() > 1.0);
            if len < w as f64 * g.horizon || disagree {
                return Level::Unsure;
            }
            angle
        };
    if tilt.abs() > g.max_tilt {
        // A lean this big is more likely a choice, or a misread, than a slip.
        return Level::Unsure;
    }
    if tilt.abs() < LEVEL {
        Level::Level
    } else {
        Level::Turn((tilt * 10.0).round() / 10.0)
    }
}

/// The roll from verticals (angle, length, x): a length-weighted straight-line fit of
/// angle against x, read at the middle `cx`. None when the lines don't fit it.
fn roll(lines: &[(f64, f64, f64)], cx: f64, width: f64, mf: f64) -> Option<f64> {
    let wsum: f64 = lines.iter().map(|l| l.1).sum();
    let mx = lines.iter().map(|l| l.1 * l.2).sum::<f64>() / wsum;
    let ma = lines.iter().map(|l| l.1 * l.0).sum::<f64>() / wsum;
    let sxx: f64 = lines.iter().map(|l| l.1 * (l.2 - mx).powi(2)).sum();
    let sxa: f64 = lines.iter().map(|l| l.1 * (l.2 - mx) * (l.0 - ma)).sum();
    // Lines bunched in one place can't tell roll from convergence: assume none.
    let slope = if (sxx / wsum).sqrt() >= width * 0.12 {
        sxa / sxx
    } else {
        0.0
    };
    let at = |x: f64| ma + slope * (x - mx);
    let misfit = weighted_median(
        &lines
            .iter()
            .map(|l| ((l.0 - at(l.2)).abs(), l.1))
            .collect::<Vec<_>>(),
    );
    (misfit <= mf).then(|| at(cx))
}

fn fold(a: f64) -> f64 {
    let a = a.rem_euclid(180.0);
    if a > 90.0 {
        a - 180.0
    } else {
        a
    }
}

/// Long straight lines through `pts` ((own angle, u, v), v up), within `MAX_TILT` of the
/// u axis: (angle, length). A line is a Hough peak; nearby cells are cleared before the
/// next one, so one line counts once.
fn hough(pts: &[(f64, f64, f64)], span: usize, short: f64) -> Vec<(f64, f64, f64)> {
    let n = (2.0 * MAX_TILT / BIN).round() as usize + 1;
    let rhos = 2 * span + 1;
    let mut acc = vec![0u32; n * rhos];
    let trig: Vec<(f64, f64)> = (0..n)
        .map(|i| (i as f64 * BIN - MAX_TILT).to_radians().sin_cos())
        .collect();
    let reach = (SPREAD / BIN).round() as i64;
    for &(dev, u, v) in pts {
        let c = ((dev + MAX_TILT) / BIN).round() as i64;
        for i in (c - reach).max(0)..=(c + reach).min(n as i64 - 1) {
            let (s, co) = trig[i as usize];
            // Distance of the point from a line through the origin at this angle.
            let rho = (v * co - u * s).round() as i64 + span as i64;
            if (0..rhos as i64).contains(&rho) {
                acc[i as usize * rhos + rho as usize] += 1;
            }
        }
    }
    let min_len = (short * 0.15).max(40.0);
    let mut lines = Vec::new();
    for _ in 0..24 {
        let (best, &count) = acc
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| **c)
            .expect("cells");
        if (count as f64) < min_len {
            break;
        }
        let (ti, ri) = (best / rhos, best % rhos);
        let angle = ti as f64 * BIN - MAX_TILT;
        let (len, fit, at) = extent(pts, angle, ri as f64 - span as f64);
        if angle.abs() <= MAX_TILT - BIN && len >= min_len {
            lines.push((fit, len, at));
        }
        for t in ti.saturating_sub(15)..(ti + 16).min(n) {
            for r in ri.saturating_sub(12)..(ri + 13).min(rhos) {
                acc[t * rhos + r] = 0;
            }
        }
    }
    lines
}

/// How long the line at `angle`, `rho` really is (the longest run of edge points along it
/// with no gap over 8 pixels), its angle refined by a least-squares fit of that run, and
/// where it sits across the frame (its mean v).
fn extent(pts: &[(f64, f64, f64)], angle: f64, rho: f64) -> (f64, f64, f64) {
    let (s, c) = angle.to_radians().sin_cos();
    let mut near: Vec<(f64, f64, f64)> = pts
        .iter()
        .filter(|&&(dev, u, v)| (dev - angle).abs() <= SPREAD && (v * c - u * s - rho).abs() <= 2.0)
        .map(|&(_, u, v)| (u * c + v * s, u, v))
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut best, mut run, mut start) = (0.0f64, (0, 0), 0);
    for i in 1..near.len() {
        if near[i].0 - near[i - 1].0 > 8.0 {
            start = i;
        }
        if near[i].0 - near[start].0 > best {
            best = near[i].0 - near[start].0;
            run = (start, i + 1);
        }
    }
    let pts = &near[run.0..run.1];
    let k = pts.len() as f64;
    if k < 3.0 {
        return (best, angle, rho);
    }
    let (mu, mv) = (
        pts.iter().map(|p| p.1).sum::<f64>() / k,
        pts.iter().map(|p| p.2).sum::<f64>() / k,
    );
    let (mut suv, mut suu) = (0.0, 0.0);
    for p in pts {
        suv += (p.1 - mu) * (p.2 - mv);
        suu += (p.1 - mu) * (p.1 - mu);
    }
    let fit = (suv / suu).atan().to_degrees();
    (
        best,
        if (fit - angle).abs() <= 1.0 {
            fit
        } else {
            angle
        },
        mv,
    )
}

/// The value with half the weight on each side.
fn weighted_median(v: &[(f64, f64)]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let half = v.iter().map(|x| x.1).sum::<f64>() / 2.0;
    let mut run = 0.0;
    for (x, w) in &v {
        run += w;
        if run >= half {
            return *x;
        }
    }
    v.last().map_or(0.0, |x| x.0)
}

/// A 5-tap binomial blur (σ about 1 px), rows then columns, edges clamped.
fn blur(v: &[f32], w: usize, h: usize) -> Vec<f32> {
    const K: [f32; 5] = [1.0, 4.0, 6.0, 4.0, 1.0];
    let tap = |get: &dyn Fn(i64) -> f32, i: i64| -> f32 {
        K.iter()
            .enumerate()
            .map(|(k, c)| c * get(i + k as i64 - 2))
            .sum::<f32>()
            / 16.0
    };
    let mut rows = vec![0.0; v.len()];
    for y in 0..h {
        let line = &v[y * w..(y + 1) * w];
        for x in 0..w {
            rows[y * w + x] = tap(&|i| line[i.clamp(0, w as i64 - 1) as usize], x as i64);
        }
    }
    let mut out = vec![0.0; v.len()];
    for x in 0..w {
        for y in 0..h {
            out[y * w + x] = tap(
                &|i| rows[i.clamp(0, h as i64 - 1) as usize * w + x],
                y as i64,
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_plain(img: &RgbImage) -> Level {
        find(img, None)
    }

    /// A grey sky over darker ground, the horizon rising `tilt` degrees to the right, with
    /// some noise so it isn't a perfect drawing.
    fn horizon(tilt: f64) -> RgbImage {
        let (w, h) = (640usize, 427usize);
        let k = tilt.to_radians().tan();
        let mut data = Vec::with_capacity(w * h * 3);
        let mut seed = 7u32;
        for y in 0..h {
            for x in 0..w {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 27) as f64 - 16.0;
                // y grows down, so the horizon's y falls as x grows when it rises.
                let line = h as f64 * 0.55 - (x as f64 - w as f64 / 2.0) * k;
                let d = (y as f64 - line).clamp(-0.5, 0.5) + 0.5; // anti-aliased edge
                let v = (190.0 * (1.0 - d) + 70.0 * d + noise).clamp(0.0, 255.0) as u8;
                data.extend([v, v, v]);
            }
        }
        RgbImage {
            width: w,
            height: h,
            data,
        }
    }

    #[test]
    fn finds_a_tilted_horizon_both_ways() {
        for tilt in [3.0, -2.0, 5.5] {
            match find_plain(&horizon(tilt)) {
                Level::Turn(a) => assert!((a - tilt).abs() <= 0.3, "{tilt}: found {a}"),
                other => panic!("{tilt}: {other:?}"),
            }
        }
        // Past the gate's max tilt a lean is a choice, not a slip.
        assert_eq!(find_plain(&horizon(7.5)), Level::Unsure);
    }

    /// Plain wall with dark posts leaning `tilt` degrees left at the top (a camera turned
    /// clockwise by that much), like a room shot slightly crooked.
    fn posts(tilt: f64) -> RgbImage {
        let (w, h) = (640usize, 427usize);
        let k = tilt.to_radians().tan();
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                // Going up (y falling) a left-leaning post moves left.
                let shift = (h as f64 / 2.0 - y as f64) * k;
                let xx = x as f64 + shift;
                // How much of this pixel the nearest post covers, anti-aliased like a lens.
                let cover = [150.0, 330.0, 500.0]
                    .iter()
                    .map(|c: &f64| (12.5 - (xx - c).abs()).clamp(0.0, 1.0))
                    .fold(0.0, f64::max);
                let v = (200.0 - 140.0 * cover) as u8;
                data.extend([v, v, v]);
            }
        }
        RgbImage {
            width: w,
            height: h,
            data,
        }
    }

    #[test]
    fn finds_leaning_verticals() {
        for tilt in [2.5, -4.0] {
            match find_plain(&posts(tilt)) {
                Level::Turn(a) => assert!((a - tilt).abs() <= 0.3, "{tilt}: found {a}"),
                other => panic!("{tilt}: {other:?}"),
            }
        }
        let Level::Turn(a) = find_plain(&posts(3.0)) else {
            panic!()
        };
        let turned = crate::geometry::apply(
            posts(3.0),
            &crate::geometry::Crop {
                angle: a,
                ..Default::default()
            },
        );
        assert!(
            matches!(find_plain(&turned), Level::Level),
            "turning by {a} levels it"
        );
    }

    #[test]
    fn a_level_horizon_stays_and_noise_is_unsure() {
        assert_eq!(find_plain(&horizon(0.0)), Level::Level);
        let mut seed = 3u32;
        let data = (0..320 * 240 * 3)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 24) as u8
            })
            .collect();
        let noise = RgbImage {
            width: 320,
            height: 240,
            data,
        };
        assert_eq!(find_plain(&noise), Level::Unsure);
    }

    #[test]
    fn turning_by_the_answer_levels_it() {
        let img = horizon(4.0);
        let Level::Turn(a) = find_plain(&img) else {
            panic!("no tilt found")
        };
        let crop = crate::geometry::Crop {
            angle: a,
            ..Default::default()
        };
        let turned = crate::geometry::apply(img, &crop);
        assert!(
            matches!(find_plain(&turned), Level::Level | Level::Turn(_)),
            "still lines to read"
        );
        if let Level::Turn(left) = find_plain(&turned) {
            assert!(left.abs() <= 0.3, "left {left}° after turning {a}°");
        }
    }
}
