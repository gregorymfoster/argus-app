//! Cheap per-Photo signals measured from a preview: how sharp, how bright, how much is
//! clipped. They are hints for the Provider's judgment (spec, ML models), not a
//! replacement for it: a dark, moody frame or a shallow-focus portrait can still win.

use image::RgbImage;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, Serialize, serde::Deserialize, PartialEq)]
pub struct Signals {
    /// Focus energy of the sharpest part of the frame (Laplacian variance of the top 5%
    /// of tiles). Only meaningful compared with other Photos at the same preview size.
    pub sharpness: f32,
    /// Mean luma, 0 (black) to 1 (white).
    pub brightness: f32,
    /// Share of pixels blown to white, and crushed to black, 0 to 1.
    pub clipped: f32,
    pub crushed: f32,
}

const TILE: u32 = 16;

/// Measure a preview. Fast: one pass over a ~768px image.
pub fn measure(img: &RgbImage) -> Signals {
    let (w, h) = img.dimensions();
    if w < 3 || h < 3 {
        return Signals::default();
    }
    let luma: Vec<f32> = img
        .pixels()
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0)
        .collect();
    let n = luma.len() as f32;
    let brightness = luma.iter().sum::<f32>() / n;
    let clipped = img
        .pixels()
        .filter(|p| p.0.iter().any(|&c| c >= 253))
        .count() as f32
        / n;
    let crushed = luma.iter().filter(|&&l| l <= 0.02).count() as f32 / n;

    // Laplacian energy per tile; the subject is usually a small part of the frame, so the
    // sharpest tiles say more about focus than the frame's average does.
    let (tw, th) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut sum = vec![0f32; (tw * th) as usize];
    let mut count = vec![0u32; (tw * th) as usize];
    let at = |x: u32, y: u32| luma[(y * w + x) as usize];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let lap = 4.0 * at(x, y) - at(x - 1, y) - at(x + 1, y) - at(x, y - 1) - at(x, y + 1);
            let t = ((y / TILE) * tw + x / TILE) as usize;
            sum[t] += lap * lap;
            count[t] += 1;
        }
    }
    let mut tiles: Vec<f32> = sum
        .iter()
        .zip(&count)
        .filter(|(_, &c)| c > 0)
        .map(|(s, &c)| s / c as f32)
        .collect();
    tiles.sort_by(|a, b| b.total_cmp(a));
    let top = (tiles.len() / 20).max(1);
    let sharpness = tiles[..top].iter().sum::<f32>() / top as f32 * 1000.0;
    Signals {
        sharpness,
        brightness,
        clipped,
        crushed,
    }
}

