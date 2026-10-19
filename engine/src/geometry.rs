//! Crop and straighten, applied after colour and tone (as in Lightroom, where geometry
//! never changes how a pixel is developed).
//!
//! The crop rectangle is normalised (0..1) to the largest upright rectangle, with the
//! Photo's own aspect ratio, that fits inside the straightened image. So any crop in
//! range is valid and a straightened Photo never shows empty corners: the same promise
//! as Lightroom's "Constrain to image".

use crate::RgbImage;
use serde::{Deserialize, Serialize};

/// A Recipe's crop and straighten. The default is the whole, unrotated Photo.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Crop {
    /// Straighten, in degrees, -45..45; positive turns the picture clockwise.
    pub angle: f64,
    /// Left, top, width and height, 0..1 of the straightened frame.
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Default for Crop {
    fn default() -> Self {
        Crop {
            angle: 0.0,
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }
}

impl Crop {
    pub fn is_identity(&self) -> bool {
        *self == Crop::default()
    }

    /// Clamp into range: angle to ±45°, the rectangle inside 0..1 and at least 5% a side.
    pub fn clamped(self) -> Crop {
        let f = |v: f64, d: f64| if v.is_finite() { v } else { d };
        let angle = f(self.angle, 0.0).clamp(-45.0, 45.0);
        let w = f(self.w, 1.0).clamp(0.05, 1.0);
        let h = f(self.h, 1.0).clamp(0.05, 1.0);
        let x = f(self.x, 0.0).clamp(0.0, 1.0 - w);
        let y = f(self.y, 0.0).clamp(0.0, 1.0 - h);
        Crop { angle, x, y, w, h }
    }
}

/// Scale of the largest upright rectangle with the image's aspect ratio that fits inside
/// the image turned by `angle` degrees.
fn inscribed_scale(w: f64, h: f64, angle: f64) -> f64 {
    let (s, c) = angle.to_radians().abs().sin_cos();
    (w / (w * c + h * s)).min(h / (w * s + h * c))
}

/// Straighten and crop `img`. The identity Crop returns it unchanged.
pub fn apply(img: RgbImage, crop: &Crop) -> RgbImage {
    let crop = crop.clamped();
    if crop.is_identity() {
        return img;
    }
    let (w, h) = (img.width as f64, img.height as f64);
    let k = inscribed_scale(w, h, crop.angle);
    let (iw, ih) = (w * k, h * k); // the valid frame, centred on the image
    let (ox, oy) = ((w - iw) / 2.0 + crop.x * iw, (h - ih) / 2.0 + crop.y * ih);
    let out_w = ((crop.w * iw).round() as usize).max(1);
    let out_h = ((crop.h * ih).round() as usize).max(1);
    let (sin, cos) = (-crop.angle).to_radians().sin_cos();
    let (cx, cy) = (w / 2.0, h / 2.0);
    let mut data = vec![0u8; out_w * out_h * 3];
    use rayon::prelude::*;
    data.par_chunks_mut(out_w * 3)
        .enumerate()
        .for_each(|(v, row)| {
            for u in 0..out_w {
                // Point in the straightened frame, turned back into the original.
                let (px, py) = (ox + u as f64 + 0.5 - cx, oy + v as f64 + 0.5 - cy);
                let sx = px * cos - py * sin + cx - 0.5;
                let sy = px * sin + py * cos + cy - 0.5;
                let rgb = bilinear(&img, sx, sy);
                row[u * 3..u * 3 + 3].copy_from_slice(&rgb);
            }
        });
    RgbImage {
        width: out_w,
        height: out_h,
        data,
    }
}

fn bilinear(img: &RgbImage, x: f64, y: f64) -> [u8; 3] {
    let x = x.clamp(0.0, (img.width - 1) as f64);
    let y = y.clamp(0.0, (img.height - 1) as f64);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(img.width - 1), (y0 + 1).min(img.height - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let px = |x: usize, y: usize, c: usize| img.data[(y * img.width + x) * 3 + c] as f64;
    let mut out = [0u8; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
        let bot = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
        *o = (top * (1.0 - fy) + bot * fy).round() as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: usize, h: usize) -> RgbImage {
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                data.extend([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 128]);
            }
        }
        RgbImage {
            width: w,
            height: h,
            data,
        }
    }

    #[test]
    fn identity_is_untouched_and_a_crop_takes_that_part() {
        let img = gradient(200, 100);
        let same = apply(img.clone(), &Crop::default());
        assert_eq!(same.data, img.data);
        let right = apply(
            img,
            &Crop {
                x: 0.5,
                w: 0.5,
                ..Default::default()
            },
        );
        assert_eq!((right.width, right.height), (100, 100));
        assert!(
            right.data[0] >= 126,
            "starts at the middle: red {}",
            right.data[0]
        );
    }

    #[test]
    fn straightening_never_shows_empty_corners() {
        let mut img = gradient(300, 200);
        for p in img.data.chunks_mut(3) {
            p[2] = 200; // blue everywhere, so a corner sampled from outside would differ
        }
        let out = apply(
            img,
            &Crop {
                angle: 10.0,
                ..Default::default()
            },
        );
        assert!(out.width < 300 && out.width > 200, "{}", out.width);
        assert!(
            (out.width as f64 / out.height as f64 - 1.5).abs() < 0.02,
            "keeps the aspect ratio"
        );
        for (x, y) in [
            (0, 0),
            (out.width - 1, 0),
            (0, out.height - 1),
            (out.width - 1, out.height - 1),
        ] {
            assert_eq!(out.data[(y * out.width + x) * 3 + 2], 200);
        }
    }

    #[test]
    fn clamping_keeps_crops_in_range() {
        let c = Crop {
            angle: 90.0,
            x: 0.9,
            y: -1.0,
            w: 0.5,
            h: 2.0,
        }
        .clamped();
        assert_eq!(
            c,
            Crop {
                angle: 45.0,
                x: 0.5,
                y: 0.0,
                w: 0.5,
                h: 1.0
            }
        );
    }
}
