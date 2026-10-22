//! Golden-image tests (ADR 0005): render every fixture RAW at 1024 px long edge with
//! default options and compare against `fixtures/golden/<stem>.png`.
//!
//! Tolerance: mean absolute error per channel (0..255 scale) must be <= `MAE_TOLERANCE`.
//! A deterministic pipeline on the same machine gives MAE 0; 1.0 (~0.4% of full scale)
//! absorbs f32 rounding/ordering differences between refactors, thread counts and CPUs
//! (a single LUT step is ~1 level on a few percent of pixels) while a real change to
//! exposure, WB, matrix or curve moves MAE by several levels.
//!
//! Bless (rewrite references): `ARGUS_BLESS=1 cargo test --profile fast --test engine golden::`
//! (`just bless`). On failure `out/golden/<stem>_actual.png` and `_diff.png` (|a-b| x8).

use argus_engine::{decode, develop, DevelopOptions, RgbImage};
use std::path::{Path, PathBuf};

const MAE_TOLERANCE: f64 = 1.0;
const LONG_EDGE: usize = 1024;
const DIFF_GAIN: u32 = 8;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn write_png(path: &Path, w: usize, h: usize, data: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create(path).unwrap()),
        w as u32,
        h as u32,
    );
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Best);
    let mut wr = enc.write_header().unwrap();
    wr.write_image_data(data).unwrap();
    wr.finish().unwrap();
}

fn read_png(path: &Path) -> (usize, usize, Vec<u8>) {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0u8; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).unwrap();
    assert_eq!(
        info.color_type,
        png::ColorType::Rgb,
        "{} must be 8-bit RGB",
        path.display()
    );
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    buf.truncate(info.buffer_size());
    (info.width as usize, info.height as usize, buf)
}

fn render(path: &Path) -> RgbImage {
    let raw = decode(path).expect("decode");
    develop(
        &raw,
        &DevelopOptions {
            long_edge: Some(LONG_EDGE),
            ..Default::default()
        },
    )
}

/// Per-channel mean absolute error on the 0..255 scale.
fn mae(a: &[u8], b: &[u8]) -> [f64; 3] {
    let mut sum = [0u64; 3];
    for (pa, pb) in a.chunks(3).zip(b.chunks(3)) {
        for c in 0..3 {
            sum[c] += pa[c].abs_diff(pb[c]) as u64;
        }
    }
    let n = (a.len() / 3) as f64;
    sum.map(|s| s as f64 / n)
}

#[test]
fn golden_renders_match() {
    let raw_dir = root().join("fixtures/raw");
    let golden_dir = root().join("fixtures/golden");
    let out_dir = root().join("out/golden");
    let bless = std::env::var_os("ARGUS_BLESS").is_some_and(|v| v == "1");

    let mut files: Vec<PathBuf> = std::fs::read_dir(&raw_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw")))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    if files.is_empty() {
        eprintln!("golden: no *.ARW in fixtures/raw, skipping");
        return;
    }

    let mut failures = Vec::new();
    for file in &files {
        let stem = file.file_stem().unwrap().to_string_lossy().to_string();
        let img = render(file);
        let golden = golden_dir.join(format!("{stem}.png"));
        if bless {
            write_png(&golden, img.width, img.height, &img.data);
            eprintln!("golden: blessed {}", golden.display());
            continue;
        }
        if !golden.exists() {
            failures.push(format!(
                "{stem}: missing reference {}; run `just bless`",
                golden.display()
            ));
            continue;
        }
        let (gw, gh, gdata) = read_png(&golden);
        if (gw, gh) != (img.width, img.height) {
            let actual = out_dir.join(format!("{stem}_actual.png"));
            write_png(&actual, img.width, img.height, &img.data);
            failures.push(format!(
                "{stem}: dimension mismatch, got {}x{}, reference {gw}x{gh}; actual: {}",
                img.width,
                img.height,
                actual.display()
            ));
            continue;
        }
        let m = mae(&img.data, &gdata);
        eprintln!(
            "golden: {stem} MAE r={:.4} g={:.4} b={:.4} (tolerance {MAE_TOLERANCE})",
            m[0], m[1], m[2]
        );
        if m.iter().any(|&v| v > MAE_TOLERANCE) {
            let actual = out_dir.join(format!("{stem}_actual.png"));
            let diff = out_dir.join(format!("{stem}_diff.png"));
            write_png(&actual, img.width, img.height, &img.data);
            let d: Vec<u8> = img
                .data
                .iter()
                .zip(&gdata)
                .map(|(&a, &b)| (a.abs_diff(b) as u32 * DIFF_GAIN).min(255) as u8)
                .collect();
            write_png(&diff, img.width, img.height, &d);
            failures.push(format!(
                "{stem}: MAE r={:.4} g={:.4} b={:.4} exceeds {MAE_TOLERANCE}\n    actual: {}\n    diff (x{DIFF_GAIN}): {}\n    reference: {}\n    If intentional, run `just bless` after viewing the images.",
                m[0], m[1], m[2], actual.display(), diff.display(), golden.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "golden mismatches:\n  {}",
        failures.join("\n  ")
    );
}
