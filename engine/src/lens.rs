//! Lens corrections from the camera's own data, in place of a lens profile database
//! (lensfun is CC-BY-SA, which Argus can't ship). Sony writes the corrections its own
//! JPEG engine uses into every ARW: in the RAW image's IFD, tags 0x7032 (vignetting),
//! 0x7035 (lateral chromatic aberration, red then blue) and 0x7037 (distortion), each a
//! count followed by that many knots spread over the radius from the centre to a corner.
//! Tags 0x7031, 0x7034 and 0x7036 say whether the camera applied each one to its JPEG.
//! The layout follows ExifTool's public Sony tag tables; the scales below were fitted to
//! the camera's own JPEGs (docs/research/lens-corrections.md).
//!
//! The Recipe's `lensCorrections` (in `global`, Lightroom's Optics panel) picks what runs:
//! absent or "camera" does what the camera did to its JPEG, "on" every correction the file
//! carries (Lightroom's Enable Profile Corrections), "off" none. They run on the linear
//! demosaiced image before anything else, so Masks, crop and effects all see the
//! corrected frame.

use crate::recipe::Adjust;
use rayon::prelude::*;
use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Which corrections to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Which {
    pub distortion: bool,
    pub vignetting: bool,
    pub ca: bool,
}

impl Which {
    pub const ALL: Which = Which {
        distortion: true,
        vignetting: true,
        ca: true,
    };
    pub fn any(self) -> bool {
        self.distortion || self.vignetting || self.ca
    }
}

/// One RAW's corrections: per knot, the source radius factor (distortion; red and blue
/// for CA) and the share of the centre's light that reaches the sensor (vignetting).
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub distortion: Vec<f32>,
    pub vignetting: Vec<f32>,
    pub ca_red: Vec<f32>,
    pub ca_blue: Vec<f32>,
    /// What the camera applied to its own JPEG.
    pub camera: Which,
}

/// The Recipe's choice, from `global.lensCorrections`.
pub fn which(p: &Profile, a: &Adjust) -> Which {
    match a.other.get("lensCorrections").and_then(Value::as_str) {
        Some("on") => Which::ALL,
        Some("off") => Which::default(),
        _ => p.camera,
    }
}

const HEAD: usize = 1 << 20;

/// The corrections stored in the RAW at `path`, if it has them (Sony ARW today).
pub fn read(path: &Path) -> Option<Profile> {
    let mut buf = Vec::with_capacity(HEAD);
    std::fs::File::open(path)
        .ok()?
        .take(HEAD as u64)
        .read_to_end(&mut buf)
        .ok()?;
    parse(&buf)
}

/// Parse the corrections from the start of a TIFF-based RAW.
pub fn parse(b: &[u8]) -> Option<Profile> {
    let le = match b.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let s: [u8; 2] = b.get(o..o + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(s)
        } else {
            u16::from_be_bytes(s)
        })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let s: [u8; 4] = b.get(o..o + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(s)
        } else {
            u32::from_be_bytes(s)
        })
    };
    // Entries of the IFD at `off`: (tag, type, count, offset of the value).
    let entries = |off: usize| -> Option<Vec<(u16, u16, usize, usize)>> {
        let n = u16_at(off)? as usize;
        (0..n)
            .map(|i| {
                let e = off + 2 + 12 * i;
                let (tag, typ, count) = (u16_at(e)?, u16_at(e + 2)?, u32_at(e + 4)? as usize);
                let size = count * if matches!(typ, 3 | 8) { 2 } else { 4 };
                let at = if size <= 4 {
                    e + 8
                } else {
                    u32_at(e + 8)? as usize
                };
                Some((tag, typ, count, at))
            })
            .collect()
    };
    let ifd0 = entries(u32_at(4)? as usize)?;
    // The RAW image's IFD: IFD0's first SubIFD.
    let &(_, _, _, at) = ifd0.iter().find(|e| e.0 == 0x14a)?;
    let raw = entries(u32_at(at)? as usize)?;
    let tag = |t: u16| raw.iter().find(|e| e.0 == t).copied();
    let shorts = |t: u16| -> Option<Vec<i16>> {
        let (_, typ, count, at) = tag(t)?;
        if !matches!(typ, 3 | 8) {
            return None;
        }
        (0..count)
            .map(|i| u16_at(at + 2 * i).map(|v| v as i16))
            .collect()
    };
    let flag = |t: u16| tag(t).and_then(|(_, _, _, at)| u16_at(at));
    // A count, then the knots.
    let knots = |v: Vec<i16>, parts: usize| -> Option<Vec<f32>> {
        let n = *v.first()? as usize;
        (n >= 2 * parts && v.len() > n && n.is_multiple_of(parts))
            .then(|| v[1..=n].iter().map(|&x| x as f32).collect())
    };
    let distortion = knots(shorts(0x7037)?, 1)?;
    let vignetting = knots(shorts(0x7032)?, 1)?;
    let ca = knots(shorts(0x7035)?, 2)?;
    let n = ca.len() / 2;
    Some(Profile {
        // Source radius = r * factor.
        distortion: distortion.iter().map(|d| 1.0 + d / 16384.0).collect(),
        // The light reaching the sensor falls to 2^(0.5 - 2^(v / 8192 - 1)) of the centre's.
        vignetting: vignetting
            .iter()
            .map(|v| 2f32.powf(0.5 - 2f32.powf(v / 8192.0 - 1.0)))
            .collect(),
        ca_red: ca[..n].iter().map(|c| 1.0 + c / 2_097_152.0).collect(),
        ca_blue: ca[n..].iter().map(|c| 1.0 + c / 2_097_152.0).collect(),
        camera: Which {
            vignetting: flag(0x7031).is_some_and(|v| v & 0xff != 0),
            ca: flag(0x7034).is_some_and(|v| v != 0),
            distortion: flag(0x7036).is_some_and(|v| v & 1 == 1),
        },
    })
}

