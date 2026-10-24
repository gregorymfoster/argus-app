//! The Mask path of a render: the global finish, then each local adjustment blended in
//! by its Mask, then natural retouching (skin, eyes, teeth, light on faces), then the global
//! neighbourhood effects ([`crate::effects`]). Only taken
//! when a Recipe needs it ([`Recipe::needs_masks`]), so plain renders stay on the fast path
//! and the golden images don't move.
//!
//! Retouching never moves a pixel: no reshaping, no warping, no generative fill (spec).

use crate::masks::{refine, Analysis, Canvas, Mask, Shape};
use crate::pipeline::{to8, Finish, Look, LUMA};
use crate::recipe::{Adjust, Recipe};
use crate::RgbImage;
use rayon::prelude::*;

/// Finish an upright linear camera-RGB image (`w` x `h`, interleaved) with a Recipe that
/// has Masks. Without an `analysis`, one is made from the base render when an AI Mask or
/// retouch needs it. `scale` is pixels per pixel of the original ([`crate::effects`]).
pub fn finish(
    cam: &[f32],
    (w, h, scale): (usize, usize, f32),
    matrix: [[f32; 3]; 3],
    look: &Look,
    recipe: &Recipe,
    analysis: Option<&Analysis>,
) -> Vec<u8> {
    let blurred;
    let cam = match recipe.lens_blur.active() {
        true => {
            blurred = lens_blur(cam, (w, h), matrix, look, recipe, analysis);
            blurred.as_deref().unwrap_or(cam)
        }
        false => cam,
    };
    let global = recipe.render_adjust();
    let base_fin = Finish::new(matrix, look, &global);
    let mut out: Vec<f32> = cam
        .par_chunks(3)
        .flat_map_iter(|p| base_fin.px([p[0], p[1], p[2]]))
        .collect();
    let guide: Vec<f32> = out
        .par_chunks(3)
        .map(|p| (LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2]).clamp(0.0, 1.0))
        .collect();

    // Each local with its Amount (Lightroom's, 0..2 here): how much of the adjusted render
    // the Mask lets through, so 150% pushes past the sliders the way Lightroom's does.
    let mut locals: Vec<(Mask, Adjust, f32)> = Vec::new();
    if recipe.correction.face != 0.0 {
        locals.push((
            Mask::new(Shape::Faces { face: None }),
            Adjust {
                exposure: recipe.correction.face,
                ..Default::default()
            },
            1.0,
        ));
    }
    locals.extend(recipe.local.iter().map(|l| {
        let amount = l.amount.unwrap_or(100.0).clamp(0.0, 200.0) / 100.0;
        (l.mask.clone(), l.adjust.clone(), amount as f32)
    }));

    let g = &recipe.global;
    let wants_ai = locals.iter().any(|(m, _, _)| m.needs_analysis())
        || g.skin > 0.0
        || g.eyes > 0.0
        || g.teeth > 0.0
        || locals.iter().any(|(_, a, _)| a.eyes > 0.0 || a.teeth > 0.0);
    let made;
    let analysis = match analysis {
        Some(a) => Some(a),
        None if wants_ai => {
            let img = RgbImage {
                width: w,
                height: h,
                data: out.iter().map(|&v| to8(v)).collect(),
            };
            made = Analysis::of(&img);
            match &made {
                Ok(a) => Some(a),
                Err(e) => {
                    eprintln!("masks: no analysis ({e}); AI Masks render empty");
                    None
                }
            }
        }
        None => None,
    };
    let wants_rgb = locals.iter().any(|(m, _, _)| {
        m.all()
            .iter()
            .any(|x| matches!(x.shape, Shape::Color { .. }))
    });
    let base_rgb = wants_rgb.then(|| out.clone());
    let canvas = Canvas {
        width: w,
        height: h,
        guide: &guide,
        rgb: base_rgb.as_deref(),
        analysis,
    };

    let n = w * h;
    let mut skin = vec![0f32; n];
    let mut eyes = vec![0f32; n];
    let mut teeth = vec![0f32; n];
    let add = |dst: &mut [f32], src: &[f32], k: f32| {
        dst.par_iter_mut().zip(src).for_each(|(d, s)| *d += s * k);
    };
    let eye_plane =
        (g.eyes > 0.0 || locals.iter().any(|(_, a, _)| a.eyes > 0.0)).then(|| canvas.eyes());
    let mouth_plane =
        (g.teeth > 0.0 || locals.iter().any(|(_, a, _)| a.teeth > 0.0)).then(|| canvas.mouths());

    for (mask, delta, amount) in &locals {
        let weight = match canvas.plane(mask) {
            Ok(mut p) => {
                if *amount != 1.0 {
                    p.par_iter_mut().for_each(|v| *v *= amount);
                }
                p
            }
            Err(e) => {
                eprintln!("masks: skipping a {} Mask: {e}", mask.kind());
                continue;
            }
        };
        if !delta.is_tonally_neutral() {
            let fin = Finish::new(matrix, look, &global.plus(delta).clamped());
            out.par_chunks_mut(3)
                .zip(cam.par_chunks(3))
                .zip(&weight)
                .for_each(|((o, c), &k)| {
                    if k > 1e-3 {
                        let t = fin.px([c[0], c[1], c[2]]);
                        for i in 0..3 {
                            o[i] += (t[i] - o[i]) * k;
                        }
                    }
                });
        }
        if let Some(fx) = crate::effects::local_part(delta) {
            let mut e = out.clone();
            crate::effects::apply(&mut e, w, h, &fx, scale);
            out.par_chunks_mut(3)
                .zip(e.par_chunks(3))
                .zip(&weight)
                .for_each(|((o, t), &k)| {
                    for i in 0..3 {
                        o[i] += (t[i] - o[i]) * k;
                    }
                });
        }
        if delta.skin > 0.0 {
            add(&mut skin, &weight, (delta.skin / 100.0) as f32);
        }
        // Eyes and teeth inside a local follow the Mask loosely: a faces Mask covers the
        // skin around the eyes, not the eyes themselves.
        if delta.eyes > 0.0 || delta.teeth > 0.0 {
            let r = (w.max(h) / 60).max(1);
            let near: Vec<f32> = refine::box_blur(&weight, w, h, r)
                .into_iter()
                .map(|v| (v * 4.0).min(1.0))
                .collect();
            if let (Some(e), true) = (&eye_plane, delta.eyes > 0.0) {
                let gated: Vec<f32> = e.iter().zip(&near).map(|(a, b)| a * b).collect();
                add(&mut eyes, &gated, (delta.eyes / 100.0) as f32 * amount);
            }
            if let (Some(m), true) = (&mouth_plane, delta.teeth > 0.0) {
                let gated: Vec<f32> = m.iter().zip(&near).map(|(a, b)| a * b).collect();
                add(&mut teeth, &gated, (delta.teeth / 100.0) as f32 * amount);
            }
        }
    }
    if g.skin > 0.0 {
        if let Ok(p) = canvas.plane(&Mask::new(Shape::Skin)) {
            add(&mut skin, &p, (g.skin / 100.0) as f32);
        }
    }
    if let (Some(e), true) = (&eye_plane, g.eyes > 0.0) {
        add(&mut eyes, e, (g.eyes / 100.0) as f32);
    }
    if let (Some(m), true) = (&mouth_plane, g.teeth > 0.0) {
        add(&mut teeth, m, (g.teeth / 100.0) as f32);
    }

    let face_px = analysis
        .and_then(Analysis::face_width)
        .map(|f| f * w as f32)
        .unwrap_or(w.max(h) as f32 / 8.0);
    if skin.iter().any(|&v| v > 0.0) {
        smooth_skin(&mut out, w, h, &skin, face_px);
    }
    if eyes.iter().any(|&v| v > 0.0) {
        brighten_eyes(&mut out, w, h, &eyes, face_px);
    }
    if teeth.iter().any(|&v| v > 0.0) {
        whiten_teeth(&mut out, &teeth);
    }
    crate::effects::apply(&mut out, w, h, &global, scale);
    out.into_par_iter().map(to8).collect()
}

