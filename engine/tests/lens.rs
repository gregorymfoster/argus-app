//! Lens corrections read from the camera's own data in sample ARWs (engine/src/lens.rs).

use argus_engine::lens::{self, Which};
use argus_engine::recipe::Recipe;
use argus_engine::{decode, develop, DevelopOptions};
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures")
        .join(rel);
    p.exists().then_some(p).or_else(|| {
        eprintln!("{rel} missing, skipping");
        None
    })
}

#[test]
fn sony_arws_carry_their_lens_corrections() {
    // A7R II with the FE 28mm F2: the camera corrected all three in its JPEG.
    if let Some(p) = fixture("raw/DSC04949.ARW") {
        let l = lens::read(&p).expect("A7R II profile");
        assert_eq!(l.camera, Which::ALL);
        assert_eq!(l.distortion.len(), 16);
        // Barrel: the corners sample from nearer the centre.
        assert!(*l.distortion.last().unwrap() < 0.95, "{:?}", l.distortion);
        // Corners get roughly a third less light than the centre.
        let corner = *l.vignetting.last().unwrap();
        assert!((0.55..0.85).contains(&corner), "corner light {corner}");
        assert!((l.vignetting[0] - 1.0).abs() < 0.01);
    }
    // A7R V with the FE 50mm F1.4 GM: no distortion correction in the camera's JPEG.
    if let Some(p) = fixture("raw-a7rv/DSC07745.ARW") {
        let l = lens::read(&p).expect("A7R V profile");
        assert!(!l.camera.distortion && l.camera.vignetting && l.camera.ca);
    }
}

#[test]
fn lens_corrections_brighten_the_corners_unless_turned_off() {
    let Some(p) = fixture("raw/DSC04949.ARW") else {
        return;
    };
    let raw = decode(&p).unwrap();
    let render = |mode: &str| {
        let mut recipe = Recipe::default();
        recipe
            .global
            .other
            .insert("lensCorrections".into(), mode.into());
        develop(
            &raw,
            &DevelopOptions {
                long_edge: Some(600),
                recipe,
                ..Default::default()
            },
        )
    };
    // Distortion moves the frame a little, so compare regions rather than pixels.
    let (off, on) = (render("off"), render("on"));
    assert_eq!((off.width, off.height), (on.width, on.height));
    let mean = |img: &argus_engine::RgbImage, x0: usize, y0: usize| {
        let mut s = 0.0;
        for y in y0..y0 + 20 {
            for x in x0..x0 + 20 {
                s += img.data[(y * img.width + x) * 3 + 1] as f64;
            }
        }
        s / 400.0
    };
    let (w, h) = (off.width, off.height);
    let centre = |i| mean(i, w / 2 - 10, h / 2 - 10);
    assert!((centre(&off) - centre(&on)).abs() < 6.0, "centre moved");
    // Bottom corners are water, no clipped sky.
    let corners = |i| mean(i, 0, h - 20) + mean(i, w - 20, h - 20);
    assert!(
        corners(&on) > corners(&off) + 6.0,
        "{} vs {}",
        corners(&on),
        corners(&off)
    );
    // The default follows the camera, which corrected all three on this file.
    let camera = develop(
        &raw,
        &DevelopOptions {
            long_edge: Some(600),
            ..Default::default()
        },
    );
    assert_eq!(camera.data, on.data);
}

#[test]
fn edit_recipe_takes_lens_corrections_as_a_word() {
    let mut r = Recipe::default();
    r.set_value("lensCorrections", &"off".into(), false)
        .unwrap();
    assert_eq!(r.global.other["lensCorrections"], "off");
    r.set_value("lensCorrections", &"camera".into(), false)
        .unwrap();
    assert!(!r.global.other.contains_key("lensCorrections"));
    assert!(r
        .set_value("lensCorrections", &"maybe".into(), false)
        .is_err());
}