/// `knots` at radius `r` (0 = centre, 1 = corner). Knot i sits at (i + 1) / n, the best
/// fit to the camera's JPEGs; inside the first knot the curve holds its first value.
fn at(knots: &[f32], r: f32) -> f32 {
    let n = knots.len();
    let x = (r * n as f32 - 1.0).clamp(0.0, (n - 1) as f32);
    let i = (x as usize).min(n - 2);
    let t = x - i as f32;
    knots[i] + (knots[i + 1] - knots[i]) * t
}

/// Steps of the radius lookup tables.
const STEPS: usize = 1024;

/// `img` (`w * h * 3` linear RGB) with `which` corrections of `p`. Distortion zooms just
/// enough that no empty edge shows, as the camera's own correction does.
pub fn correct(img: &[f32], w: usize, h: usize, p: &Profile, which: Which) -> Vec<f32> {
    let warp = Warp::new(w, h, p, which);
    let mut out = vec![0f32; w * h * 3];
    out.par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y, row)| warp.row(img, y, row));
    out
}

/// The corrections for one image size, ready to run a row at a time (so a full-size
/// export can stream rows straight into colour and tone).
pub struct Warp {
    w: usize,
    h: usize,
    /// Source radius factor per channel, and the gain, over r in 0..=1.5.
    scale: [Vec<f32>; 3],
    gain: Vec<f32>,
    zoom: f32,
}

impl Warp {
    pub fn new(w: usize, h: usize, p: &Profile, which: Which) -> Self {
        let dist = |r: f32| {
            if which.distortion {
                at(&p.distortion, r)
            } else {
                1.0
            }
        };
        let ca = |k: &[f32], r: f32| if which.ca { at(k, r) } else { 1.0 };
        // Lookup tables over r in 0..=1.5 (beyond a corner only for safety).
        let table = |f: &dyn Fn(f32) -> f32| -> Vec<f32> {
            (0..=STEPS)
                .map(|i| f(1.5 * i as f32 / STEPS as f32))
                .collect()
        };
        let scale = [
            table(&|r| dist(r) * ca(&p.ca_red, r)),
            table(&|r| dist(r)),
            table(&|r| dist(r) * ca(&p.ca_blue, r)),
        ];
        let gain = table(&|r| {
            if which.vignetting {
                1.0 / at(&p.vignetting, r.min(1.0))
            } else {
                1.0
            }
        });
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let rn = (cx * cx + cy * cy).sqrt();
        // The largest zoom that keeps every sample of the frame's edge inside it.
        let mut zoom = 1.0f32;
        for k in 0..=64 {
            let t = k as f32 / 64.0;
            for (ex, ey) in [(t * cx, cy), (cx, t * cy)] {
                let r = (ex * ex + ey * ey).sqrt() / rn;
                for s in &scale {
                    let f = look(s, r);
                    if f > 0.0 {
                        zoom = zoom.min(1.0 / f);
                    }
                }
            }
        }
        Warp {
            w,
            h,
            scale,
            gain,
            zoom,
        }
    }

    /// Output row `y` (`w * 3` values) from the whole uncorrected image `img`.
    pub fn row(&self, img: &[f32], y: usize, out: &mut [f32]) {
        let (w, h) = (self.w, self.h);
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let rn = (cx * cx + cy * cy).sqrt();
        let dy = (y as f32 + 0.5 - cy) / rn;
        for x in 0..w {
            let dx = (x as f32 + 0.5 - cx) / rn;
            let r = (dx * dx + dy * dy).sqrt();
            let sg = self.zoom * look(&self.scale[1], r);
            // CA shifts are a fraction of a pixel, so one gain serves all three channels.
            let g = look(&self.gain, r * sg);
            for c in 0..3 {
                let s = if c == 1 {
                    sg
                } else {
                    self.zoom * look(&self.scale[c], r)
                };
                let (sx, sy) = (cx + dx * s * rn - 0.5, cy + dy * s * rn - 0.5);
                out[x * 3 + c] = sample(img, w, h, sx, sy, c) * g;
            }
        }
    }
}

