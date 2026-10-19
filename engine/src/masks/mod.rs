//! Masks: where a local adjustment applies (GLOSSARY "Mask").
//!
//! A Mask in a Recipe is a description of how it is made, never a bitmap (spec, Recipe):
//!
//! | kind         | fields                                   | made from                         |
//! |--------------|------------------------------------------|-----------------------------------|
//! | `subject`    |                                          | every person (segmenter)          |
//! | `background` |                                          | everything but the people         |
//! | `faces`      | `face`? (index, left to right)           | face skin (segmenter + detector)  |
//! | `skin`       |                                          | face and body skin                |
//! | `person`     | `person`? (face index), `parts`          | chosen parts of one person or all |
//! | `hair`       |                                          | hair                              |
//! | `sky`        |                                          | sky (scene segmenter)             |
//! | `brush`      | `dots: [{x, y, r}]`, `erase`?            | hand-painted dots                 |
//! | `linear`     | `from: {x, y}`, `to: {x, y}`             | full at `from`, none at `to`      |
//! | `radial`     | `x, y, rx, ry`, `feather`? (0..1)        | an ellipse, soft edge             |
//! | `gradient`   | `side: left/right/bottom`, `extent`      | the prototype's edge gradient     |
//!
//! A `person` Mask is Lightroom's Select People: `parts` is any of `face_skin`, `body_skin`,
//! `eyes`, `teeth`, `hair` and `whole` (the whole person), combined, and `person` keeps it to
//! the one whose face has that index (as for `faces`).
//!
//! Every kind also takes `invert: true`. Coordinates are 0..1 of the upright image's width
//! (x) and height (y); `rx` is a fraction of the width, `ry` of the height, and a brush
//! dot's `r` a fraction of the long edge. AI kinds come from an [`Analysis`] of the Photo
//! (its `segment`, `sky` and `faces` Facts, ADR 0013) and are refined against the render, so
//! their edges follow the image.

pub mod models;
pub mod refine;

pub use models::Face;

use crate::{Error, Result, RgbImage};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dot {
    pub x: f64,
    pub y: f64,
    pub r: f64,
}

/// A part of a person a `person` Mask can select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonPart {
    FaceSkin,
    BodySkin,
    Eyes,
    Teeth,
    Hair,
    Whole,
}

fn face_skin_only() -> Vec<PersonPart> {
    vec![PersonPart::FaceSkin]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Bottom,
}

fn half() -> f64 {
    0.5
}

fn no_dots(v: &[Dot]) -> bool {
    v.is_empty()
}

/// How a Mask is made. See the module docs for each kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Shape {
    Subject,
    Background,
    Faces {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<usize>,
    },
    Skin,
    Hair,
    Sky,
    Person {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        person: Option<usize>,
        #[serde(default = "face_skin_only")]
        parts: Vec<PersonPart>,
    },
    Brush {
        dots: Vec<Dot>,
        #[serde(default, skip_serializing_if = "no_dots")]
        erase: Vec<Dot>,
    },
    Linear {
        from: Point,
        to: Point,
    },
    Radial {
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        #[serde(default = "half")]
        feather: f64,
    },
    Gradient {
        side: Side,
        extent: f64,
    },
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    #[serde(flatten)]
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
}

impl Mask {
    pub fn new(shape: Shape) -> Self {
        Mask {
            shape,
            invert: false,
        }
    }

    /// True when the Mask needs an [`Analysis`] of the Photo (a model or the sky finder).
    pub fn needs_analysis(&self) -> bool {
        matches!(
            self.shape,
            Shape::Subject
                | Shape::Background
                | Shape::Faces { .. }
                | Shape::Skin
                | Shape::Hair
                | Shape::Sky
                | Shape::Person { .. }
        )
    }

