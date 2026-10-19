//! The local models behind AI Masks (ADR 0012): MediaPipe's multiclass selfie segmenter
//! (person, hair, face skin, body skin, clothes), OpenCV's YuNet face detector (boxes plus
//! eye and mouth landmarks) and TensorFlow's DeepLab v3 MobileNetV2 trained on ADE20K
//! (scene classes; we use sky). Both are embedded in the binary and run on the CPU with
//! tract, so Masks work offline, with no API keys and nothing to install.

use crate::analyzers::runtime::{Lazy, Model};
use crate::{Error, Result, RgbImage};
use tract_onnx::prelude::*;

const SEGMENTER: &[u8] = include_bytes!("../../models/selfie_multiclass_256x256.onnx");
const FACE_DETECTOR: &[u8] = include_bytes!("../../models/face_detection_yunet_2023mar.onnx");
const SCENE: &[u8] = include_bytes!("../../models/deeplabv3_mnv2_ade20k.onnx");

/// Segmenter input and output size (square; the image is stretched to it).
pub const SEG_SIZE: usize = 256;
/// Scene segmenter input size (square; stretched) and its output size.
const SCENE_SIZE: usize = 513;
pub const SCENE_OUT: usize = 129;
/// ADE20K's sky class in DeepLab's 151-class numbering (0 = unlabelled).
const ADE_SKY: usize = 3;

/// Face detector input size (square; the image is letterboxed into it).
const DET_SIZE: usize = 640;

static SEGMENTER_MODEL: Lazy = Lazy::new(SEGMENTER, &[1, SEG_SIZE, SEG_SIZE, 3]);
static FACE_DETECTOR_MODEL: Lazy = Lazy::new(FACE_DETECTOR, &[1, 3, DET_SIZE, DET_SIZE]);
static SCENE_MODEL: Lazy = Lazy::new(SCENE, &[1, SCENE_SIZE, SCENE_SIZE, 3]);

fn segmenter() -> Result<&'static Model> {
    SEGMENTER_MODEL.get()
}

fn face_detector() -> Result<&'static Model> {
    FACE_DETECTOR_MODEL.get()
}

fn scene() -> Result<&'static Model> {
    SCENE_MODEL.get()
}

/// Sky probability at [`SCENE_OUT`]², from the scene segmenter's softmax.
pub fn sky(img: &RgbImage) -> Result<Vec<f32>> {
    let n = SCENE_SIZE;
    let input: Tensor = tract_ndarray::Array4::from_shape_fn((1, n, n, 3), |(_, y, x, c)| {
        sample(
            img,
            (x as f32 + 0.5) / n as f32,
            (y as f32 + 0.5) / n as f32,
        )[c] * 2.0
            - 1.0
    })
    .into();
    let out = scene()?
        .run(tvec!(input.into()))
        .map_err(|e| Error(format!("scene segmenter: {e}")))?;
    let t = out[0].clone().into_tensor();
    let v = t
        .to_plain_array_view::<f32>()
        .map_err(|e| Error(format!("scene segmenter output: {e}")))?;
    let (h, w, k) = (v.shape()[1], v.shape()[2], v.shape()[3]);
    if (h, w) != (SCENE_OUT, SCENE_OUT) {
        return Err(Error(format!(
            "scene segmenter: unexpected output {:?}",
            v.shape()
        )));
    }
    let mut plane = vec![0f32; h * w];
    for y in 0..h {
        for x in 0..w {
            let mx = (0..k).map(|c| v[[0, y, x, c]]).fold(f32::MIN, f32::max);
            let sum: f32 = (0..k).map(|c| (v[[0, y, x, c]] - mx).exp()).sum();
            plane[y * w + x] = (v[[0, y, x, ADE_SKY]] - mx).exp() / sum;
        }
    }
    Ok(plane)
}

/// Segmenter classes, in output order.
pub const CLASSES: [&str; 6] = [
    "background",
    "hair",
    "body_skin",
    "face_skin",
    "clothes",
    "other",
];

