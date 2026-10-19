//! Batch undo, Version history and Looks through the tool layer, as the Agent, MCP and
//! the app call them.

use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::path::Path;

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..160 * 120 * 3)
        .map(|i| 60 + ((i as u8).wrapping_mul(seed) % 120))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 160, 120, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn library(n: u8) -> (tempfile::TempDir, Tools, Vec<i64>) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    for i in 0..n {
        write_jpeg(&src.join(format!("p{i:02}.jpg")), i * 2 + 3);
    }
    let tools = Tools::new(tmp.path().join("Argus.library"));
    json1(&tools, "import_folder", json!({ "folder": src }));
    let ids = json1(&tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    (tmp, tools, ids)
}

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match tools
        .call(name, args)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .remove(0)
    {
        Part::Json(v) => v,
        p => panic!("{name}: expected JSON, got {p:?}"),
    }
}

fn mean(tools: &Tools, id: i64) -> f64 {
    let parts = tools
        .call("render_photo", json!({ "photo_id": id, "long_edge": 64 }))
        .unwrap();
    let Part::Jpeg(b) = &parts[1] else { panic!() };
    let img = image::load_from_memory(b).unwrap().to_rgb8();
    img.pixels()
        .map(|p| p.0.iter().map(|&c| c as f64).sum::<f64>())
        .sum::<f64>()
        / img.pixels().len() as f64
}

#[test]
fn undo_batch_puts_recipes_and_verdicts_back_and_is_itself_undoable() {
    let (_t, tools, ids) = library(3);
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [ids[0]], "set": { "exposure": 0.5 } }),
    );
    json1(
        &tools,
        "set_verdict",
        json!({ "photo_ids": [ids[1]], "verdict": "pick", "as": "person" }),
    );
    let ai = json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [ids[0], ids[2]], "set": { "temp": 20 } }),
    );
    json1(
        &tools,
        "set_verdict",
        json!({ "photo_ids": [ids[2]], "verdict": "reject", "reason": "soft" }),
    );

    // Undo the edit batch by id: Photo 0 keeps its earlier exposure, Photo 2 is neutral again.
    let u = json1(&tools, "undo_batch", json!({ "batch_id": ai["batch_id"] }));
    let mut want = vec![ids[0], ids[2]];
    want.sort();
    assert_eq!(u["recipes"], json!(want));
    let r0 = json1(&tools, "read_recipe", json!({ "photo_id": ids[0] }));
    assert_eq!(r0["recipe"]["global"], json!({ "exposure": 0.5 }));
    assert_eq!(r0["version"], 3, "undo appends a Version");
    let r2 = json1(&tools, "read_recipe", json!({ "photo_id": ids[2] }));
    assert_eq!(r2["recipe"]["global"], json!({}));

    // Without an id it undoes the newest AI batch: the reject goes, back to Unflagged by nobody.
    let u = json1(&tools, "undo_batch", json!({}));
    assert_eq!(u["verdicts"], json!([ids[2]]));
    let photos = json1(&tools, "list_photos", json!({}));
    assert_eq!(photos[2]["verdict"], "unflagged");
    assert_eq!(photos[2]["verdict_by"], Value::Null);
    assert_eq!(
        photos[1]["verdict"], "pick",
        "the person's flag is untouched"
    );

    // History shows every Version with its batch label.
    let h = json1(&tools, "list_versions", json!({ "photo_id": ids[0] }));
    let labels: Vec<_> = h
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(labels, ["edit_recipe", "edit_recipe", "Undo: edit_recipe"]);
    assert_eq!(h[2]["author"], "person");
}