    pub fn kind(&self) -> &'static str {
        match self.shape {
            Shape::Subject => "subject",
            Shape::Background => "background",
            Shape::Faces { .. } => "faces",
            Shape::Skin => "skin",
            Shape::Hair => "hair",
            Shape::Sky => "sky",
            Shape::Person { .. } => "person",
            Shape::Brush { .. } => "brush",
            Shape::Linear { .. } => "linear",
            Shape::Radial { .. } => "radial",
            Shape::Gradient { .. } => "gradient",
        }
    }
}

/// What the models found in one Photo, at their own small resolutions. Everything a Mask
/// or a retouch needs, and cheap to cache.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// Segmenter probabilities per [`models::CLASSES`], each [`models::SEG_SIZE`]².
    pub seg: [Vec<f32>; 6],
    /// Sky probability, `sky_w` x `sky_h`.
    pub sky: Vec<f32>,
    pub sky_w: usize,
    pub sky_h: usize,
    pub faces: Vec<Face>,
}

/// See [`Analysis::coverage`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Coverage {
    pub people: f32,
    pub sky: f32,
    pub faces: usize,
}

/// The Facts an [`Analysis`] is made from (ADR 0013).
pub const ANALYSIS_FACTS: &[&str] = &["segment", "sky", "faces"];

impl Analysis {
    /// Run the models on an upright 8-bit image (any size): for renders with no Photo
    /// behind them. A Photo's Analysis comes from its Facts ([`Analysis::from_facts`]).
    pub fn of(img: &RgbImage) -> Result<Analysis> {
        let small = shrink(img, crate::analyzers::SMALL_EDGE);
        let ((seg, faces), sky) = rayon::join(
            || rayon::join(|| models::segment(&small), || models::detect_faces(&small)),
            || models::sky(&small),
        );
        let n = models::SCENE_OUT;
        Ok(Analysis {
            seg: seg?,
            sky: sky?,
            sky_w: n,
            sky_h: n,
            faces: faces?,
        })
    }

    /// Assemble from a Photo's `segment`, `sky` and `faces` Facts.
    pub fn from_facts(facts: &crate::analyzers::Facts) -> Result<Analysis> {
        use crate::analyzers::{faces, segment, sky};
        let get = |id: &str| {
            facts
                .get(id)
                .ok_or_else(|| Error(format!("no {id} Fact for the Mask analysis")))
        };
        let n = models::SCENE_OUT;
        Ok(Analysis {
            seg: segment::planes(get("segment")?)?,
            sky: sky::plane(get("sky")?)?,
            sky_w: n,
            sky_h: n,
            faces: faces::read(get("faces")?),
        })
    }

    /// A probability plane by class name, at segmenter size.
    fn class(&self, name: &str) -> &[f32] {
        let i = models::CLASSES
            .iter()
            .position(|c| *c == name)
            .expect("class");
        &self.seg[i]
    }

    /// How much of the frame is people and sky (0..1 each), and how many faces: enough for
    /// an editor to know which AI Masks are worth using on this Photo.
    pub fn coverage(&self) -> Coverage {
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        Coverage {
            people: 1.0 - mean(self.class("background")),
            sky: mean(&self.sky),
            faces: self.faces.len(),
        }
    }

    /// Median face width as a fraction of the image width, if there are faces.
    pub fn face_width(&self) -> Option<f32> {
        let mut w: Vec<f32> = self.faces.iter().map(|f| f.w).collect();
        w.sort_by(f32::total_cmp);
        w.get(w.len() / 2).copied()
    }
}