fn look(t: &[f32], r: f32) -> f32 {
    let x = (r / 1.5 * STEPS as f32).clamp(0.0, STEPS as f32);
    let i = (x as usize).min(STEPS - 1);
    t[i] + (t[i + 1] - t[i]) * (x - i as f32)
}

fn sample(img: &[f32], w: usize, h: usize, x: f32, y: f32, c: usize) -> f32 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let v = |xx: usize, yy: usize| img[(yy * w + xx) * 3 + c];
    let a = v(x0, y0) + (v(x1, y0) - v(x0, y0)) * tx;
    let b = v(x0, y1) + (v(x1, y1) - v(x0, y1)) * tx;
    a + (b - a) * ty
}

/// A demosaiced image's corrections, and the corrected pixels of the last render, so
/// slider re-renders don't warp the image again.
#[derive(Default)]
pub struct Lens {
    pub profile: Option<Arc<Profile>>,
    done: Mutex<Option<(Which, Arc<Vec<f32>>)>>,
}

impl Lens {
    pub fn new(profile: Option<Arc<Profile>>) -> Self {
        Lens {
            profile,
            done: Mutex::new(None),
        }
    }

    /// The same corrections with nothing cached, for a copy of the image.
    pub fn fresh(&self) -> Self {
        Lens::new(self.profile.clone())
    }

    /// `rgb` corrected as `adjust` asks, or None when nothing applies.
    pub fn corrected(
        &self,
        rgb: &[f32],
        w: usize,
        h: usize,
        adjust: &Adjust,
    ) -> Option<Arc<Vec<f32>>> {
        let p = self.profile.as_ref()?;
        let which = which(p, adjust);
        if !which.any() {
            return None;
        }
        let mut done = self.done.lock().unwrap();
        if let Some((w0, img)) = done.as_ref() {
            if *w0 == which {
                return Some(img.clone());
            }
        }
        let img = Arc::new(correct(rgb, w, h, p, which));
        *done = Some((which, img.clone()));
        Some(img)
    }

    /// Heap size of the cached pixels, for caches.
    pub fn bytes(&self) -> usize {
        self.done
            .lock()
            .unwrap()
            .as_ref()
            .map_or(0, |(_, v)| v.len() * std::mem::size_of::<f32>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(d: f32, v: f32) -> Profile {
        Profile {
            distortion: (0..16)
                .map(|i| 1.0 + d * ((i + 1) as f32 / 16.0).powi(2))
                .collect(),
            vignetting: (0..16)
                .map(|i| 1.0 - v * ((i + 1) as f32 / 16.0).powi(2))
                .collect(),
            ca_red: vec![1.0; 16],
            ca_blue: vec![1.0; 16],
            camera: Which::ALL,
        }
    }

    #[test]
    fn vignetting_brightens_the_corners_and_leaves_the_centre() {
        let (w, h) = (64, 48);
        let img = vec![0.25f32; w * h * 3];
        let out = correct(&img, w, h, &profile(0.0, 0.3), Which::ALL);
        let px = |x: usize, y: usize| out[(y * w + x) * 3 + 1];
        assert!((px(32, 24) - 0.25).abs() < 0.01, "centre {}", px(32, 24));
        assert!(px(0, 0) > 0.33, "corner {}", px(0, 0));
    }

    #[test]
    fn barrel_correction_pulls_the_corners_in_and_never_shows_an_empty_edge() {
        let (w, h) = (64, 48);
        // A horizontal ramp: distortion moves it, so the corners change.
        let img: Vec<f32> = (0..w * h * 3)
            .map(|i| ((i / 3) % w) as f32 / w as f32)
            .collect();
        let barrel = correct(&img, w, h, &profile(-0.08, 0.0), Which::ALL);
        let v = |o: &[f32], x: usize, y: usize| o[(y * w + x) * 3];
        // The corner samples from closer to the middle, so a darker point of the ramp.
        assert!(v(&barrel, w - 1, 0) < v(&img, w - 1, 0));
        let pin = correct(&img, w, h, &profile(0.08, 0.0), Which::ALL);
        // Pincushion samples outward and zooms so nothing is clamped at the corner.
        assert!(v(&pin, w - 1, h - 1) <= v(&img, w - 1, h - 1) + 1e-3);
        assert!(v(&pin, 0, h / 2) >= 0.0);
    }

    #[test]
    fn the_recipe_picks_camera_all_or_none() {
        let mut p = profile(0.0, 0.0);
        p.camera = Which {
            distortion: false,
            vignetting: true,
            ca: true,
        };
        let a = |v: Value| {
            let mut a = Adjust::default();
            a.other.insert("lensCorrections".into(), v);
            a
        };
        assert_eq!(which(&p, &Adjust::default()), p.camera);
        assert_eq!(which(&p, &a("on".into())), Which::ALL);
        assert_eq!(which(&p, &a("off".into())), Which::default());
    }
}
