//! AI Denoise, Lightroom's Denoise: a learned denoiser runs once over a RAW's full-size
//! demosaiced image and the result is kept as the Photo's denoised base. The Recipe's
//! `denoise` Amount (0..100, 0 = off) blends the base with the plain demosaic at render time,
//! so moving the slider is as cheap as any other.
//!
//! The base is computed once per Photo ([`compute`], by the `denoise` Queue job, `denoise_photo`
//! or an export that needs it) and kept in the Library's cache folder ([`base_file`]), so
//! Clear cache drops it and it comes back on demand. Until it exists, a Photo renders as if
//! Denoise were off. RAW Photos only, as in Lightroom.
//!
//! The model is SCUNet's blind real-noise model (`scunet_color_real_psnr`, Apache-2.0, see
//! `engine/models/manifest.json`), run through tract like every other model (ADR 0013) in
//! overlapping tiles. It was trained on sRGB images, so the linear camera RGB of stage 3 is
//! brightened to use the range and sRGB-encoded on the way in, and decoded on the way out.

use crate::analyzers::runtime::{floats, ndarray, Lazy, Tensor};
use crate::library::{Photo, CACHE_DIR};
use crate::pipeline::{
    demosaic_bilinear, normalize_white_balance, resize_area, srgb_decode, srgb_encode,
};
use crate::recipe::Recipe;
use crate::{Error, Result};
use rayon::prelude::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Bump when the base would come out different, so stale bases are recomputed.
pub const VERSION: u32 = 1;

/// Tile size the model was exported at, and how far tiles overlap so their seams blend.
const TILE: usize = 256;
const OVERLAP: usize = 32;

static MODEL: Lazy = Lazy::new(
    include_bytes!("../models/scunet_color_real_f16.onnx"),
    &[1, 3, TILE, TILE],
);

/// The Recipe's Denoise Amount as a blend weight, 0 (off) to 1. See [`weight`].
pub fn amount(recipe: &Recipe) -> f32 {
    weight(recipe.global.more("denoise"))
}

/// Blend weight for an Amount of 0..100: 1 - (1 - a)^2, so the middle of the slider already
/// takes three quarters of the noise out (a straight blend left Amount 50 visibly grainy,
/// where Lightroom's 50 is clean) and the top end eases into the model's full, smoother look.
pub fn weight(amount: f64) -> f32 {
    let a = (amount / 100.0).clamp(0.0, 1.0) as f32;
    1.0 - (1.0 - a) * (1.0 - a)
}

/// Where `photo`'s denoised base lives: `cache/denoise/` in its Library, named by the
/// original's content hash (which also names its preview). None for non-RAW Photos.
pub fn base_file(photo: &Photo) -> Option<PathBuf> {
    if photo.kind != "raw" {
        return None;
    }
    let hash = photo.preview.file_name()?.to_str()?.split('.').next()?;
    let root = photo.preview.parent()?.parent()?.parent()?;
    Some(
        root.join(CACHE_DIR)
            .join("denoise")
            .join(format!("{hash}.v{VERSION}.bin")),
    )
}

/// The base file, if it has been computed.
pub fn ready(photo: &Photo) -> Option<PathBuf> {
    base_file(photo).filter(|f| f.is_file())
}

/// After a Recipe is saved: when it asks for Denoise and the Photo's base doesn't exist
/// yet, queue the `denoise` job, ahead of everything when the person set it (they are
/// looking at the Photo), else with other background work.
pub fn queue_if_needed(
    lib: &mut crate::library::Library,
    photo: crate::library::PhotoId,
    recipe: &serde_json::Value,
    author: crate::library::Author,
) -> Result<()> {
    let on = recipe["global"]["denoise"]
        .as_f64()
        .is_some_and(|v| v > 0.0);
    if !on {
        return Ok(());
    }
    let Some(p) = lib.photo(photo)? else {
        return Ok(());
    };
    if base_file(&p).is_none_or(|f| f.is_file()) {
        return Ok(());
    }
    let priority =
        (author == crate::library::Author::Person).then_some(crate::jobs::PRIORITY_FOCUS);
    crate::jobs::enqueue(
        lib,
        "denoise",
        &serde_json::json!({ "photo_id": photo }),
        priority,
        None,
    )?;
    Ok(())
}

