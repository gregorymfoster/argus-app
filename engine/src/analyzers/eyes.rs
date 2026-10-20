//! Eyes open and face focus, per face: MediaPipe's face landmarker (478 points) and its
//! blendshapes model (Apache 2.0), run on a rotated crop around each YuNet face, plus the
//! focus energy around the eyes compared with the sharpest part of the frame.
//!
//! The crop follows MediaPipe's own image pipeline: a square 1.5x the face, turned so the
//! eyes are level, then a second pass on the box the first landmarks give. Eyes count as
//! closed when both eyes' blink score passes [`CLOSED`]; squinting into a laugh or looking
//! down scores about 0.5-0.7, a real blink 0.9.

use super::runtime::{floats, ndarray, Lazy, Tensor};
use super::{Analyzer, Cost, Fact, Input};
use crate::masks::Face;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

const LANDMARKS: &[u8] = include_bytes!("../../models/face_landmarks_mediapipe.onnx");
const BLENDSHAPES: &[u8] = include_bytes!("../../models/face_blendshapes_mediapipe.onnx");
const CROP: usize = 256;
static LANDMARK_MODEL: Lazy = Lazy::new(LANDMARKS, &[1, CROP, CROP, 3]);
static BLENDSHAPE_MODEL: Lazy = Lazy::new(BLENDSHAPES, &[1, 146, 2]);

/// Blink score (both eyes) above which a face's eyes count as closed.
pub const CLOSED: f32 = 0.6;
/// Faces narrower than this (pixels of the preview) are too small to read eyes on.
const MIN_FACE_PX: f32 = 40.0;
/// The landmarker's face-presence score below which a YuNet face is left unread.
const MIN_PRESENCE: f32 = 0.5;

/// Landmarks the blendshapes model reads (MediaPipe's `kLandmarksSubsetIdxs`).
const SUBSET: [usize; 146] = [
    0, 1, 4, 5, 6, 7, 8, 10, 13, 14, 17, 21, 33, 37, 39, 40, 46, 52, 53, 54, 55, 58, 61, 63, 65,
    66, 67, 70, 78, 80, 81, 82, 84, 87, 88, 91, 93, 95, 103, 105, 107, 109, 127, 132, 133, 136,
    144, 145, 146, 148, 149, 150, 152, 153, 154, 155, 157, 158, 159, 160, 161, 162, 163, 168, 172,
    173, 176, 178, 181, 185, 191, 195, 197, 234, 246, 249, 251, 263, 267, 269, 270, 276, 282, 283,
    284, 285, 288, 291, 293, 295, 296, 297, 300, 308, 310, 311, 312, 314, 317, 318, 321, 323, 324,
    332, 334, 336, 338, 356, 361, 362, 365, 373, 374, 375, 377, 378, 379, 380, 381, 382, 384, 385,
    386, 387, 388, 389, 390, 397, 398, 400, 402, 405, 409, 415, 454, 466, 468, 469, 470, 471, 472,
    473, 474, 475, 476, 477,
];
const BLINK_LEFT: usize = 9;
const BLINK_RIGHT: usize = 10;
/// Eye corners, lids and brows of both eyes: the region whose focus counts.
const EYE_REGION: [usize; 12] = [33, 133, 159, 145, 263, 362, 386, 374, 70, 105, 300, 334];

/// What was read on one face (same order as the `faces` Fact).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FaceRead {
    pub face: usize,
    /// False when the face was too small or the landmarker didn't see a face there.
    pub read: bool,
    /// Blink score per eye as seen in the image, `[left, right]`, 0 open to 1 shut.
    pub blink: [f32; 2],
    pub eyes_closed: bool,
    /// Focus energy around the eyes over the frame's sharpest tiles: about 1 means the
    /// eyes are as sharp as anything in the frame, well under 0.3 means they're soft.
    pub focus: f32,
}

pub struct Eyes;