#[test]
fn a_look_renders_under_each_photos_own_sliders_and_tunes_them_all_at_once() {
    let (_t, tools, ids) = library(3);
    json1(
        &tools,
        "set_verdict",
        json!({ "photo_ids": [ids[0], ids[1]], "verdict": "pick" }),
    );
    let before = [mean(&tools, ids[0]), mean(&tools, ids[2])];

    // A new Look goes on the Shoot's Picks by default.
    let shoot = json1(&tools, "list_shoots", json!({}))[0]["id"].clone();
    let l = json1(
        &tools,
        "set_look",
        json!({ "shoot_id": shoot, "name": "Bright and airy", "set": { "exposure": 0.6 } }),
    );
    assert_eq!(l["look"]["id"], "bright-and-airy");
    let mut applied: Vec<i64> = serde_json::from_value(l["applied_to"].clone()).unwrap();
    applied.sort();
    let mut want = vec![ids[0], ids[1]];
    want.sort();
    assert_eq!(applied, want);
    assert!(
        mean(&tools, ids[0]) > before[0] + 10.0,
        "the Look brightens a Pick"
    );
    assert!(
        (mean(&tools, ids[2]) - before[1]).abs() < 1.0,
        "a Photo not under it is unchanged"
    );

    // Tuning the only Look needs no id, changes every Photo under it, and undoes in one step.
    let lit = mean(&tools, ids[1]);
    let t = json1(
        &tools,
        "set_look",
        json!({ "shoot_id": shoot, "set": { "exposure": -0.4 }, "relative": true }),
    );
    assert_eq!(t["created"], false);
    assert!((t["look"]["adjust"]["exposure"].as_f64().unwrap() - 0.2).abs() < 1e-9);
    assert!(mean(&tools, ids[1]) < lit - 5.0);
    json1(&tools, "undo_batch", json!({ "batch_id": t["batch_id"] }));
    assert_eq!(
        json1(&tools, "list_looks", json!({ "shoot_id": shoot }))[0]["adjust"]["exposure"],
        0.6
    );
}

#[test]
fn sync_to_look_folds_a_photos_sliders_into_the_shared_look() {
    let (_t, tools, ids) = library(2);
    json1(
        &tools,
        "set_verdict",
        json!({ "photo_ids": ids, "verdict": "pick" }),
    );
    let shoot = json1(&tools, "list_shoots", json!({}))[0]["id"].clone();
    json1(
        &tools,
        "set_look",
        json!({ "shoot_id": shoot, "name": "Warm", "set": { "temp": 20 } }),
    );
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [ids[0]], "set": { "contrast": 15, "temp": 5 } }),
    );
    let s = json1(&tools, "sync_to_look", json!({ "photo_id": ids[0] }));
    assert_eq!(
        s["look"]["adjust"],
        json!({ "contrast": 15.0, "temp": 25.0 })
    );
    assert_eq!(s["photos_under_look"].as_array().unwrap().len(), 2);
    let r = json1(&tools, "read_recipe", json!({ "photo_id": ids[0] }));
    assert_eq!(r["recipe"]["global"], json!({}));
    assert_eq!(r["recipe"]["look"], "warm");
}

#[test]
fn look_mistakes_come_back_as_messages() {
    let (_t, tools, ids) = library(1);
    let shoot = json1(&tools, "list_shoots", json!({}))[0]["id"].clone();
    let e = tools
        .call("set_look", json!({ "shoot_id": shoot, "look_id": "nope" }))
        .unwrap_err()
        .0;
    assert!(e.contains("omit look_id"), "{e}");
    let e = tools
        .call(
            "set_look",
            json!({ "shoot_id": shoot, "set": { "correction.exposure": 1 } }),
        )
        .unwrap_err()
        .0;
    assert!(e.contains("edit_recipe"), "{e}");
    let e = tools.call("undo_batch", json!({})).unwrap_err().0;
    assert!(e.contains("no AI change"), "{e}");
    let _ = ids;
}

#[test]
fn a_v1_library_migrates_and_keeps_working() {
    let (_t, tools, ids) = library(1);
    {
        let db = rusqlite::Connection::open(tools.library().join("library.db")).unwrap();
        db.execute_batch(
            "ALTER TABLE looks DROP COLUMN batch_id; ALTER TABLE photos DROP COLUMN jpeg;
             DROP TABLE ratings; DROP TABLE rating_log; DROP TABLE best_photos;
             DROP TABLE suggestions; DROP TABLE jobs; DROP TABLE facts; DROP TABLE usage_events;
             ALTER TABLE photos DROP COLUMN lens; ALTER TABLE photos DROP COLUMN focal; ALTER TABLE photos DROP COLUMN iso; ALTER TABLE photos DROP COLUMN aperture; ALTER TABLE photos DROP COLUMN shutter;
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let shoot = json1(&tools, "list_shoots", json!({}))[0]["id"].clone();
    let l = json1(
        &tools,
        "set_look",
        json!({ "shoot_id": shoot, "name": "Warm", "set": { "temp": 10 }, "photo_ids": ids }),
    );
    json1(&tools, "undo_batch", json!({ "batch_id": l["batch_id"] }));
    assert_eq!(
        json1(&tools, "list_looks", json!({ "shoot_id": shoot })),
        json!([])
    );
}
