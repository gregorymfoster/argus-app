//! Heal and Clone Spots through the tool layer, as the Agent and the Edit screen use them:
//! a synthetic JPEG with a dark blot, healed, moved, repicked, removed and undone.

use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::path::Path;

const W: u32 = 600;
const H: u32 = 400;
/// The blot: centre and radius in pixels.
const BLOT: (f32, f32, f32) = (300.0, 200.0, 9.0);

/// A soft gradient with grain and one dark blot, like dust on a sky.
fn write_photo(path: &Path) {
    let img = image::RgbImage::from_fn(W, H, |x, y| {
        let g = 110.0 + 70.0 * x as f32 / W as f32 + 30.0 * y as f32 / H as f32;
        let n = ((x * 7919 + y * 104_729) % 11) as f32 - 5.0;
        let blot = (x as f32 - BLOT.0).hypot(y as f32 - BLOT.1) <= BLOT.2;
        let v = if blot { 25.0 } else { g + n };
        image::Rgb([v as u8, (v * 0.95) as u8, (v * 0.9) as u8])
    });
    img.save(path).unwrap();
}

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match &tools
        .call(name, args)
        .unwrap_or_else(|e| panic!("{name}: {e}"))[0]
    {
        Part::Json(v) => v.clone(),
        p => panic!("expected JSON, got {p:?}"),
    }
}

/// Mean luma of the render inside the blot's circle (scaled to the render's size).
fn blot_luma(tools: &Tools) -> f64 {
    let parts = tools
        .call("render_photo", json!({ "photo_id": 1, "long_edge": W }))
        .unwrap();
    let jpeg = parts
        .iter()
        .find_map(|p| match p {
            Part::Jpeg(b) => Some(b.clone()),
            _ => None,
        })
        .expect("a render");
    let img = image::load_from_memory(&jpeg).unwrap().to_luma8();
    let k = img.width() as f32 / W as f32;
    let (cx, cy, r) = (BLOT.0 * k, BLOT.1 * k, BLOT.2 * k);
    let (mut s, mut n) = (0f64, 0f64);
    for (x, y, p) in img.enumerate_pixels() {
        if (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) < r - 1.0 {
            s += p[0] as f64;
            n += 1.0;
        }
    }
    s / n
}

#[test]
fn a_spot_heals_moves_and_undoes_through_the_tool_layer() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("card");
    std::fs::create_dir_all(&src).unwrap();
    write_photo(&src.join("DSC00001.jpg"));
    let tools = Tools::new(tmp.path().join("Argus.library"));
    json1(&tools, "import_folder", json!({ "folder": src }));

    let dark = blot_luma(&tools);
    assert!(dark < 40.0, "the blot shows before healing: {dark}");

    let (x, y, r) = (BLOT.0 / W as f32, BLOT.1 / H as f32, 14.0 / W as f32);
    let picked = json1(
        &tools,
        "pick_spot_source",
        json!({ "photo_id": 1, "x": x, "y": y, "r": r }),
    );
    let added = json1(
        &tools,
        "add_spot",
        json!({ "photo_ids": [1], "x": x, "y": y, "r": r }),
    );
    let spot = &added["photos"]["1"]["spot"];
    assert_eq!(spot["id"], "spot");
    assert_eq!(spot["mode"], "heal");
    assert_eq!(
        spot["from"], picked["from"],
        "add_spot picks as pick_spot_source does"
    );
    let from = (
        spot["from"]["x"].as_f64().unwrap(),
        spot["from"]["y"].as_f64().unwrap(),
    );
    let apart = ((from.0 - x as f64) * W as f64).hypot((from.1 - y as f64) * H as f64);
    assert!(
        apart >= 2.0 * 14.0,
        "the source is clear of the spot: {from:?}"
    );
    let healed = blot_luma(&tools);
    assert!(healed > 120.0, "the blot is gone: {dark} -> {healed}");

    // The saved Recipe carries it, so a slider edit from the UI keeps it.
    let saved = json1(&tools, "read_recipe", json!({ "photo_id": 1 }));
    assert_eq!(saved["recipe"]["spots"].as_array().unwrap().len(), 1);

    // Clone from a hand-placed source; then ask for a fresh pick.
    let edited = json1(
        &tools,
        "edit_spot",
        json!({ "photo_id": 1, "spot_id": "spot", "mode": "clone", "from": { "x": 0.2, "y": 0.5 } }),
    );
    assert_eq!(edited["spot"]["mode"], "clone");
    assert_eq!(edited["spot"]["from"]["x"], 0.2);
    let repicked = json1(
        &tools,
        "edit_spot",
        json!({ "photo_id": 1, "spot_id": "spot", "repick": true }),
    );
    assert_ne!(repicked["spot"]["from"]["x"], 0.2);

    // Too big is object removal, which Spots don't do; off the Photo is an error.
    let big = tools.call(
        "add_spot",
        json!({ "photo_ids": [1], "x": 0.5, "y": 0.5, "r": 0.4 }),
    );
    assert!(big.unwrap_err().0.contains("object removal"));
    let off = tools.call(
        "add_spot",
        json!({ "photo_ids": [1], "x": 1.5, "y": 0.5, "r": 0.02 }),
    );
    assert!(off.unwrap_err().0.contains("outside the Photo"));

    let gone = json1(
        &tools,
        "remove_spot",
        json!({ "photo_id": 1, "spot_id": "spot" }),
    );
    assert_eq!(gone["spots"], json!([]));
    assert!(
        blot_luma(&tools) < 40.0,
        "removing the Spot brings the blot back"
    );

    // Undo the removal: the Spot is back.
    json1(&tools, "undo_batch", json!({}));
    let back = json1(&tools, "read_recipe", json!({ "photo_id": 1 }));
    assert_eq!(back["recipe"]["spots"][0]["id"], "spot");
}

#[test]
fn accepting_a_suggestion_keeps_the_persons_spots() {
    use argus_engine::library::{Author, Library};
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("card");
    std::fs::create_dir_all(&src).unwrap();
    write_photo(&src.join("DSC00001.jpg"));
    let dir = tmp.path().join("Argus.library");
    let tools = Tools::new(dir.clone());
    json1(&tools, "import_folder", json!({ "folder": src }));
    json1(
        &tools,
        "add_spot",
        json!({ "photo_ids": [1], "x": 0.5, "y": 0.5, "r": 0.02 }),
    );
    let mut lib = Library::open(&dir).unwrap();
    let ids = lib
        .replace_suggestions(
            1,
            None,
            None,
            &[(
                "Punchy".into(),
                "more contrast".into(),
                json!({ "global": { "contrast": 30 } }),
                None,
            )],
        )
        .unwrap();
    for amount in [None, Some(0.5)] {
        lib.accept_suggestions(&ids, Author::Person, amount)
            .unwrap();
        let r = lib.read_recipe(1).unwrap().unwrap().recipe;
        assert_eq!(r["spots"][0]["id"], "spot", "at {amount:?}: {r}");
        assert!(r["global"]["contrast"].as_f64().unwrap() > 0.0);
    }
}