/// Each value's percentile (0-100) among `values`: 100 = the highest of them.
pub fn percentiles(values: &[f32]) -> Vec<u8> {
    let n = values.len();
    if n < 2 {
        return vec![50; n];
    }
    values
        .iter()
        .map(|v| {
            let below = values.iter().filter(|o| *o < v).count();
            (below * 100 / (n - 1)) as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(blur: bool) -> RgbImage {
        RgbImage::from_fn(128, 96, |x, y| {
            let v = if blur {
                ((x as f32 / 128.0) * 255.0) as u8
            } else if (x / 4 + y / 4) % 2 == 0 {
                230
            } else {
                20
            };
            image::Rgb([v, v, v])
        })
    }

    #[test]
    fn sharp_detail_beats_a_smooth_gradient() {
        let sharp = measure(&checker(false));
        let soft = measure(&checker(true));
        assert!(
            sharp.sharpness > soft.sharpness * 10.0,
            "{sharp:?} {soft:?}"
        );
    }

    #[test]
    fn brightness_and_clipping() {
        let white = measure(&RgbImage::from_pixel(64, 64, image::Rgb([255, 255, 255])));
        assert!((white.brightness - 1.0).abs() < 1e-3);
        assert_eq!(white.clipped, 1.0);
        let black = measure(&RgbImage::from_pixel(64, 64, image::Rgb([0, 0, 0])));
        assert_eq!(black.crushed, 1.0);
        assert_eq!(black.clipped, 0.0);
    }

    #[test]
    fn percentiles_rank_within_the_set() {
        assert_eq!(percentiles(&[3.0, 1.0, 2.0]), vec![100, 0, 50]);
        assert_eq!(percentiles(&[5.0]), vec![50]);
    }
}

/// Where a render's light sits, as numbers a model can't misjudge by eye: a vision model
/// sees a dark frame as "fine" or a bright one as "blown" unreliably, so every render the
/// Agent and the Refine loop look at comes with these.
#[derive(Debug, Clone, Copy, Default, Serialize, serde::Deserialize, PartialEq)]
pub struct Light {
    /// Mean, 1st percentile, median and 99th percentile of sRGB luma, 0 (black) to 100.
    pub mean: f32,
    pub p1: f32,
    pub median: f32,
    pub p99: f32,
    /// Percent of pixels with a channel blown to white, and with luma crushed to black.
    pub clipped_pct: f32,
    pub crushed_pct: f32,
    /// Percent of pixels in each fifth of the luma range, darkest first.
    pub zones: [f32; 5],
    /// Colour cast of the mid-tones in stops: + warm (red over blue), + magenta (green
    /// below red and blue). Near 0 for a neutral grey.
    pub warm: f32,
    pub magenta: f32,
}

impl Light {
    /// One line for a prompt: "mean 42, p1 3 ...".
    pub fn line(&self) -> String {
        let z = self.zones.map(|v| format!("{v:.0}")).join("/");
        format!(
            "luma mean {:.0}, p1 {:.0}, median {:.0}, p99 {:.0} (0-100); clipped {:.1}%, crushed {:.1}%; \
             zones dark->bright {z}%; mid-tone cast warm {:+.2}, magenta {:+.2} stops",
            self.mean, self.p1, self.median, self.p99, self.clipped_pct, self.crushed_pct, self.warm, self.magenta
        )
    }
}

/// Measure a render's [`Light`]. One pass over the pixels.
pub fn light(img: &crate::RgbImage) -> Light {
    let px = img.data.chunks_exact(3);
    let n = px.len();
    if n == 0 {
        return Light::default();
    }
    let mut hist = [0u32; 256];
    let (mut clipped, mut sum) = (0u32, 0f64);
    let mut mid = [0f64; 3];
    let mut mids = 0u32;
    for p in px {
        let l =
            (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as usize;
        hist[l.min(255)] += 1;
        sum += l as f64;
        if p.iter().any(|&c| c >= 253) {
            clipped += 1;
        }
        if (64..=192).contains(&l) {
            for c in 0..3 {
                mid[c] += p[c] as f64;
            }
            mids += 1;
        }
    }
    let pct = |q: f64| {
        let want = (q * n as f64) as u32;
        let mut acc = 0;
        for (i, &h) in hist.iter().enumerate() {
            acc += h;
            if acc > want {
                return i as f32 / 2.55;
            }
        }
        100.0
    };
    let mut zones = [0f32; 5];
    for (i, &h) in hist.iter().enumerate() {
        zones[(i * 5 / 256).min(4)] += h as f32;
    }
    let (warm, magenta) = if mids > 0 {
        let [r, g, b] = mid.map(|v| v.max(1.0));
        ((r / b).log2() as f32, (((r + b) / 2.0) / g).log2() as f32)
    } else {
        (0.0, 0.0)
    };
    Light {
        mean: (sum / n as f64 / 2.55) as f32,
        p1: pct(0.01),
        median: pct(0.5),
        p99: pct(0.99),
        clipped_pct: clipped as f32 * 100.0 / n as f32,
        crushed_pct: hist[..=5].iter().sum::<u32>() as f32 * 100.0 / n as f32,
        zones: zones.map(|z| z * 100.0 / n as f32),
        warm,
        magenta,
    }
}