impl Analyzer for Eyes {
    fn id(&self) -> &'static str {
        "eyes"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn needs(&self) -> &'static [&'static str] {
        &["faces"]
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let faces = super::faces::read(input.fact("faces")?);
        let img = input.preview;
        let luma = luma(img);
        let frame = frame_focus(&luma, img.width() as usize, img.height() as usize);
        let mut reads = Vec::new();
        let mut blob = Vec::new();
        for (i, f) in faces.iter().enumerate() {
            match read_face(img, f)? {
                Some((blink, points)) => {
                    let focus = eye_focus(&luma, img, &points) / frame.max(1e-6);
                    reads.push(FaceRead {
                        face: i,
                        read: true,
                        blink,
                        eyes_closed: blink[0] > CLOSED && blink[1] > CLOSED,
                        focus: (focus * 100.0).round() / 100.0,
                    });
                    let (w, h) = (img.width() as f32, img.height() as f32);
                    for p in &points {
                        for v in [p[0] / w, p[1] / h] {
                            blob.extend(((v.clamp(0.0, 1.0) * 65535.0) as u16).to_le_bytes());
                        }
                    }
                }
                None => reads.push(FaceRead {
                    face: i,
                    read: false,
                    blink: [0.0; 2],
                    eyes_closed: false,
                    focus: 0.0,
                }),
            }
        }
        Ok(Fact {
            value: json!({ "faces": reads }),
            blob: Some(blob),
        })
    }
    fn caption(&self, fact: &Fact) -> Option<String> {
        let reads: Vec<FaceRead> = read(fact).into_iter().filter(|r| r.read).collect();
        if reads.is_empty() {
            return None;
        }
        let closed = reads.iter().filter(|r| r.eyes_closed).count();
        let soft = reads.iter().filter(|r| r.focus < SOFT).count();
        let n = reads.len();
        let eyes = match (closed, n) {
            (0, 1) => "eyes open".to_string(),
            (0, _) => "all eyes open".to_string(),
            (1, 1) => "eyes closed".to_string(),
            (c, n) => format!("eyes closed on {c} of {n} faces"),
        };
        let focus = match (soft, n) {
            (0, _) => "faces sharp",
            (s, n) if s == n => "faces soft",
            _ => "some faces soft",
        };
        Some(format!("{eyes}, {focus}"))
    }
}

/// Eye focus under which a face reads as soft.
pub const SOFT: f32 = 0.25;

/// The per-face reads in a Fact this Analyzer made.
pub fn read(fact: &Fact) -> Vec<FaceRead> {
    serde_json::from_value(fact.value["faces"].clone()).unwrap_or_default()
}

/// The 478 landmarks of each read face (normalised 0..1 of the image), in the order of
/// the read faces.
pub fn landmarks(fact: &Fact) -> Vec<Vec<[f32; 2]>> {
    let Some(b) = &fact.blob else {
        return Vec::new();
    };
    let per = 478 * 4;
    b.chunks_exact(per)
        .map(|face| {
            face.as_chunks::<4>()
                .0
                .iter()
                .map(|p| {
                    let x = u16::from_le_bytes([p[0], p[1]]) as f32 / 65535.0;
                    let y = u16::from_le_bytes([p[2], p[3]]) as f32 / 65535.0;
                    [x, y]
                })
                .collect()
        })
        .collect()
}

/// A square region of interest in image pixels, turned by `angle` (radians).
#[derive(Debug, Clone, Copy)]
struct Roi {
    cx: f32,
    cy: f32,
    side: f32,
    angle: f32,
}

/// Blink scores per eye and the landmarks of one face, in image pixels.
type Reading = ([f32; 2], Vec<[f32; 2]>);

/// One face's [`Reading`], or None if it can't be read.
fn read_face(img: &image::RgbImage, f: &Face) -> Result<Option<Reading>> {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let side = (f.w * w).max(f.h * h);
    if side < MIN_FACE_PX {
        return Ok(None);
    }
    let [l, r] = f.eyes;
    let first = Roi {
        cx: (f.x + f.w / 2.0) * w,
        cy: (f.y + f.h / 2.0) * h,
        side: side * 1.5,
        angle: ((r[1] - l[1]) * h).atan2((r[0] - l[0]) * w),
    };
    let Some(points) = landmark(img, first)? else {
        return Ok(None);
    };
    let Some(points) = landmark(img, roi_from(&points))? else {
        return Ok(None);
    };
    let subset: Vec<f32> = SUBSET
        .iter()
        .flat_map(|&i| [points[i][0], points[i][1]])
        .collect();
    let input: Tensor = ndarray::Array3::from_shape_vec((1, 146, 2), subset)
        .map_err(|e| Error(format!("blendshapes input: {e}")))?
        .into();
    let out = BLENDSHAPE_MODEL.run(input)?;
    let scores = floats(&out[0])?;
    // The model names eyes from the subject's side: their left eye is on the image's right.
    let blink = [scores[BLINK_RIGHT], scores[BLINK_LEFT]].map(|v| (v * 1000.0).round() / 1000.0);
    Ok(Some((blink, points)))
}

/// Run the landmarker on one crop. Landmarks in image pixels, or None if no face there.
fn landmark(img: &image::RgbImage, roi: Roi) -> Result<Option<Vec<[f32; 2]>>> {
    let (ca, sa) = (roi.angle.cos(), roi.angle.sin());
    let to_image = |u: f32, v: f32| {
        // u, v: crop pixels; centred and scaled to image pixels, then turned.
        let (du, dv) = (
            (u / CROP as f32 - 0.5) * roi.side,
            (v / CROP as f32 - 0.5) * roi.side,
        );
        [roi.cx + du * ca - dv * sa, roi.cy + du * sa + dv * ca]
    };
    let mut data = Vec::with_capacity(CROP * CROP * 3);
    for y in 0..CROP {
        for x in 0..CROP {
            let [ix, iy] = to_image(x as f32 + 0.5, y as f32 + 0.5);
            data.extend(bilinear(img, ix, iy));
        }
    }
    let input: Tensor = ndarray::Array4::from_shape_vec((1, CROP, CROP, 3), data)
        .map_err(|e| Error(format!("landmarker input: {e}")))?
        .into();
    let out = LANDMARK_MODEL.run(input)?;
    let presence = 1.0 / (1.0 + (-floats(&out[1])?[0]).exp());
    if presence < MIN_PRESENCE {
        return Ok(None);
    }
    let p = floats(&out[0])?;
    Ok(Some(
        (0..478).map(|i| to_image(p[i * 3], p[i * 3 + 1])).collect(),
    ))
}