/// Compute `photo`'s denoised base unless it exists: decode the RAW, demosaic at full size,
/// run the model, write the base. Returns its path. `progress(done, total)` counts tiles.
pub fn compute(photo: &Photo, progress: &dyn Fn(usize, usize)) -> Result<PathBuf> {
    let file = base_file(photo).ok_or_else(|| {
        Error(format!(
            "Photo {} is not a RAW, so it can't be denoised",
            photo.id
        ))
    })?;
    if file.is_file() {
        return Ok(file);
    }
    let raw = crate::decode(Path::new(&photo.path))?;
    let (w, h) = raw.crop_size();
    let rgb = demosaic_bilinear(&normalize_white_balance(&raw), w, h, raw.cfa);
    drop(raw);
    let out = denoise(&rgb, w, h, progress)?;
    drop(rgb);
    save(&file, &out, w, h)?;
    Ok(file)
}

const MAGIC: &[u8; 8] = b"ARGUSDN1";

/// Write a base: magic, width, height, then deflated linear RGB as little-endian u16.
/// Written beside and renamed, so a killed write never leaves half a base.
fn save(file: &Path, rgb: &[f32], w: usize, h: usize) -> Result<()> {
    let io = |e: std::io::Error| Error(format!("{}: {e}", file.display()));
    let dir = file.parent().expect("base file has a folder");
    std::fs::create_dir_all(dir).map_err(io)?;
    let tmp = file.with_extension("part");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(io)?);
    out.write_all(MAGIC).map_err(io)?;
    out.write_all(&(w as u32).to_le_bytes()).map_err(io)?;
    out.write_all(&(h as u32).to_le_bytes()).map_err(io)?;
    let mut z = flate2::write::DeflateEncoder::new(out, flate2::Compression::fast());
    for row in rgb.chunks(w * 3) {
        let bytes: Vec<u8> = row
            .iter()
            .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes())
            .collect();
        z.write_all(&bytes).map_err(io)?;
    }
    z.finish().map_err(io)?.flush().map_err(io)?;
    std::fs::rename(&tmp, file).map_err(io)
}

/// Read a base back as linear RGB at full size: (rgb, width, height).
fn load(file: &Path) -> Result<(Vec<f32>, usize, usize)> {
    let io = |e: std::io::Error| Error(format!("denoised base {}: {e}", file.display()));
    let mut f = std::io::BufReader::new(std::fs::File::open(file).map_err(io)?);
    let mut head = [0u8; 16];
    f.read_exact(&mut head).map_err(io)?;
    if &head[..8] != MAGIC {
        return Err(Error(format!("{} is not a denoised base", file.display())));
    }
    let w = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
    let h = u32::from_le_bytes(head[12..16].try_into().unwrap()) as usize;
    let mut z = flate2::read::DeflateDecoder::new(f);
    let mut rgb = vec![0f32; w * h * 3];
    let mut row = vec![0u8; w * 6];
    for out in rgb.chunks_mut(w * 3) {
        z.read_exact(&mut row).map_err(io)?;
        for (o, b) in out.iter_mut().zip(row.as_chunks::<2>().0) {
            *o = u16::from_le_bytes(*b) as f32 / 65535.0;
        }
    }
    Ok((rgb, w, h))
}

