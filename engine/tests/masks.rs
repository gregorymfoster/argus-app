//! Masks and retouching on real people and skies (two of Greg's bench JPEGs, private repo only), at
//! the develop seam and through the tool layer as the Agent calls it.

use argus_engine::masks::refine::box_blur;
use argus_engine::masks::Analysis;
use argus_engine::recipe::Recipe;
use argus_engine::tools::{Part, Tools};
use argus_engine::{develop_rgb8_with, RgbImage};
use serde_json::{json, Value};
use std::path::Path;

/// Two people close up, indoors, white wall behind (no sky).
const PEOPLE: &str = "../fixtures/bench/cull/2024-08-10/photos/DSC05579.jpg";
/// A close-up portrait with some blemishes.
const PORTRAIT: &str = "../fixtures/bench/cull/2023-05-20/photos/DSC01103.jpg";
/// A couple on a lawn under a big sky.
const FIELD: &str = "../fixtures/bench/cull/2024-08-10/photos/DSC05617.jpg";

fn load(p: &str) -> RgbImage {
    let img = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join(p))
        .unwrap()
        .to_rgb8();
    RgbImage {
        width: img.width() as usize,
        height: img.height() as usize,
        data: img.into_raw(),
    }
}

/// Mean luma over a box given as fractions of the image.
fn luma(img: &RgbImage, x0: f32, y0: f32, x1: f32, y1: f32) -> f64 {
    let (w, h) = (img.width as f32, img.height as f32);
    let (mut s, mut n) = (0f64, 0f64);
    for y in (y0 * h) as usize..(y1 * h) as usize {
        for x in (x0 * w) as usize..(x1 * w) as usize {
            let p = &img.data[(y * img.width + x) * 3..];
            s += 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
            n += 1.0;
        }
    }
    s / n
}

fn recipe(v: Value) -> Recipe {
    Recipe::from_value(&v).unwrap()
}

#[test]
fn analysis_finds_the_faces_and_the_sky() {
    if !crate::has_bench() {
        return;
    }
    let people = Analysis::of(&load(PEOPLE)).unwrap();
    assert_eq!(people.faces.len(), 2, "{:?}", people.faces);
    assert!(people.faces[0].x < people.faces[1].x, "left to right");
    let mean = |p: &[f32]| p.iter().sum::<f32>() / p.len() as f32;
    assert!(mean(&people.sky) < 0.02, "a white wall is not sky");

    let field = Analysis::of(&load(FIELD)).unwrap();
    assert!(mean(&field.sky) > 0.2, "sky {}", mean(&field.sky));
    let n = field.sky_w;
    let row = |y: usize| mean(&field.sky[y * n..(y + 1) * n]);
    assert!(
        row(n / 10) > 0.8 && row(n * 9 / 10) < 0.05,
        "sky on top, lawn below"
    );
    assert!(field.faces.len() >= 2);
}

#[test]
fn a_sky_mask_darkens_the_sky_and_leaves_the_people() {
    if !crate::has_bench() {
        return;
    }
    let img = load(FIELD);
    let a = Analysis::of(&img).unwrap();
    let base = develop_rgb8_with(&img, &Recipe::default(), Some(&a));
    let r = recipe(
        json!({ "local": [{ "id": "sky", "mask": { "kind": "sky" }, "adjust": { "exposure": -1.0 } }] }),
    );
    let out = develop_rgb8_with(&img, &r, Some(&a));
    let (sky0, sky1) = (
        luma(&base, 0.0, 0.0, 1.0, 0.15),
        luma(&out, 0.0, 0.0, 1.0, 0.15),
    );
    assert!(sky1 < sky0 - 30.0, "sky {sky0} -> {sky1}");
    let (lawn0, lawn1) = (
        luma(&base, 0.0, 0.85, 1.0, 1.0),
        luma(&out, 0.0, 0.85, 1.0, 1.0),
    );
    assert!((lawn0 - lawn1).abs() < 2.0, "lawn {lawn0} -> {lawn1}");
}