/// Shrink so the long edge is at most `edge` (area average), keeping the aspect.
pub fn shrink(img: &RgbImage, edge: usize) -> RgbImage {
    let long = img.width.max(img.height);
    if long <= edge {
        return RgbImage {
            width: img.width,
            height: img.height,
            data: img.data.clone(),
        };
    }
    let s = edge as f32 / long as f32;
    let (w, h) = (
        ((img.width as f32 * s).round() as usize).max(1),
        ((img.height as f32 * s).round() as usize).max(1),
    );
    let planes: Vec<Vec<f32>> = (0..3)
        .map(|c| {
            let p: Vec<f32> = img
                .data
                .iter()
                .skip(c)
                .step_by(3)
                .map(|&v| v as f32)
                .collect();
            let r = ((1.0 / s) / 2.0).floor() as usize;
            refine::resize(
                &refine::box_blur(&p, img.width, img.height, r),
                img.width,
                img.height,
                w,
                h,
            )
        })
        .collect();
    let mut data = vec![0u8; w * h * 3];
    for (i, px) in data.chunks_mut(3).enumerate() {
        for c in 0..3 {
            px[c] = planes[c][i].round().clamp(0.0, 255.0) as u8;
        }
    }
    RgbImage {
        width: w,
        height: h,
        data,
    }
}

/// Where Masks are drawn: the upright render's size, its luma as the edge guide, and the
/// Analysis when an AI Mask needs one.
pub struct Canvas<'a> {
    pub width: usize,
    pub height: usize,
    /// Luma of the base render (encoded 0..1), `width` x `height`.
    pub guide: &'a [f32],
    pub analysis: Option<&'a Analysis>,
}