/// A base at the size [`crate::Demosaiced::new`] makes for `long_edge` (None = full size),
/// pixel for pixel: halved by 2x2 quads when that path halves, then area-resized.
pub fn base_at(file: &Path, long_edge: Option<usize>) -> Result<(Vec<f32>, usize, usize)> {
    let (rgb, w, h) = load(file)?;
    let Some(t) = long_edge.map(|t| t.max(1)) else {
        return Ok((rgb, w, h));
    };
    if w.max(h) <= t {
        return Ok((rgb, w, h));
    }
    let (rgb, w, h) = if t * 2 <= w.max(h) {
        halve(&rgb, w, h)
    } else {
        (rgb, w, h)
    };
    Ok(if w.max(h) > t {
        resize_area(&rgb, w, h, t)
    } else {
        (rgb, w, h)
    })
}

/// 2x2 box average to (w/2, h/2), the grid of [`crate::pipeline::demosaic_half`].
fn halve(rgb: &[f32], w: usize, h: usize) -> (Vec<f32>, usize, usize) {
    let (ow, oh) = (w / 2, h / 2);
    let mut out = vec![0f32; ow * oh * 3];
    out.par_chunks_mut(ow * 3)
        .enumerate()
        .for_each(|(y, orow)| {
            let (r0, r1) = (&rgb[y * 2 * w * 3..], &rgb[(y * 2 + 1) * w * 3..]);
            for (x, px) in orow.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                for (c, v) in px.iter_mut().enumerate() {
                    let i = x * 6 + c;
                    *v = (r0[i] + r0[i + 3] + r1[i] + r1[i + 3]) * 0.25;
                }
            }
        });
    (out, ow, oh)
}

/// Lightroom's Amount: blend the denoised image over the plain one, in place.
pub fn blend(noisy: &mut [f32], denoised: &[f32], amount: f32) {
    noisy
        .par_iter_mut()
        .zip(denoised.par_iter())
        .for_each(|(n, d)| *n += amount * (d - *n));
}

/// How many model tiles a `w` x `h` image takes (for progress and time estimates).
pub fn tiles(w: usize, h: usize) -> usize {
    starts(w).len() * starts(h).len()
}

/// Denoise interleaved linear RGB (`w * h * 3`, 0..1). `progress(done, total)` is called
/// after each row of tiles.
pub fn denoise(
    rgb: &[f32],
    w: usize,
    h: usize,
    progress: &dyn Fn(usize, usize),
) -> Result<Vec<f32>> {
    if rgb.len() != w * h * 3 || w == 0 || h == 0 {
        return Err(Error(format!(
            "denoise: {w}x{h} image with {} values",
            rgb.len()
        )));
    }
    if std::env::var_os("ARGUS_DENOISE_FAKE").is_some() {
        return Ok(fake(rgb, w, h));
    }
    MODEL.get()?;
    let gain = gain(rgb);
    let xs = starts(w);
    let ys = starts(h);
    let total = xs.len() * ys.len();
    progress(0, total);
    let mut acc = vec![0f32; w * h * 3];
    let mut weight = vec![0f32; w * h];
    for (row, &y0) in ys.iter().enumerate() {
        let tiles: Vec<Vec<f32>> = xs
            .par_iter()
            .map(|&x0| run_tile(rgb, w, h, x0, y0, gain))
            .collect::<Result<_>>()?;
        for (&x0, out) in xs.iter().zip(&tiles) {
            for ty in 0..TILE {
                let Some(y) = inside(y0 + ty, h) else {
                    continue;
                };
                let wy = ramp(ty);
                for tx in 0..TILE {
                    let Some(x) = inside(x0 + tx, w) else {
                        continue;
                    };
                    let k = wy * ramp(tx);
                    let p = y * w + x;
                    weight[p] += k;
                    for c in 0..3 {
                        acc[p * 3 + c] += k * out[c * TILE * TILE + ty * TILE + tx];
                    }
                }
            }
        }
        progress((row + 1) * xs.len(), total);
    }
    for (p, &k) in weight.iter().enumerate() {
        for c in 0..3 {
            let i = p * 3 + c;
            let v = srgb_decode((acc[i] / k).clamp(0.0, 1.0)) / gain;
            // Where the brightened input clipped, the model never saw the real value.
            acc[i] = if rgb[i] * gain >= 0.999 { rgb[i] } else { v };
        }
    }
    Ok(acc)
}