/// Per-class probabilities at [`SEG_SIZE`]², class-major (`planes[c][y * SEG_SIZE + x]`).
pub fn segment(img: &RgbImage) -> Result<[Vec<f32>; 6]> {
    let n = SEG_SIZE;
    let input: Tensor = tract_ndarray::Array4::from_shape_fn((1, n, n, 3), |(_, y, x, c)| {
        sample(
            img,
            (x as f32 + 0.5) / n as f32,
            (y as f32 + 0.5) / n as f32,
        )[c]
    })
    .into();
    let out = segmenter()?
        .run(tvec!(input.into()))
        .map_err(|e| Error(format!("segmenter: {e}")))?;
    let t = out[0].clone().into_tensor();
    let v = t
        .to_plain_array_view::<f32>()
        .map_err(|e| Error(format!("segmenter output: {e}")))?;
    let mut planes: [Vec<f32>; 6] = std::array::from_fn(|_| vec![0f32; n * n]);
    for y in 0..n {
        for x in 0..n {
            let p: [f32; 6] = std::array::from_fn(|c| v[[0, y, x, c]].max(0.0));
            let sum = p.iter().sum::<f32>().max(1e-6);
            for c in 0..6 {
                planes[c][y * n + x] = p[c] / sum;
            }
        }
    }
    Ok(planes)
}

/// Bilinear sample of an 8-bit image at normalised (u, v), as 0..1 floats.
fn sample(img: &RgbImage, u: f32, v: f32) -> [f32; 3] {
    let (w, h) = (img.width, img.height);
    let x = (u * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
    let y = (v * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |x: usize, y: usize, c: usize| img.data[(y * w + x) * 3 + c] as f32 / 255.0;
    std::array::from_fn(|c| {
        let a = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
        let b = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
        a * (1.0 - fy) + b * fy
    })
}

/// One detected face, in normalised image coordinates (0..1 of width and height).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Face {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
    /// Eye centres as seen in the image: `[left, right]`, each `[x, y]`.
    pub eyes: [[f32; 2]; 2],
    pub nose: [f32; 2],
    /// Mouth corners as seen in the image: `[left, right]`.
    pub mouth: [[f32; 2]; 2],
}

const MIN_FACE_SCORE: f32 = 0.6;

/// Faces in `img`, left to right.
pub fn detect_faces(img: &RgbImage) -> Result<Vec<Face>> {
    let n = DET_SIZE;
    let scale = n as f32 / img.width.max(img.height) as f32;
    let (sw, sh) = (img.width as f32 * scale, img.height as f32 * scale);
    // YuNet takes BGR 0..255, letterboxed top-left.
    let input: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, n, n), |(_, c, y, x)| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        if fx > sw || fy > sh {
            return 0.0;
        }
        sample(img, fx / sw, fy / sh)[2 - c] * 255.0
    })
    .into();
    let out = face_detector()?
        .run(tvec!(input.into()))
        .map_err(|e| Error(format!("face detector: {e}")))?;
    let views: Vec<Tensor> = out.iter().map(|t| t.clone().into_tensor()).collect();
    let flat = |i: usize| -> Result<&[f32]> {
        views[i]
            .try_as_plain_ram()
            .and_then(|v| v.as_slice::<f32>())
            .map_err(|e| Error(format!("face detector output: {e}")))
    };
    let mut found = Vec::new();
    for (k, stride) in [8usize, 16, 32].into_iter().enumerate() {
        let (cls, obj, bbox, kps) = (flat(k)?, flat(3 + k)?, flat(6 + k)?, flat(9 + k)?);
        let cols = n / stride;
        for i in 0..cls.len() {
            let score = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
            if score < MIN_FACE_SCORE {
                continue;
            }
            let (c, r) = ((i % cols) as f32, (i / cols) as f32);
            let s = stride as f32;
            let b = &bbox[i * 4..i * 4 + 4];
            let (cx, cy) = ((c + b[0]) * s, (r + b[1]) * s);
            let (w, h) = (b[2].exp() * s, b[3].exp() * s);
            let p = &kps[i * 10..i * 10 + 10];
            let pt = |j: usize| [(p[j * 2] + c) * s / sw, (p[j * 2 + 1] + r) * s / sh];
            found.push(Face {
                x: (cx - w / 2.0) / sw,
                y: (cy - h / 2.0) / sh,
                w: w / sw,
                h: h / sh,
                score,
                eyes: [pt(0), pt(1)],
                nose: pt(2),
                mouth: [pt(3), pt(4)],
            });
        }
    }
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Face> = Vec::new();
    for f in found {
        if kept.iter().all(|k| iou(k, &f) < 0.3) {
            kept.push(f);
        }
    }
    for f in &mut kept {
        if f.eyes[0][0] > f.eyes[1][0] {
            f.eyes.swap(0, 1);
        }
        if f.mouth[0][0] > f.mouth[1][0] {
            f.mouth.swap(0, 1);
        }
    }
    kept.sort_by(|a, b| a.x.total_cmp(&b.x));
    Ok(kept)
}

fn iou(a: &Face, b: &Face) -> f32 {
    let ix = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.0);
    let iy = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.0);
    let i = ix * iy;
    i / (a.w * a.h + b.w * b.h - i).max(1e-9)
}