impl Canvas<'_> {
    fn analysis(&self, m: &Mask) -> Result<&Analysis> {
        self.analysis
            .ok_or_else(|| Error(format!("a {} Mask needs the Photo's analysis", m.kind())))
    }

    /// The Mask's weight (0..1) at every pixel.
    pub fn plane(&self, m: &Mask) -> Result<Vec<f32>> {
        let (w, h) = (self.width, self.height);
        let n = models::SEG_SIZE;
        let ai = |coarse: Vec<f32>, cw: usize, ch: usize| {
            refine::refine(&coarse, cw, ch, self.guide, w, h, 1.5 / n as f32)
        };
        let mut p = match &m.shape {
            Shape::Subject => {
                let a = self.analysis(m)?;
                ai(
                    a.class("background").iter().map(|v| 1.0 - v).collect(),
                    n,
                    n,
                )
            }
            Shape::Background => ai(self.analysis(m)?.class("background").to_vec(), n, n),
            Shape::Skin => {
                let a = self.analysis(m)?;
                let (f, b) = (a.class("face_skin"), a.class("body_skin"));
                ai(f.iter().zip(b).map(|(x, y)| x + y).collect(), n, n)
            }
            Shape::Hair => ai(self.analysis(m)?.class("hair").to_vec(), n, n),
            Shape::Faces { face } => {
                let a = self.analysis(m)?;
                let mut coarse = a.class("face_skin").to_vec();
                if let Some(i) = face {
                    check_face(a, *i)?;
                    let gate = face_gate(&a.faces, *i, n, n);
                    coarse.iter_mut().zip(&gate).for_each(|(v, g)| *v *= g);
                }
                ai(coarse, n, n)
            }
            Shape::Sky => {
                let a = self.analysis(m)?;
                ai(a.sky.clone(), a.sky_w, a.sky_h)
            }
            Shape::Person { person, parts } => {
                let a = self.analysis(m)?;
                let gate = match person {
                    Some(i) => {
                        check_face(a, *i)?;
                        Some(face_gate(&a.faces, *i, n, n))
                    }
                    None => None,
                };
                // Segmenter parts, combined and kept to the person, then refined together.
                let mut coarse = vec![0f32; n * n];
                for part in parts {
                    let class = match part {
                        PersonPart::FaceSkin => a.class("face_skin").to_vec(),
                        PersonPart::BodySkin => a.class("body_skin").to_vec(),
                        PersonPart::Hair => a.class("hair").to_vec(),
                        PersonPart::Whole => {
                            a.class("background").iter().map(|v| 1.0 - v).collect()
                        }
                        PersonPart::Eyes | PersonPart::Teeth => continue,
                    };
                    coarse
                        .iter_mut()
                        .zip(&class)
                        .for_each(|(o, v)| *o = o.max(*v));
                }
                if let Some(g) = &gate {
                    coarse.iter_mut().zip(g).for_each(|(v, g)| *v *= g);
                }
                let mut out = if coarse.iter().any(|v| *v > 0.0) {
                    ai(coarse, n, n)
                } else {
                    vec![0f32; w * h]
                };
                // Eyes and teeth come from the face detector's landmarks, already sharp.
                let only = |i: usize| person.is_none_or(|p| p == i);
                for part in parts {
                    let plane = match part {
                        PersonPart::Eyes => self.eyes_of(only),
                        PersonPart::Teeth => self.mouths_of(only),
                        _ => continue,
                    };
                    out.iter_mut().zip(&plane).for_each(|(o, v)| *o = o.max(*v));
                }
                out
            }
            Shape::Brush { dots, erase } => {
                let long = w.max(h) as f32;
                let paint = |dots: &[Dot], out: &mut [f32], erase: bool| {
                    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                        let py = y as f32 + 0.5;
                        for d in dots {
                            let (cx, cy, r) = (
                                d.x as f32 * w as f32,
                                d.y as f32 * h as f32,
                                (d.r as f32 * long).max(0.5),
                            );
                            if (py - cy).abs() > r {
                                continue;
                            }
                            let x0 = ((cx - r).floor().max(0.0)) as usize;
                            let x1 = ((cx + r).ceil() as usize).min(w);
                            for (x, o) in row.iter_mut().enumerate().take(x1).skip(x0) {
                                let dist = ((x as f32 + 0.5 - cx).hypot(py - cy)) / r;
                                // Solid to 40% of the radius, then a smooth falloff (as the prototype).
                                let v = 1.0 - smoothstep(0.4, 1.0, dist);
                                *o = if erase { *o * (1.0 - v) } else { o.max(v) };
                            }
                        }
                    });
                };
                let mut out = vec![0f32; w * h];
                paint(dots, &mut out, false);
                paint(erase, &mut out, true);
                out
            }
            Shape::Linear { from, to } => linear(w, h, *from, *to),
            Shape::Gradient { side, extent } => {
                let e = extent.clamp(0.0, 1.0);
                let (from, to) = match side {
                    Side::Left => (Point { x: 0.0, y: 0.5 }, Point { x: e, y: 0.5 }),
                    Side::Right => (Point { x: 1.0, y: 0.5 }, Point { x: 1.0 - e, y: 0.5 }),
                    Side::Bottom => (Point { x: 0.5, y: 1.0 }, Point { x: 0.5, y: 1.0 - e }),
                };
                linear(w, h, from, to)
            }
            Shape::Radial {
                x,
                y,
                rx,
                ry,
                feather,
            } => {
                let f = feather.clamp(0.0, 1.0) as f32;
                let (x, y) = (*x as f32, *y as f32);
                let (rx, ry) = (rx.abs().max(1e-4) as f32, ry.abs().max(1e-4) as f32);
                let mut out = vec![0f32; w * h];
                out.par_chunks_mut(w).enumerate().for_each(|(py, row)| {
                    let v = ((py as f32 + 0.5) / h as f32 - y) / ry;
                    for (px, o) in row.iter_mut().enumerate() {
                        let u = ((px as f32 + 0.5) / w as f32 - x) / rx;
                        let d = u.hypot(v);
                        *o = 1.0 - smoothstep(1.0 - f, 1.0 + 1e-4, d);
                    }
                });
                out
            }
        };
        if m.invert {
            p.par_iter_mut().for_each(|v| *v = 1.0 - *v);
        }
        Ok(p)
    }

    /// Eyes (irises and whites) of every face, soft ellipses from the detector's landmarks.
    pub fn eyes(&self) -> Vec<f32> {
        self.eyes_of(|_| true)
    }

    fn eyes_of(&self, only: impl Fn(usize) -> bool) -> Vec<f32> {
        self.landmark_plane(only, |f| {
            let d = dist(f.eyes[0], f.eyes[1], self.width, self.height);
            f.eyes.iter().map(|e| (*e, 0.22 * d, 0.13 * d)).collect()
        })
    }

    /// Mouths of every face (where teeth show), from the mouth corners.
    pub fn mouths(&self) -> Vec<f32> {
        self.mouths_of(|_| true)
    }

    fn mouths_of(&self, only: impl Fn(usize) -> bool) -> Vec<f32> {
        self.landmark_plane(only, |f| {
            let d = dist(f.mouth[0], f.mouth[1], self.width, self.height);
            let c = [
                (f.mouth[0][0] + f.mouth[1][0]) / 2.0,
                (f.mouth[0][1] + f.mouth[1][1]) / 2.0,
            ];
            vec![(c, 0.5 * d, 0.22 * d)]
        })
    }

    /// Soft ellipses (centre, radius x, radius y in pixels) per face, combined by max.
    fn landmark_plane(
        &self,
        only: impl Fn(usize) -> bool,
        shapes: impl Fn(&Face) -> Vec<([f32; 2], f32, f32)>,
    ) -> Vec<f32> {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0f32; w * h];
        let Some(a) = self.analysis else {
            return out;
        };
        for (_, f) in a.faces.iter().enumerate().filter(|(i, _)| only(*i)) {
            for (c, rx, ry) in shapes(f) {
                let (cx, cy) = (c[0] * w as f32, c[1] * h as f32);
                let (rx, ry) = (rx.max(1.0), ry.max(1.0));
                let y0 = (cy - ry).floor().max(0.0) as usize;
                let y1 = ((cy + ry).ceil() as usize).min(h);
                let x0 = (cx - rx).floor().max(0.0) as usize;
                let x1 = ((cx + rx).ceil() as usize).min(w);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let d = ((x as f32 + 0.5 - cx) / rx).hypot((y as f32 + 0.5 - cy) / ry);
                        let v = 1.0 - smoothstep(0.6, 1.0, d);
                        let o = &mut out[y * w + x];
                        *o = o.max(v);
                    }
                }
            }
        }
        out
    }
}

