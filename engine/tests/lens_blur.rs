//! Lens Blur through the tool layer: the Agent turns it on with `edit_recipe`, sees the
//! depth with `view_depth`, and the render keeps the people sharp while the background
//! softens. The depth model is swapped for a stand-in (`ARGUS_DEPTH_FAKE`) so this runs in a
//! second; the model itself is checked by `analyzers::` on real photos.

use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::path::Path;

const PEOPLE: &str = "../fixtures/bench/cull/2024-08-10/photos/DSC05579.jpg";

fn part_json(p: &Part) -> &Value {
    match p {
        Part::Json(v) => v,
        p => panic!("expected JSON, got {p:?}"),
    }
}

fn jpeg(p: &Part) -> image::RgbImage {
    match p {
        Part::Jpeg(b) => image::load_from_memory(b).unwrap().to_rgb8(),
        p => panic!("expected a JPEG, got {p:?}"),
    }
}

/// Mean absolute difference between neighbouring pixels in a box: how much detail it has.
fn detail(img: &image::RgbImage, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let (mut sum, mut n) = (0f32, 0f32);
    for y in (y0 * h) as u32..(y1 * h) as u32 {
        for x in (x0 * w) as u32..((x1 * w) as u32).min(img.width() - 1) {
            let a = img.get_pixel(x, y)[1] as f32;
            let b = img.get_pixel(x + 1, y)[1] as f32;
            sum += (a - b).abs();
            n += 1.0;
        }
    }
    sum / n.max(1.0)
}

#[test]
fn lens_blur_softens_the_background_and_keeps_the_people() {
    if !crate::has_bench() {
        return;
    }
    let _env = crate::env_lock();
    std::env::set_var("ARGUS_DEPTH_FAKE", "1");
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
    let render = || jpeg(&call("render_photo", json!({ "photo_id": 1, "long_edge": 768 }))[1]);
    let before = render();

    call(
        "edit_recipe",
        json!({ "photo_ids": [1], "set": { "lensBlur.amount": 100, "lensBlur.bokeh": "blade" } }),
    );
    let recipe = call("read_recipe", json!({ "photo_id": 1 }));
    let lb = &part_json(&recipe[0])["recipe"]["lensBlur"];
    assert_eq!(lb["amount"], 100.0, "{lb}");
    assert_eq!(lb["bokeh"], "blade");
    let after = render();

    // The stand-in puts the top of the frame far behind the people.
    let (sky0, sky1) = (
        detail(&before, 0.0, 0.0, 1.0, 0.12),
        detail(&after, 0.0, 0.0, 1.0, 0.12),
    );
    assert!(sky1 < sky0 * 0.7, "background detail {sky0} -> {sky1}");

    let depth = call(
        "view_depth",
        json!({ "photo_id": 1, "x": 0.5, "y": 0.9, "long_edge": 256 }),
    );
    let d = part_json(&depth[0]);
    assert_eq!(d["subject"], true);
    let focus: Vec<f64> = serde_json::from_value(d["focus"].clone()).unwrap();
    assert!(focus[0] < focus[1] && focus[1] <= 100.0, "{focus:?}");
    let at: Vec<f64> = serde_json::from_value(d["at"].clone()).unwrap();
    assert!(at[0] > 50.0, "the bottom of the frame is near: {at:?}");
    assert_eq!(d["histogram"].as_array().unwrap().len(), 32);
    assert!(jpeg(&depth[1]).width() == 256 || jpeg(&depth[1]).height() == 256);

    // A focal range picked at the top of the frame flips it: the background comes back.
    call(
        "edit_recipe",
        json!({ "photo_ids": [1], "set": { "lensBlur.focus": [0, 30] } }),
    );
    let flipped = render();
    let sky2 = detail(&flipped, 0.0, 0.0, 1.0, 0.12);
    assert!(sky2 > sky1 * 1.2, "in focus again: {sky1} -> {sky2}");
    std::env::remove_var("ARGUS_DEPTH_FAKE");
}