/// `ARGUS_DENOISE_FAKE=1` (tests, UI checks): a 5x5 box blur in place of the model, so a
/// whole frame "denoises" in a second instead of minutes on a cloud CPU.
fn fake(rgb: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0f32; rgb.len()];
    out.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut sum = [0f32; 3];
            let mut n = 0.0;
            for yy in y.saturating_sub(2)..(y + 3).min(h) {
                for xx in x.saturating_sub(2)..(x + 3).min(w) {
                    for c in 0..3 {
                        sum[c] += rgb[(yy * w + xx) * 3 + c];
                    }
                    n += 1.0;
                }
            }
            for c in 0..3 {
                row[x * 3 + c] = sum[c] / n;
            }
        }
    });
    out
}

/// Tiles run over the image padded by OVERLAP on every side (mirrored), so no tile edge,
/// where the model sees least context, falls on the picture. Tile origins in that padded
/// frame: TILE wide, stepping TILE - OVERLAP, the last one flush with the far edge.
fn starts(n: usize) -> Vec<usize> {
    let padded = n + 2 * OVERLAP;
    if padded <= TILE {
        return vec![0];
    }
    let step = TILE - OVERLAP;
    let mut v: Vec<usize> = (0..)
        .map(|i| i * step)
        .take_while(|&s| s + TILE < padded)
        .collect();
    v.push(padded - TILE);
    v
}

/// The image coordinate of padded coordinate `v`, if it is on the picture.
fn inside(v: usize, n: usize) -> Option<usize> {
    v.checked_sub(OVERLAP).filter(|&i| i < n)
}

/// The image coordinate to sample for padded coordinate `v`: mirrored past the edges.
fn mirror(v: usize, n: usize) -> usize {
    let i = v as isize - OVERLAP as isize;
    let period = 2 * n.max(1) as isize;
    let m = i.rem_euclid(period);
    (if m < n as isize { m } else { period - 1 - m }) as usize
}

/// Blend weight of tile pixel `t`: a linear ramp over the overlap at both ends, so
/// neighbouring tiles fade into each other. Every picture pixel lies at least OVERLAP
/// inside some tile, so its weights never sum to zero.
fn ramp(t: usize) -> f32 {
    let d = t.min(TILE - 1 - t) as f32 + 0.5;
    (d / OVERLAP as f32).clamp(1e-3, 1.0)
}

/// Brighten dark frames so the model sees the noise at a level it was trained on: the
/// 99.9th percentile of the brightest channel goes to 0.9, never darker, at most 16x.
fn gain(rgb: &[f32]) -> f32 {
    let mut hist = [0u32; 1024];
    let mut n = 0u32;
    for px in rgb.as_chunks::<3>().0.iter().step_by(7) {
        let m = px[0].max(px[1]).max(px[2]).clamp(0.0, 1.0);
        hist[(m * 1023.0) as usize] += 1;
        n += 1;
    }
    let mut seen = 0;
    let cut = n - n / 1000;
    let p = hist
        .iter()
        .position(|&c| {
            seen += c;
            seen >= cut
        })
        .unwrap_or(1023) as f32
        / 1023.0;
    (0.9 / p.max(1e-4)).clamp(1.0, 16.0)
}