fn dist(a: [f32; 2], b: [f32; 2], w: usize, h: usize) -> f32 {
    ((a[0] - b[0]) * w as f32).hypot((a[1] - b[1]) * h as f32)
}

/// Where face `i` is, at w x h: a generous box around it (ears, forehead and jaw stay in),
/// minus whatever is closer to another face, so neighbouring faces don't bleed together.
fn check_face(a: &Analysis, i: usize) -> Result<()> {
    a.faces.get(i).map(|_| ()).ok_or_else(|| {
        Error(format!(
            "no face {i}: this Photo has {} (numbered from 0, left to right)",
            a.faces.len()
        ))
    })
}

fn face_gate(faces: &[Face], i: usize, w: usize, h: usize) -> Vec<f32> {
    let f = &faces[i];
    let (x0, x1) = (f.x - 0.35 * f.w, f.x + 1.35 * f.w);
    let (y0, y1) = (f.y - 0.45 * f.h, f.y + 1.3 * f.h);
    let soft = 0.15 * f.w.max(f.h);
    // Distance to a face's centre in units of its size.
    let reach = |g: &Face, u: f32, v: f32| {
        ((u - (g.x + g.w / 2.0)) / g.w).hypot((v - (g.y + g.h / 2.0)) / g.h)
    };
    let mut out = vec![0f32; w * h];
    for (k, o) in out.iter_mut().enumerate() {
        let (u, v) = (
            ((k % w) as f32 + 0.5) / w as f32,
            ((k / w) as f32 + 0.5) / h as f32,
        );
        let mine = reach(f, u, v);
        if faces
            .iter()
            .enumerate()
            .any(|(j, g)| j != i && reach(g, u, v) < mine)
        {
            continue;
        }
        let ex = (x0 - u).max(u - x1).max(0.0);
        let ey = (y0 - v).max(v - y1).max(0.0);
        *o = 1.0 - smoothstep(0.0, soft, ex.hypot(ey));
    }
    out
}