/// MediaPipe's landmarks-to-ROI: level the eye corners, box every landmark, 1.5x square.
fn roi_from(points: &[[f32; 2]]) -> Roi {
    let (a, b) = (points[33], points[263]);
    let angle = (b[1] - a[1]).atan2(b[0] - a[0]);
    let (ca, sa) = (angle.cos(), angle.sin());
    let n = points.len() as f32;
    let (mx, my) = (
        points.iter().map(|p| p[0]).sum::<f32>() / n,
        points.iter().map(|p| p[1]).sum::<f32>() / n,
    );
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in points {
        let (dx, dy) = (p[0] - mx, p[1] - my);
        let r = [dx * ca + dy * sa, -dx * sa + dy * ca];
        for k in 0..2 {
            lo[k] = lo[k].min(r[k]);
            hi[k] = hi[k].max(r[k]);
        }
    }
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    Roi {
        cx: mx + mid[0] * ca - mid[1] * sa,
        cy: my + mid[0] * sa + mid[1] * ca,
        side: (hi[0] - lo[0]).max(hi[1] - lo[1]) * 1.5,
        angle,
    }
}

/// Bilinear sample at image pixel (x, y), as 0..1 floats; edges clamp.
fn bilinear(img: &image::RgbImage, x: f32, y: f32) -> [f32; 3] {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let (x, y) = (x - 0.5, y - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let px = |xi: i64, yi: i64| img.get_pixel(xi.clamp(0, w - 1) as u32, yi.clamp(0, h - 1) as u32);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (
        px(x0, y0),
        px(x0 + 1, y0),
        px(x0, y0 + 1),
        px(x0 + 1, y0 + 1),
    );
    std::array::from_fn(|k| {
        let top = a[k] as f32 * (1.0 - fx) + b[k] as f32 * fx;
        let bottom = c[k] as f32 * (1.0 - fx) + d[k] as f32 * fx;
        (top * (1.0 - fy) + bottom * fy) / 255.0
    })
}

fn luma(img: &image::RgbImage) -> Vec<f32> {
    img.pixels()
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0)
        .collect()
}

const TILE: usize = 16;

/// Laplacian energy (mean square) over a rectangle of the luma plane.
fn energy(luma: &[f32], w: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> f32 {
    let at = |x: usize, y: usize| luma[y * w + x];
    let (mut sum, mut n) = (0f32, 0u32);
    for y in y0.max(1)..y1.min(luma.len() / w - 1) {
        for x in x0.max(1)..x1.min(w - 1) {
            let lap = 4.0 * at(x, y) - at(x - 1, y) - at(x + 1, y) - at(x, y - 1) - at(x, y + 1);
            sum += lap * lap;
            n += 1;
        }
    }
    sum / n.max(1) as f32
}

/// The frame's sharpest tiles (top 5%), the yardstick for each face.
fn frame_focus(luma: &[f32], w: usize, h: usize) -> f32 {
    let mut tiles = Vec::new();
    for ty in (0..h).step_by(TILE) {
        for tx in (0..w).step_by(TILE) {
            tiles.push(energy(luma, w, tx, ty, tx + TILE, ty + TILE));
        }
    }
    tiles.sort_by(|a, b| b.total_cmp(a));
    let top = (tiles.len() / 20).max(1);
    tiles[..top].iter().sum::<f32>() / top as f32
}

/// Focus energy around both eyes and brows.
fn eye_focus(luma: &[f32], img: &image::RgbImage, points: &[[f32; 2]]) -> f32 {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let xs = EYE_REGION.map(|i| points[i][0]);
    let ys = EYE_REGION.map(|i| points[i][1]);
    let (x0, x1) = (
        xs.iter().copied().fold(f32::MAX, f32::min),
        xs.iter().copied().fold(f32::MIN, f32::max),
    );
    let (y0, y1) = (
        ys.iter().copied().fold(f32::MAX, f32::min),
        ys.iter().copied().fold(f32::MIN, f32::max),
    );
    let pad = (x1 - x0) * 0.1;
    let c = |v: f32, max: f32| v.clamp(0.0, max) as usize;
    energy(
        luma,
        img.width() as usize,
        c(x0 - pad, w),
        c(y0 - pad, h),
        c(x1 + pad, w),
        c(y1 + pad, h),
    )
}