/// One tile through the model: planar 0..1 sRGB out, `3 * TILE * TILE`.
fn run_tile(rgb: &[f32], w: usize, h: usize, x0: usize, y0: usize, gain: f32) -> Result<Vec<f32>> {
    let input = ndarray::Array4::from_shape_fn((1, 3, TILE, TILE), |(_, c, y, x)| {
        let p = mirror(y0 + y, h) * w + mirror(x0 + x, w);
        srgb_encode((rgb[p * 3 + c] * gain).clamp(0.0, 1.0))
    });
    let out = MODEL.run(Tensor::from(input))?;
    let v = floats(&out[0])?;
    if v.len() != 3 * TILE * TILE {
        return Err(Error(format!("denoise model gave {} values", v.len())));
    }
    Ok(v.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_cover_every_pixel_and_end_flush() {
        for n in [1, 100, 192, 193, 256, 480, 481, 1000, 9504] {
            let s = starts(n);
            assert_eq!(*s.first().unwrap(), 0);
            if n + 2 * OVERLAP > TILE {
                assert_eq!(*s.last().unwrap(), n + 2 * OVERLAP - TILE);
            }
            for pair in s.windows(2) {
                assert!(pair[1] - pair[0] <= TILE - OVERLAP, "{n}: {s:?}");
            }
            // Every picture pixel is weighted mostly by tiles that see well around it.
            for i in 0..n {
                let v = i + OVERLAP;
                let best = s
                    .iter()
                    .filter(|&&a| v >= a && v < a + TILE)
                    .map(|&a| ramp(v - a))
                    .sum::<f32>();
                assert!(best >= 0.9, "{n}: pixel {i} only near a tile edge");
            }
        }
        assert_eq!(
            (
                mirror(0, 100),
                mirror(OVERLAP, 100),
                mirror(OVERLAP + 100, 100)
            ),
            (OVERLAP - 1, 0, 99)
        );
    }

    #[test]
    fn gain_brightens_dark_frames_only() {
        assert_eq!(gain(&[0.95; 300]), 1.0);
        assert!((gain(&[0.09; 300]) - 10.0).abs() < 0.2);
        assert_eq!(gain(&[0.0; 300]), 16.0);
    }

    #[test]
    fn base_round_trips_and_matches_the_half_size_grid() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache/denoise/x.v1.bin");
        let (w, h) = (9, 7);
        let rgb: Vec<f32> = (0..w * h * 3).map(|i| (i % 50) as f32 / 49.0).collect();
        save(&file, &rgb, w, h).unwrap();
        let (back, bw, bh) = base_at(&file, None).unwrap();
        assert_eq!((bw, bh), (w, h));
        assert!(back.iter().zip(&rgb).all(|(a, b)| (a - b).abs() < 1e-4));
        // Half-size path: same dims as demosaic_half (w/2, h/2), then no resize needed.
        let (_, hw, hh) = base_at(&file, Some(4)).unwrap();
        assert_eq!((hw, hh), (4, 3));
        assert_eq!((weight(0.0), weight(50.0), weight(100.0)), (0.0, 0.75, 1.0));
        let mut n = vec![0.0, 1.0];
        blend(&mut n, &[1.0, 0.0], 0.25);
        assert_eq!(n, vec![0.25, 0.75]);
    }

    /// The model removes most of the noise from a noisy gradient and keeps the gradient.
    #[test]
    fn model_removes_noise_and_keeps_the_picture() {
        let (w, h) = (300, 260);
        let mut seed = 7u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        let clean: Vec<f32> = (0..w * h * 3)
            .map(|i| 0.05 + 0.4 * ((i / 3) % w) as f32 / w as f32)
            .collect();
        let noisy: Vec<f32> = clean
            .iter()
            .map(|v| (v + noise() * 0.06).max(0.0))
            .collect();
        let t = std::time::Instant::now();
        let out = denoise(&noisy, w, h, &|_, _| {}).unwrap();
        eprintln!("{} tiles in {:?}", tiles(w, h), t.elapsed());
        let err = |a: &[f32]| {
            (a.iter()
                .zip(&clean)
                .map(|(x, y)| (x - y).powi(2))
                .sum::<f32>()
                / a.len() as f32)
                .sqrt()
        };
        let (before, after) = (err(&noisy), err(&out));
        eprintln!("rms error {before:.4} -> {after:.4}");
        assert!(after < before * 0.4, "{before} -> {after}");
    }
}