fn linear(w: usize, h: usize, from: Point, to: Point) -> Vec<f32> {
    let (fx, fy) = ((from.x * w as f64) as f32, (from.y * h as f64) as f32);
    let (dx, dy) = (
        ((to.x - from.x) * w as f64) as f32,
        ((to.y - from.y) * h as f64) as f32,
    );
    let len2 = (dx * dx + dy * dy).max(1e-6);
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let t = ((x as f32 + 0.5 - fx) * dx + (y as f32 + 0.5 - fy) * dy) / len2;
            *o = 1.0 - smoothstep(0.0, 1.0, t);
        }
    });
    out
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn canvas(guide: &[f32], w: usize, h: usize) -> Canvas<'_> {
        Canvas {
            width: w,
            height: h,
            guide,
            analysis: None,
        }
    }

    #[test]
    fn prototype_masks_parse_and_round_trip() {
        for v in [
            json!({ "kind": "faces" }),
            json!({ "kind": "faces", "face": 1 }),
            json!({ "kind": "sky", "invert": true }),
            json!({ "kind": "gradient", "side": "left", "extent": 0.4 }),
            json!({ "kind": "brush", "dots": [{ "x": 0.5, "y": 0.5, "r": 0.1 }] }),
            json!({ "kind": "radial", "x": 0.5, "y": 0.4, "rx": 0.2, "ry": 0.3, "feather": 0.5 }),
            json!({ "kind": "linear", "from": { "x": 0.5, "y": 0.0 }, "to": { "x": 0.5, "y": 0.6 } }),
        ] {
            let m: Mask = serde_json::from_value(v.clone()).unwrap();
            assert_eq!(serde_json::to_value(&m).unwrap(), v);
        }
        assert!(serde_json::from_value::<Mask>(json!({ "kind": "teeth" })).is_err());
    }

    #[test]
    fn drawn_masks_fall_off_where_they_should() {
        let (w, h) = (100, 50);
        let g = vec![0.5; w * h];
        let c = canvas(&g, w, h);
        let at = |p: &[f32], x: usize, y: usize| p[y * w + x];

        let lin = c
            .plane(&Mask::new(Shape::Linear {
                from: Point { x: 0.0, y: 0.5 },
                to: Point { x: 0.5, y: 0.5 },
            }))
            .unwrap();
        assert!(at(&lin, 0, 25) > 0.99 && at(&lin, 25, 25) > 0.4 && at(&lin, 60, 25) == 0.0);

        let rad = c
            .plane(&Mask::new(Shape::Radial {
                x: 0.5,
                y: 0.5,
                rx: 0.2,
                ry: 0.4,
                feather: 0.3,
            }))
            .unwrap();
        assert!(at(&rad, 50, 25) == 1.0 && at(&rad, 90, 25) == 0.0 && at(&rad, 50, 2) < 0.3);

        let brush = Mask {
            shape: Shape::Brush {
                dots: vec![Dot {
                    x: 0.3,
                    y: 0.5,
                    r: 0.1,
                }],
                erase: vec![Dot {
                    x: 0.35,
                    y: 0.5,
                    r: 0.03,
                }],
            },
            invert: true,
        };
        let b = c.plane(&brush).unwrap();
        assert!(at(&b, 30, 25) < 0.01, "painted, inverted");
        assert!(at(&b, 35, 25) > 0.9, "erased, inverted");
        assert!(at(&b, 80, 25) == 1.0, "outside the stroke, inverted");
    }

    #[test]
    fn ai_masks_without_analysis_say_why() {
        let g = vec![0.5; 16];
        let e = canvas(&g, 4, 4).plane(&Mask::new(Shape::Sky)).unwrap_err();
        assert!(e.0.contains("analysis"), "{}", e.0);
    }
}