#[test]
fn light_on_faces_brightens_faces_more_than_the_room() {
    if !crate::has_bench() {
        return;
    }
    let img = load(PEOPLE);
    let a = Analysis::of(&img).unwrap();
    let base = develop_rgb8_with(&img, &Recipe::default(), Some(&a));
    let r = recipe(json!({ "correction": { "exposure": 0, "wbK": 0, "face": 0.5 } }));
    let out = develop_rgb8_with(&img, &r, Some(&a));
    let f = a.faces[1];
    let (fx0, fy0, fx1, fy1) = (
        f.x + f.w * 0.3,
        f.y + f.h * 0.4,
        f.x + f.w * 0.7,
        f.y + f.h * 0.6,
    );
    let face = luma(&out, fx0, fy0, fx1, fy1) - luma(&base, fx0, fy0, fx1, fy1);
    let wall = luma(&out, 0.0, 0.0, 0.15, 0.15) - luma(&base, 0.0, 0.0, 0.15, 0.15);
    assert!(
        face > 10.0 && wall.abs() < 2.0,
        "face +{face}, wall +{wall}"
    );
}

#[test]
fn skin_smoothing_evens_skin_and_keeps_the_rest() {
    if !crate::has_bench() {
        return;
    }
    let img = load(PORTRAIT);
    let a = Analysis::of(&img).unwrap();
    let base = develop_rgb8_with(&img, &Recipe::default(), Some(&a));
    let out = develop_rgb8_with(&img, &recipe(json!({ "global": { "skin": 80 } })), Some(&a));
    // Blotchiness on a cheek: luma detail between fine texture
    // (pores, kept) and the face's shape (kept), i.e. what smoothing should even out.
    let f = a.faces[0];
    let rough = |im: &RgbImage| {
        let (w, h) = (im.width, im.height);
        let g: Vec<f32> = im.data.chunks(3).map(|p| p[1] as f32).collect();
        let (b2, b8) = (box_blur(&g, w, h, 2), box_blur(&g, w, h, 8));
        let (x0, x1) = ((f.x + f.w * 0.12) * w as f32, (f.x + f.w * 0.4) * w as f32);
        let (y0, y1) = ((f.y + f.h * 0.5) * h as f32, (f.y + f.h * 0.72) * h as f32);
        let (mut s, mut n) = (0.0, 0.0);
        for y in y0 as usize..y1 as usize {
            for x in x0 as usize..x1 as usize {
                s += ((b2[y * w + x] - b8[y * w + x]) as f64).powi(2);
                n += 1.0;
            }
        }
        (s / n).sqrt()
    };
    assert!(
        rough(&out) < rough(&base) * 0.8,
        "{} -> {}",
        rough(&base),
        rough(&out)
    );
    let wall = |im: &RgbImage| luma(im, 0.0, 0.0, 0.1, 0.1);
    assert!((wall(&out) - wall(&base)).abs() < 1.0);
}

fn part_json(p: &Part) -> &Value {
    match p {
        Part::Json(v) => v,
        p => panic!("expected JSON, got {p:?}"),
    }
}