/// Lens Blur ([`crate::lens_blur`]) over the upright camera RGB, before anything else, with
/// the Photo's depth map and subject, or ones found in the image itself when there's no
/// Analysis (a render with no Photo behind it). None when there's no depth to go on.
fn lens_blur(
    cam: &[f32],
    (w, h): (usize, usize),
    matrix: [[f32; 3]; 3],
    look: &Look,
    recipe: &Recipe,
    analysis: Option<&Analysis>,
) -> Option<Vec<f32>> {
    use crate::lens_blur as lb;
    use crate::masks::models::{self, SEG_SIZE};
    let blur = &recipe.lens_blur;
    // An edge guide for snapping depth and the subject to the photo. Luma with a rough
    // display gamma is all a guide needs, in one cheap pass: the finished image would cost a
    // whole render of the full-size frame.
    let guide: Vec<f32> = cam
        .par_chunks(3)
        .map(|p| {
            (LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2])
                .max(0.0)
                .sqrt()
                .min(1.0)
        })
        .collect();
    // The image as it will look, small, for the models when there's no Analysis.
    let small = std::sync::OnceLock::new();
    let small = || {
        small.get_or_init(|| {
            let fin = Finish::new(matrix, look, &recipe.render_adjust());
            let f = (w.max(h) / (2 * crate::analyzers::SMALL_EDGE)).max(1);
            let (sw, sh) = (w / f, h / f);
            let data = (0..sw * sh)
                .into_par_iter()
                .flat_map_iter(|i| {
                    let (x0, y0) = ((i % sw) * f, (i / sw) * f);
                    let mut a = [0f32; 3];
                    for y in y0..y0 + f {
                        for x in x0..x0 + f {
                            let j = (y * w + x) * 3;
                            a = [a[0] + cam[j], a[1] + cam[j + 1], a[2] + cam[j + 2]];
                        }
                    }
                    let n = (f * f) as f32;
                    fin.px([a[0] / n, a[1] / n, a[2] / n]).map(to8)
                })
                .collect();
            let img = RgbImage {
                width: sw,
                height: sh,
                data,
            };
            crate::masks::shrink(&img, crate::analyzers::SMALL_EDGE)
        })
    };
    let depth = match analysis.and_then(|a| a.depth.clone()) {
        Some(d) => d,
        None => match crate::analyzers::depth::estimate(small()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("lens blur: no depth map ({e}); rendering without it");
                return None;
            }
        },
    };
    // Focus on the subject: the people the segmenter finds, kept sharp edge to edge.
    let subject: Option<Vec<f32>> = if blur.focus.is_none() {
        let bg = match analysis {
            Some(a) => Some(a.class("background").to_vec()),
            None => models::segment(small()).ok().map(|s| s[0].clone()),
        };
        bg.map(|b| b.iter().map(|v| 1.0 - v).collect())
    } else {
        None
    };
    let n = SEG_SIZE;
    let range = lb::focus_range(
        blur,
        &depth,
        subject
            .as_ref()
            .map(|s| refine::resize(s, n, n, depth.width, depth.height))
            .as_deref(),
    );
    let canvas = Canvas {
        width: w,
        height: h,
        guide: &guide,
        rgb: None,
        analysis: None,
    };
    let brush = |dots: &[crate::masks::Dot]| {
        (!dots.is_empty())
            .then(|| {
                canvas
                    .plane(&Mask::new(Shape::Brush {
                        dots: dots.to_vec(),
                        erase: Vec::new(),
                    }))
                    .ok()
            })
            .flatten()
    };
    let mut keep = subject.map(|s| refine::refine(&s, n, n, &guide, w, h, 1.5 / n as f32));
    if let Some(f) = brush(&blur.focus_brush) {
        keep = Some(match keep {
            Some(k) => k.iter().zip(&f).map(|(a, b)| a.max(*b)).collect(),
            None => f,
        });
    }
    let push = brush(&blur.blur_brush);
    let full = lb::depth_at_size(&depth, &guide, w, h);
    let defocus = lb::defocus(&full, range, keep.as_deref(), push.as_deref());
    let out = lb::apply(
        cam,
        w,
        h,
        &lb::Plan {
            blur,
            defocus: &defocus,
        },
    );
    Some(out)
}

