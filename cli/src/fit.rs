#![allow(
    unknown_lints,
    clippy::needless_range_loop,
    clippy::chunks_exact_to_as_chunks
)] // dev tool, clarity over lint-golf

//! `argus fit-look`: dev tool that fits the engine's default [`Look`] to the camera's own
//! embedded JPEGs. Not on the render hot path.
//!
//! For each RAW we develop once to linear sRGB at the JPEG's size, align the two images
//! (centre crop plus a tiny integer-shift search, reported), then evaluate a parametric
//! Look on a subsample of pixels and minimise one shared objective across all fixtures with
//! Nelder-Mead. Objective per fixture: mean-over-channels MAE (0..255) + 0.5 * mean CIELAB
//! dE76; the total is the mean over fixtures.

use argus_engine::pipeline::{color_tone, Finish, Look, ToneSliders};
use argus_engine::{decode, develop_linear, embedded_preview};
use std::path::{Path, PathBuf};

const IDENTITY: [[f32; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
/// Fitting works on BLOCK x BLOCK box averages (in linear light) of both images, so sharpening,
/// noise reduction and sub-pixel misalignment in the camera JPEG do not drive the fit.
const BLOCK: usize = 8;

struct Fixture {
    name: String,
    /// Aligned, centre-cropped linear sRGB and camera JPEG, both `w x h`.
    w: usize,
    h: usize,
    lin: Vec<f32>,
    jpg: Vec<u8>,
    /// Subsampled copies (every STRIDE-th pixel in x and y) used while fitting.
    sub_w: usize,
    sub_lin: Vec<f32>,
    sub_jpg: Vec<u8>,
    sub_lab: Vec<[f32; 3]>,
    shift: [f64; 4],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Err {
    pub mae: [f64; 3],
    pub de: f64,
}
impl Err {
    fn mae_avg(&self) -> f64 {
        self.mae.iter().sum::<f64>() / 3.0
    }
    fn objective(&self) -> f64 {
        self.mae_avg() + 0.5 * self.de
    }
}

fn srgb_to_lab(rgb: [u8; 3]) -> [f32; 3] {
    static LIN: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let t = LIN.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    let lin = |v: u8| t[v as usize];
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.119192 * g + 0.9503041 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn measure(out: &[u8], target: &[u8], target_lab: Option<&[[f32; 3]]>) -> Err {
    let n = out.len() / 3;
    let mut sum = [0u64; 3];
    let mut de = 0f64;
    for (i, (a, b)) in out.chunks_exact(3).zip(target.chunks_exact(3)).enumerate() {
        for c in 0..3 {
            sum[c] += a[c].abs_diff(b[c]) as u64;
        }
        let la = srgb_to_lab([a[0], a[1], a[2]]);
        let lb = match target_lab {
            Some(t) => t[i],
            None => srgb_to_lab([b[0], b[1], b[2]]),
        };
        de += (((la[0] - lb[0]).powi(2) + (la[1] - lb[1]).powi(2) + (la[2] - lb[2]).powi(2))
            as f64)
            .sqrt();
    }
    Err {
        mae: sum.map(|s| s as f64 / n as f64),
        de: de / n as f64,
    }
}

fn render(lin: &[f32], w: usize, look: &Look) -> Vec<u8> {
    color_tone(lin, w, &Finish::plain(IDENTITY, look))
}

fn srgb_dec(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// BLOCK x BLOCK box average in linear light. Returns (linear f32, width, height).
fn block_avg(lin: &[f32], w: usize, h: usize) -> (Vec<f32>, usize, usize) {
    let (bw, bh) = (w / BLOCK, h / BLOCK);
    let mut out = vec![0f32; bw * bh * 3];
    for by in 0..bh {
        for bx in 0..bw {
            let mut acc = [0f32; 3];
            for y in by * BLOCK..(by + 1) * BLOCK {
                for x in bx * BLOCK..(bx + 1) * BLOCK {
                    for c in 0..3 {
                        acc[c] += lin[(y * w + x) * 3 + c];
                    }
                }
            }
            for c in 0..3 {
                out[(by * bw + bx) * 3 + c] = acc[c] / (BLOCK * BLOCK) as f32;
            }
        }
    }
    (out, bw, bh)
}

fn load_fixture(raw_path: &Path) -> Result<Fixture, Box<dyn std::error::Error>> {
    let name = raw_path.file_stem().unwrap().to_string_lossy().to_string();
    let jpg_bytes = embedded_preview(raw_path)?;
    let jpg = image::load_from_memory_with_format(&jpg_bytes, image::ImageFormat::Jpeg)?.to_rgb8();
    let (jw, jh) = (jpg.width() as usize, jpg.height() as usize);
    let raw = decode(raw_path)?;
    let lin = develop_linear(&raw, jw.max(jh));
    let (lw, lh) = (lin.width, lin.height);
    eprintln!("{name}: jpeg {jw}x{jh}, ours {lw}x{lh}");
    if (lw > lh) != (jw > jh) {
        return Err(format!("{name}: orientation mismatch between JPEG and render").into());
    }
    // The camera JPEG can cover a slightly different field of view than our crop (it is
    // generated from the full active area), so align with a small affine search: scale in
    // x and y plus translation, found by coordinate descent on tone-mapped error.
    const M: usize = 8; // border trimmed from the aligned images
    let (cw, ch) = (jw - 2 * M, jh - 2 * M);
    let probe = Look {
        exposure_ev: 0.6,
        shoulder_knee: 0.7,
        shadow_lift: 0.5,
        contrast: 0.5,
        saturation: 1.2,
        tone: ToneSliders::NONE,
    };
    let probe_fin = Finish::plain(IDENTITY, &probe);
    // Map JPEG pixel (x, y) -> our pixel via the images' centres.
    let sample = |p: &[f64; 4], x: usize, y: usize| -> Option<[f32; 3]> {
        let fx = (lw as f64 - 1.0) / 2.0 + (x as f64 - (jw as f64 - 1.0) / 2.0) * p[0] + p[2];
        let fy = (lh as f64 - 1.0) / 2.0 + (y as f64 - (jh as f64 - 1.0) / 2.0) * p[1] + p[3];
        if fx < 0.0 || fy < 0.0 || fx > lw as f64 - 1.001 || fy > lh as f64 - 1.001 {
            return None;
        }
        let (x0, y0) = (fx as usize, fy as usize);
        let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
        let at = |xx: usize, yy: usize, c: usize| lin.data[(yy * lw + xx) * 3 + c];
        Some(std::array::from_fn(|c| {
            let top = at(x0, y0, c) * (1.0 - tx) + at(x0 + 1, y0, c) * tx;
            let bot = at(x0, y0 + 1, c) * (1.0 - tx) + at(x0 + 1, y0 + 1, c) * tx;
            top * (1.0 - ty) + bot * ty
        }))
    };
    let align_err = |p: &[f64; 4]| -> f64 {
        let (mut lin_s, mut jpg_s) = (Vec::new(), Vec::new());
        for y in (M + 16..jh - M - 16).step_by(6) {
            for x in (M + 16..jw - M - 16).step_by(6) {
                if let Some(px) = sample(p, x, y) {
                    lin_s.extend_from_slice(&px);
                    jpg_s.extend_from_slice(&jpg.as_raw()[(y * jw + x) * 3..(y * jw + x) * 3 + 3]);
                }
            }
        }
        let n = lin_s.len() / 3;
        let o = color_tone(&lin_s, n, &probe_fin);
        measure(&o, &jpg_s, None).mae_avg()
    };
    let mut ap = [1.0f64, 1.0, 0.0, 0.0];
    let mut step = [0.004f64, 0.004, 3.0, 3.0];
    let mut cur = align_err(&ap);
    for _ in 0..40 {
        let mut improved = false;
        for i in 0..4 {
            for dir in [-1.0, 1.0] {
                let mut q = ap;
                q[i] += dir * step[i];
                let e = align_err(&q);
                if e < cur {
                    cur = e;
                    ap = q;
                    improved = true;
                }
            }
        }
        if !improved {
            for st in step.iter_mut() {
                *st *= 0.5;
            }
        }
    }
    let shift = ap;
    let mut l = Vec::with_capacity(cw * ch * 3);
    let mut j = Vec::with_capacity(cw * ch * 3);
    let (mut vx0, mut vx1, mut vy0, mut vy1) = (usize::MAX, 0, usize::MAX, 0);
    for y in M..jh - M {
        for x in M..jw - M {
            if sample(&ap, x, y).is_some() {
                vx0 = vx0.min(x);
                vx1 = vx1.max(x);
                vy0 = vy0.min(y);
                vy1 = vy1.max(y);
            }
        }
    }
    let (cw, ch) = (vx1 - vx0 + 1, vy1 - vy0 + 1);
    for y in vy0..=vy1 {
        for x in vx0..=vx1 {
            l.extend_from_slice(&sample(&ap, x, y).unwrap_or([0.0; 3]));
            j.extend_from_slice(&jpg.as_raw()[(y * jw + x) * 3..(y * jw + x) * 3 + 3]);
        }
    }
    let (sub_lin, sub_w, _) = block_avg(&l, cw, ch);
    let jlin: Vec<f32> = j.iter().map(|&v| srgb_dec(v as f32 / 255.0)).collect();
    let (jb, _, _) = block_avg(&jlin, cw, ch);
    let sub_jpg: Vec<u8> = jb
        .iter()
        .map(|&v| (argus_engine::pipeline::srgb_encode(v) * 255.0 + 0.5) as u8)
        .collect();
    let sub_lab = sub_jpg
        .chunks_exact(3)
        .map(|p| srgb_to_lab([p[0], p[1], p[2]]))
        .collect();
    Ok(Fixture {
        name,
        w: cw,
        h: ch,
        lin: l,
        jpg: j,
        sub_w,
        sub_lin,
        sub_jpg,
        sub_lab,
        shift,
    })
}

const N: usize = 5;
const NAMES: [&str; 5] = [
    "exposure_ev",
    "shoulder_knee",
    "shadow_lift",
    "contrast",
    "saturation",
];
const LO: [f64; 5] = [-0.5, 0.3, -0.5, -0.5, 0.5];
const HI: [f64; 5] = [2.5, 0.95, 3.0, 1.0, 2.0];

fn to_look(x: &[f64; N]) -> Look {
    let c = |i: usize| x[i].clamp(LO[i], HI[i]) as f32;
    Look {
        exposure_ev: c(0),
        shoulder_knee: c(1),
        shadow_lift: c(2),
        contrast: c(3),
        saturation: c(4),
        tone: ToneSliders::NONE,
    }
}
fn from_look(l: &Look) -> [f64; N] {
    [
        l.exposure_ev as f64,
        l.shoulder_knee as f64,
        l.shadow_lift as f64,
        l.contrast as f64,
        l.saturation as f64,
    ]
}

fn fit_error(fx: &Fixture, look: &Look) -> Err {
    let out = render(&fx.sub_lin, fx.sub_w, look);
    measure(&out, &fx.sub_jpg, Some(&fx.sub_lab))
}
fn total_objective(fxs: &[Fixture], x: &[f64; N]) -> f64 {
    let look = to_look(x);
    let pen: f64 = (0..N)
        .map(|i| (x[i] - x[i].clamp(LO[i], HI[i])).abs())
        .sum::<f64>()
        * 100.0;
    use rayon::prelude::*;
    fxs.par_iter()
        .map(|f| fit_error(f, &look).objective())
        .sum::<f64>()
        / fxs.len() as f64
        + pen
}

fn nelder_mead(f: &dyn Fn(&[f64; N]) -> f64, start: [f64; N], iters: usize) -> ([f64; N], f64) {
    let steps = [0.3, 0.1, 0.4, 0.2, 0.2];
    let mut pts: Vec<([f64; N], f64)> = Vec::new();
    pts.push((start, f(&start)));
    for i in 0..N {
        let mut p = start;
        p[i] += steps[i];
        pts.push((p, f(&p)));
    }
    for _ in 0..iters {
        pts.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut cen = [0f64; N];
        for p in &pts[..N] {
            for i in 0..N {
                cen[i] += p.0[i] / N as f64;
            }
        }
        let worst = pts[N];
        let along =
            |t: f64| -> [f64; N] { std::array::from_fn(|i| cen[i] + t * (worst.0[i] - cen[i])) };
        let r = along(-1.0);
        let fr = f(&r);
        if fr < pts[0].1 {
            let e = along(-2.0);
            let fe = f(&e);
            pts[N] = if fe < fr { (e, fe) } else { (r, fr) };
        } else if fr < pts[N - 1].1 {
            pts[N] = (r, fr);
        } else {
            let c = if fr < worst.1 {
                along(-0.5)
            } else {
                along(0.5)
            };
            let fc = f(&c);
            if fc < worst.1.min(fr) {
                pts[N] = (c, fc);
            } else {
                let best = pts[0].0;
                for p in pts.iter_mut().skip(1) {
                    p.0 = std::array::from_fn(|i| best[i] + 0.5 * (p.0[i] - best[i]));
                    p.1 = f(&p.0);
                }
            }
        }
    }
    pts.sort_by(|a, b| a.1.total_cmp(&b.1));
    pts[0]
}

fn report(fxs: &[Fixture], title: &str, look: &Look) {
    println!("{title}: {look:?}");
    let mut tot = Err::default();
    for f in fxs {
        let out = render(&f.lin, f.w, look);
        let e = measure(&out, &f.jpg, None);
        println!(
            "  {}: MAE r={:.2} g={:.2} b={:.2} (avg {:.2})  dE76={:.2}",
            f.name,
            e.mae[0],
            e.mae[1],
            e.mae[2],
            e.mae_avg(),
            e.de
        );
        for c in 0..3 {
            tot.mae[c] += e.mae[c] / fxs.len() as f64;
        }
        tot.de += e.de / fxs.len() as f64;
    }
    println!(
        "  mean: MAE r={:.2} g={:.2} b={:.2} (avg {:.2})  dE76={:.2}",
        tot.mae[0],
        tot.mae[1],
        tot.mae[2],
        tot.mae_avg(),
        tot.de
    );
}

fn write_png(
    path: &Path,
    w: usize,
    h: usize,
    data: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create(path)?),
        w as u32,
        h as u32,
    );
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header()?;
    wr.write_image_data(data)?;
    wr.finish()?;
    Ok(())
}

/// Write `<name>_compare.png`: ours (left) | camera JPEG (right), each halved by 2x2 box.
fn write_composite(dir: &Path, f: &Fixture, look: &Look) -> Result<(), Box<dyn std::error::Error>> {
    let ours = render(&f.lin, f.w, look);
    let (hw, hh) = (f.w / 2, f.h / 2);
    let half = |src: &[u8], x: usize, y: usize| -> [u8; 3] {
        let mut p = [0u32; 3];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let i = ((y * 2 + dy) * f.w + x * 2 + dx) * 3;
            for c in 0..3 {
                p[c] += src[i + c] as u32;
            }
        }
        p.map(|v| (v / 4) as u8)
    };
    let mut data = Vec::with_capacity(hw * 2 * hh * 3);
    for y in 0..hh {
        for x in 0..hw {
            data.extend_from_slice(&half(&ours, x, y));
        }
        for x in 0..hw {
            data.extend_from_slice(&half(&f.jpg, x, y));
        }
    }
    write_png(
        &dir.join(format!("{}_compare.png", f.name)),
        hw * 2,
        hh,
        &data,
    )
}

pub fn run(
    raw_dir: &Path,
    out_dir: &Path,
    eval_only: Option<Look>,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out_dir)?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(raw_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw")))
        .collect();
    files.sort();
    let fxs = files
        .iter()
        .map(|p| load_fixture(p))
        .collect::<Result<Vec<_>, _>>()?;
    for f in &fxs {
        println!(
            "{}: aligned {}x{}; JPEG<-ours scale x {:.4} y {:.4}, shift ({:.1}, {:.1}) px",
            f.name, f.w, f.h, f.shift[0], f.shift[1], f.shift[2], f.shift[3]
        );
    }
    if std::env::var_os("FIT_DIAG").is_some() {
        diag(&fxs);
    }
    report(&fxs, "before (legacy)", &Look::LEGACY);
    let best = match eval_only {
        Some(l) => l,
        None => {
            let f = |x: &[f64; N]| total_objective(&fxs, x);
            let mut best = (from_look(&Look::LEGACY), f(&from_look(&Look::LEGACY)));
            let starts = [
                from_look(&Look::LEGACY),
                [1.0, 0.7, 0.5, 0.3, 1.2],
                [1.3, 0.6, 1.0, 0.5, 1.1],
                [1.3, 0.3, 0.0, 0.0, 1.2],
                [1.0, 0.2, 0.3, -0.3, 1.1],
                [0.6, 0.5, 0.0, 0.0, 1.0],
            ];
            for s in starts {
                let mut cur = (s, f(&s));
                for _ in 0..3 {
                    cur = nelder_mead(&f, cur.0, 250);
                }
                eprintln!("start {s:?} -> {:?} obj {:.4}", cur.0, cur.1);
                if cur.1 < best.1 {
                    best = cur;
                }
            }
            to_look(&best.0)
        }
    };
    report(&fxs, "after (fitted)", &best);
    for (n, v) in NAMES.iter().zip(from_look(&best)) {
        println!("  {n} = {v:.4}");
    }
    for f in &fxs {
        write_composite(out_dir, f, &best)?;
    }
    println!("composites (ours | camera JPEG) in {}", out_dir.display());
    Ok(())
}

/// Diagnostic: per fixture, JPEG luma (encoded 0..255) mean/std binned by our linear luma.
fn diag(fxs: &[Fixture]) {
    for f in fxs {
        let mut bins = vec![(0f64, 0f64, 0f64, [0f64; 3]); 16];
        for (l, j) in f.sub_lin.chunks_exact(3).zip(f.sub_jpg.chunks_exact(3)) {
            let y = 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
            let e = argus_engine::pipeline::srgb_encode(y);
            let b = ((e * 16.0) as usize).min(15);
            let jy = 0.2126 * j[0] as f64 + 0.7152 * j[1] as f64 + 0.0722 * j[2] as f64;
            bins[b].0 += 1.0;
            bins[b].1 += jy;
            bins[b].2 += jy * jy;
            for c in 0..3 {
                bins[b].3[c] += j[c] as f64 - jy;
            }
        }
        println!(
            "{} (bin = our linear luma, sRGB-encoded/16): n, jpeg mean, std, mean(c-luma) r g b",
            f.name
        );
        for (i, b) in bins.iter().enumerate() {
            if b.0 > 0.0 {
                let m = b.1 / b.0;
                println!(
                    "  {:>5.1}: {:>6} {:>6.1} {:>5.1}  {:>5.1} {:>5.1} {:>5.1}",
                    (i as f32 + 0.5) * 255.0 / 16.0,
                    b.0,
                    m,
                    (b.2 / b.0 - m * m).max(0.0).sqrt(),
                    b.3[0] / b.0,
                    b.3[1] / b.0,
                    b.3[2] / b.0
                );
            }
        }
    }
}