#[test]
fn the_agent_adds_checks_edits_and_removes_a_mask() {
    if !crate::has_bench() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2024-08-10_party");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(PEOPLE),
        src.join("DSC05579.jpg"),
    )
    .unwrap();
    let tools = Tools::new(tmp.path().join("Argus.library"));
    tools
        .call("import_folder", json!({ "folder": src }))
        .unwrap();
    let call = |name: &str, a: Value| {
        tools
            .call(name, a)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };

    let seen = call("analyze_photo", json!({ "photo_id": 1 }));
    assert_eq!(part_json(&seen[0])["faces"].as_array().unwrap().len(), 2);

    let added = call(
        "add_mask",
        json!({ "photo_ids": [1], "mask": { "kind": "faces", "face": 1 }, "adjust": { "exposure": 0.4, "skin": 30 } }),
    );
    let local = part_json(&added[0])["photos"]["1"]["local_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(local, "faces");
    let bad = tools.call(
        "add_mask",
        json!({ "photo_ids": [1], "mask": { "kind": "faces", "face": 7 }, "adjust": {} }),
    );
    assert!(bad.unwrap_err().0.contains("2 face(s)"));
    let bad = tools.call(
        "add_mask",
        json!({ "photo_ids": [1], "mask": { "kind": "moon" }, "adjust": {} }),
    );
    assert!(bad.unwrap_err().0.contains("Mask kinds"));

    let view = call(
        "view_mask",
        json!({ "photo_id": 1, "local_id": local, "long_edge": 256 }),
    );
    let cover = part_json(&view[0])["coverage"].as_f64().unwrap();
    assert!(cover > 0.03 && cover < 0.3, "one face covers {cover}");
    assert!(matches!(view[1], Part::Jpeg(_)));

    let edited = call(
        "edit_mask",
        json!({ "photo_id": 1, "local_id": local, "set": { "exposure": 0.2 }, "relative": true }),
    );
    let exp = part_json(&edited[0])["local"]["adjust"]["exposure"]
        .as_f64()
        .unwrap();
    assert!((exp - 0.6).abs() < 1e-6);

    let recipe = call("read_recipe", json!({ "photo_id": 1 }));
    assert_eq!(
        part_json(&recipe[0])["recipe"]["local"][0]["name"],
        "Face 2"
    );
    call("render_photo", json!({ "photo_id": 1, "long_edge": 256 }));

    call("remove_mask", json!({ "photo_id": 1, "local_id": local }));
    let recipe = call("read_recipe", json!({ "photo_id": 1 }));
    assert!(part_json(&recipe[0])["recipe"]["local"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn a_person_mask_selects_parts_of_one_person() {
    if !crate::has_bench() {
        return;
    }
    let img = load(PEOPLE);
    let a = Analysis::of(&img).unwrap();
    let base = develop_rgb8_with(&img, &Recipe::default(), Some(&a));
    let plane = |mask: Value| {
        let r = recipe(
            json!({ "local": [{ "id": "p", "mask": mask, "adjust": { "exposure": 1.0 } }] }),
        );
        develop_rgb8_with(&img, &r, Some(&a))
    };
    let lift = |im: &RgbImage, f: &argus_engine::masks::Face| {
        luma(
            im,
            f.x + f.w * 0.3,
            f.y + f.h * 0.4,
            f.x + f.w * 0.7,
            f.y + f.h * 0.6,
        ) - luma(
            &base,
            f.x + f.w * 0.3,
            f.y + f.h * 0.4,
            f.x + f.w * 0.7,
            f.y + f.h * 0.6,
        )
    };
    let (f0, f1) = (a.faces[0], a.faces[1]);
    let second = plane(json!({ "kind": "person", "person": 1, "parts": ["face_skin"] }));
    assert!(
        lift(&second, &f1) > 10.0 && lift(&second, &f0) < 2.0,
        "only the second face: {} {}",
        lift(&second, &f1),
        lift(&second, &f0)
    );
    let wall = |im: &RgbImage| luma(im, 0.0, 0.0, 0.1, 0.1) - luma(&base, 0.0, 0.0, 0.1, 0.1);
    let whole = plane(json!({ "kind": "person", "parts": ["whole"] }));
    assert!(lift(&whole, &f0) > 10.0 && lift(&whole, &f1) > 10.0 && wall(&whole).abs() < 2.0);
    // Eyes alone: the eyes change, the cheeks don't.
    let eyes = plane(json!({ "kind": "person", "person": 0, "parts": ["eyes"] }));
    let e = f0.eyes[0];
    let at = |im: &RgbImage, c: [f32; 2]| {
        luma(im, c[0] - 0.004, c[1] - 0.003, c[0] + 0.004, c[1] + 0.003)
    };
    assert!(
        at(&eyes, e) - at(&base, e) > 5.0,
        "eye +{}",
        at(&eyes, e) - at(&base, e)
    );
    let cheek = [f0.x + f0.w * 0.5, f0.y + f0.h * 0.75];
    assert!((at(&eyes, cheek) - at(&base, cheek)).abs() < 2.0);
}