fn channel(img: &[f32], c: usize) -> Vec<f32> {
    img.par_chunks(3).map(|p| p[c]).collect()
}

/// Natural skin smoothing, frequency-separation style: an edge-preserving smooth (guided
/// filter, radius from the face size) evens out blotches and blemishes while edges (eyes,
/// lips, jaw) stay; some of the finest detail (pores) is put back so skin keeps its texture.
/// `amount` is 0..1 per pixel; 1 is the strongest that still looks like skin.
pub fn smooth_skin(img: &mut [f32], w: usize, h: usize, amount: &[f32], face_px: f32) {
    let r = ((face_px / 22.0).round() as usize).clamp(1, 64);
    let fine_r = ((face_px / 220.0).round() as usize).max(1);
    let planes: Vec<Vec<f32>> = (0..3)
        .into_par_iter()
        .map(|c| {
            let ch = channel(img, c);
            let smooth = refine::guided_filter(&ch, &ch, w, h, r, 0.004);
            let fine = refine::box_blur(&ch, w, h, fine_r);
            ch.iter()
                .zip(smooth)
                .zip(fine)
                .map(|((&v, s), f)| s + 0.55 * (v - f))
                .collect()
        })
        .collect();
    img.par_chunks_mut(3).enumerate().for_each(|(i, p)| {
        let k = (amount[i] * 0.9).clamp(0.0, 0.9);
        if k > 0.0 {
            for (c, v) in p.iter_mut().enumerate() {
                *v += (planes[c][i] - *v) * k;
            }
        }
    });
}

