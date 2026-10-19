//! Recipes change renders the way their sliders say (tested at the develop seam).
//! Synthetic images run everywhere; the RAW cases skip when fixtures/raw is empty.

use argus_engine::recipe::Recipe;
use argus_engine::{decode, develop, develop_rgb8, DevelopOptions, RgbImage};
use serde_json::json;
use std::path::Path;

fn recipe(global: serde_json::Value) -> Recipe {
    Recipe::from_value(&json!({ "global": global })).unwrap()
}

/// A gradient with colour: R ramps across, G ramps down, B constant.
fn chart() -> RgbImage {
    let (w, h) = (64, 32);
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            data.extend([(x * 4) as u8, (y * 8) as u8, 120]);
        }
    }
    RgbImage {
        width: w,
        height: h,
        data,
    }
}

fn mean(img: &RgbImage) -> [f64; 3] {
    let mut s = [0f64; 3];
    for p in img.data.chunks(3) {
        for c in 0..3 {
            s[c] += p[c] as f64;
        }
    }
    s.map(|v| v / (img.data.len() / 3) as f64)
}

fn luma(m: [f64; 3]) -> f64 {
    0.2126 * m[0] + 0.7152 * m[1] + 0.0722 * m[2]
}

#[test]
fn empty_recipe_leaves_a_jpeg_photo_unchanged() {
    let img = chart();
    let out = develop_rgb8(&img, &Recipe::default());
    let max = img
        .data
        .iter()
        .zip(&out.data)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(max <= 1, "max diff {max}");
}

#[test]
fn sliders_move_a_jpeg_photo_in_their_direction() {
    let img = chart();
    let base = mean(&img);
    let at = |g| mean(&develop_rgb8(&img, &recipe(g)));
    assert!(luma(at(json!({ "exposure": 1.0 }))) > luma(base) + 20.0);
    assert!(luma(at(json!({ "exposure": -1.0 }))) < luma(base) - 20.0);
    let warm = at(json!({ "temp": 50.0 }));
    assert!(warm[0] - warm[2] > base[0] - base[2] + 10.0);
    let magenta = at(json!({ "tint": 50.0 }));
    let pink = |m: [f64; 3]| (m[0] + m[2]) / 2.0 - m[1];
    assert!(pink(magenta) > pink(base) + 6.0, "{base:?} -> {magenta:?}");
    assert!(luma(at(json!({ "shadows": 100.0 }))) > luma(base));
    assert!(luma(at(json!({ "highlights": -100.0 }))) < luma(base));
    let grey = at(json!({ "saturation": -100.0 }));
    assert!((grey[0] - grey[2]).abs() < (base[0] - base[2]).abs() * 0.2 + 1.0);
}

#[test]
fn recipe_exposure_brightens_a_raw_render() {
    let raw_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw/DSC04949.ARW");
    if !raw_path.exists() {
        eprintln!("recipe_render: {} missing, skipping", raw_path.display());
        return;
    }
    let raw = decode(&raw_path).unwrap();
    let render = |r: Recipe| {
        mean(&develop(
            &raw,
            &DevelopOptions {
                long_edge: Some(256),
                recipe: r,
                ..Default::default()
            },
        ))
    };
    let base = render(Recipe::default());
    let bright = render(recipe(json!({ "exposure": 1.0 })));
    let cool = render(recipe(json!({ "temp": -40.0 })));
    assert!(luma(bright) > luma(base) + 15.0, "{base:?} -> {bright:?}");
    assert!(cool[2] - cool[0] > base[2] - base[0] + 5.0);
}
