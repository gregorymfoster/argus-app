//! Plane operations Masks are made of: resize, box blur and an edge-aware guided filter.
//! A plane is one f32 per pixel, row-major.

use rayon::prelude::*;

/// Bilinear resize of a plane, sampling at pixel centres.
pub fn resize(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    if (sw, sh) == (dw, dh) {
        return src.to_vec();
    }
    let mut out = vec![0f32; dw * dh];
    let sx = sw as f32 / dw as f32;
    let sy = sh as f32 / dh as f32;
    out.par_chunks_mut(dw).enumerate().for_each(|(y, row)| {
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (sh - 1) as f32);
        let y0 = fy as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let ty = fy - y0 as f32;
        let (r0, r1) = (&src[y0 * sw..(y0 + 1) * sw], &src[y1 * sw..(y1 + 1) * sw]);
        for (x, o) in row.iter_mut().enumerate() {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (sw - 1) as f32);
            let x0 = fx as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let tx = fx - x0 as f32;
            let a = r0[x0] + (r0[x1] - r0[x0]) * tx;
            let b = r1[x0] + (r1[x1] - r1[x0]) * tx;
            *o = a + (b - a) * ty;
        }
    });
    out
}

/// Mean over a (2r+1)² box, clamped at the borders (each output divides by the pixels
/// actually covered). Separable running sums: O(pixels) whatever the radius.
pub fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r == 0 {
        return src.to_vec();
    }
    let mut tmp = vec![0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        blur_line(&src[y * w..(y + 1) * w], row, r);
    });
    // Vertical pass on the transpose keeps memory access sequential.
    let t = transpose(&tmp, w, h);
    let mut tv = vec![0f32; w * h];
    tv.par_chunks_mut(h).enumerate().for_each(|(x, col)| {
        blur_line(&t[x * h..(x + 1) * h], col, r);
    });
    transpose(&tv, h, w)
}

fn blur_line(src: &[f32], out: &mut [f32], r: usize) {
    let n = src.len();
    let mut prefix = Vec::with_capacity(n + 1);
    let mut acc = 0f64;
    prefix.push(0f64);
    for &v in src {
        acc += v as f64;
        prefix.push(acc);
    }
    for (i, o) in out.iter_mut().enumerate() {
        let a = i.saturating_sub(r);
        let b = (i + r + 1).min(n);
        *o = ((prefix[b] - prefix[a]) / (b - a) as f64) as f32;
    }
}

fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(h).enumerate().for_each(|(x, col)| {
        for (y, o) in col.iter_mut().enumerate() {
            *o = src[y * w + x];
        }
    });
    out
}

/// He et al.'s guided filter: smooths `p` while snapping its edges to the edges of
/// `guide` (both w x h). `eps` sets how strong a guide edge must be to be kept.
pub fn guided_filter(guide: &[f32], p: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let mul = |a: &[f32], b: &[f32]| a.par_iter().zip(b).map(|(x, y)| x * y).collect::<Vec<_>>();
    let mean_i = box_blur(guide, w, h, r);
    let mean_p = box_blur(p, w, h, r);
    let corr_ii = box_blur(&mul(guide, guide), w, h, r);
    let corr_ip = box_blur(&mul(guide, p), w, h, r);
    let (a, b): (Vec<f32>, Vec<f32>) = (0..w * h)
        .into_par_iter()
        .map(|i| {
            let var = corr_ii[i] - mean_i[i] * mean_i[i];
            let cov = corr_ip[i] - mean_i[i] * mean_p[i];
            let a = cov / (var + eps);
            (a, mean_p[i] - a * mean_i[i])
        })
        .unzip();
    let (ma, mb) = (box_blur(&a, w, h, r), box_blur(&b, w, h, r));
    (0..w * h)
        .into_par_iter()
        .map(|i| ma[i] * guide[i] + mb[i])
        .collect()
}

/// Largest edge refinement runs at; bigger renders refine at this size and upsample, which
/// keeps a 42 MP export fast and loses nothing a mask edge needs.
pub const REFINE_EDGE: usize = 2048;

/// Refine a coarse mask (any size) against a full-size guide: upsample, guided filter,
/// clamp to 0..1. `r_frac` is the filter radius as a fraction of the long edge.
pub fn refine(
    coarse: &[f32],
    cw: usize,
    ch: usize,
    guide: &[f32],
    w: usize,
    h: usize,
    r_frac: f32,
) -> Vec<f32> {
    let s = (REFINE_EDGE as f32 / w.max(h) as f32).min(1.0);
    let (rw, rh) = (
        ((w as f32 * s).round() as usize).max(1),
        ((h as f32 * s).round() as usize).max(1),
    );
    let g = if (rw, rh) == (w, h) {
        guide.to_vec()
    } else {
        resize(guide, w, h, rw, rh)
    };
    let p = resize(coarse, cw, ch, rw, rh);
    let r = ((rw.max(rh) as f32 * r_frac).round() as usize).max(1);
    let mut q = guided_filter(&g, &p, rw, rh, r, 1e-3);
    q.par_iter_mut().for_each(|v| *v = v.clamp(0.0, 1.0));
    if (rw, rh) == (w, h) {
        q
    } else {
        resize(&q, rw, rh, w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_blur_keeps_constants_and_averages_steps() {
        let (w, h) = (9, 5);
        let flat = vec![0.5f32; w * h];
        assert!(box_blur(&flat, w, h, 3)
            .iter()
            .all(|v| (v - 0.5).abs() < 1e-6));
        let step: Vec<f32> = (0..w * h)
            .map(|i| if i % w < 4 { 0.0 } else { 1.0 })
            .collect();
        let b = box_blur(&step, w, h, 1);
        assert!((b[3] - 1.0 / 3.0).abs() < 1e-6 && (b[4] - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn guided_filter_snaps_a_blurry_mask_to_the_guide_edge() {
        let (w, h) = (64, 8);
        let guide: Vec<f32> = (0..w * h)
            .map(|i| if i % w < 32 { 0.1 } else { 0.9 })
            .collect();
        // A soft ramp centred on the edge.
        let p: Vec<f32> = (0..w * h)
            .map(|i| ((i % w) as f32 - 24.0) / 16.0)
            .map(|v| v.clamp(0.0, 1.0))
            .collect();
        let q = guided_filter(&guide, &p, w, h, 4, 1e-4);
        assert!(q[28] < 0.3 && q[36] > 0.7, "{} {}", q[28], q[36]);
    }

    #[test]
    fn resize_is_identity_at_same_size_and_keeps_ramps() {
        let src: Vec<f32> = (0..16).map(|v| v as f32).collect();
        assert_eq!(resize(&src, 4, 4, 4, 4), src);
        let up = resize(&[0.0, 1.0], 2, 1, 4, 1);
        assert!(up.windows(2).all(|p| p[0] <= p[1]));
    }
}