/// Brighter, crisper eyes: a little lift on the whites and irises plus local contrast
/// (unsharp mask at a radius from the face size). `amount` is 0..1 per pixel.
pub fn brighten_eyes(img: &mut [f32], w: usize, h: usize, amount: &[f32], face_px: f32) {
    let r = ((face_px / 60.0).round() as usize).max(1);
    let blurred: Vec<Vec<f32>> = (0..3)
        .into_par_iter()
        .map(|c| refine::box_blur(&channel(img, c), w, h, r))
        .collect();
    img.par_chunks_mut(3).enumerate().for_each(|(i, p)| {
        let k = amount[i].clamp(0.0, 1.0);
        if k > 0.0 {
            for (c, v) in p.iter_mut().enumerate() {
                let detail = *v - blurred[c][i];
                let lifted = *v + 0.08 * k * (1.0 - *v) + 0.9 * k * detail;
                *v = lifted;
            }
        }
    });
}

/// Whiter teeth: inside the mouth, bright low-saturation pixels (teeth, not lips or
/// shadow) lose their yellow and lift a little. `amount` is 0..1 per pixel.
pub fn whiten_teeth(img: &mut [f32], amount: &[f32]) {
    img.par_chunks_mut(3).enumerate().for_each(|(i, p)| {
        let k = amount[i].clamp(0.0, 1.0);
        if k <= 0.0 {
            return;
        }
        let l = LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2];
        let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
        let sat = if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        // Teeth: bright and fairly neutral; lips are saturated red, the mouth's inside dark.
        let tooth = crate::masks::smoothstep(0.35, 0.55, l)
            * (1.0 - crate::masks::smoothstep(0.16, 0.3, sat));
        let t = k * tooth;
        if t > 0.0 {
            for v in p.iter_mut() {
                let desat = l + (*v - l) * (1.0 - 0.7 * t);
                *v = desat + 0.07 * t * (1.0 - desat);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skin_smoothing_evens_blotches_and_keeps_strong_edges() {
        let (w, h) = (80, 40);
        // Left: skin tone with blotches; right: a dark edge (eyelash line).
        let mut img = vec![0f32; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                let blotch = if (x / 3 + y / 3) % 2 == 0 {
                    0.03
                } else {
                    -0.03
                };
                let v = if x < 60 { 0.7 + blotch } else { 0.15 };
                img[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[v, v * 0.8, v * 0.7]);
            }
        }
        let var = |img: &[f32]| {
            let vals: Vec<f32> = (10..30)
                .flat_map(|y| (5..50).map(move |x| (y, x)))
                .map(|(y, x)| img[(y * w + x) * 3])
                .collect();
            let m = vals.iter().sum::<f32>() / vals.len() as f32;
            vals.iter().map(|v| (v - m).powi(2)).sum::<f32>() / vals.len() as f32
        };
        let before = var(&img);
        let mut out = img.clone();
        smooth_skin(&mut out, w, h, &vec![1.0; w * h], 120.0);
        assert!(var(&out) < before * 0.5, "{} -> {}", before, var(&out));
        // The edge stays put: one pixel either side keeps its side's tone.
        assert!(out[(20 * w + 58) * 3] > 0.55 && out[(20 * w + 62) * 3] < 0.3);
        // No amount, no change.
        let mut same = img.clone();
        smooth_skin(&mut same, w, h, &vec![0.0; w * h], 120.0);
        assert_eq!(same, img);
    }

    #[test]
    fn teeth_whitening_leaves_lips_and_skin_alone() {
        let mut px = vec![
            0.85, 0.80, 0.68, /* lips */ 0.70, 0.25, 0.25, /* skin */ 0.85, 0.65, 0.55,
        ];
        whiten_teeth(&mut px, &[1.0, 1.0, 1.0]);
        let sat = |p: &[f32]| {
            let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
            (mx - mn) / mx
        };
        assert!(sat(&px[0..3]) < 0.12, "tooth desaturated: {:?}", &px[0..3]);
        assert!(
            (px[3] - 0.70).abs() < 1e-6 && (px[4] - 0.25).abs() < 1e-6,
            "lips untouched"
        );
        assert!(
            (px[7] - 0.65).abs() < 1e-6,
            "skin untouched: {:?}",
            &px[6..9]
        );
    }
}
