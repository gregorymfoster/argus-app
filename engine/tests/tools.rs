//! The tool layer through its one entry point, as the Agent and MCP call it: JSON in,
//! JSON and JPEGs out, against a temp Library of generated JPEGs.

use argus_engine::cull::{ContactSheet, CullPhoto, CullResult, Provider};
use argus_engine::library::Verdict;
use argus_engine::tools::{Part, Tools};
use argus_engine::Result;
use serde_json::{json, Value};
use std::path::Path;

fn write_jpeg(path: &Path, w: u16, h: u16, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..w as usize * h as usize * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, w, h, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

/// A Library with one imported Shoot of `n` distinct JPEGs.
fn library(n: u8) -> (tempfile::TempDir, Tools) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    for i in 0..n {
        write_jpeg(&src.join(format!("p{i:02}.jpg")), 320, 240, i * 2 + 3);
    }
    let tools = Tools::new(tmp.path().join("Argus.library"));
    let r = json1(&tools, "import_folder", json!({ "folder": src }));
    assert_eq!(r["imported"], n as u64, "{r}");
    (tmp, tools)
}

fn call(tools: &Tools, name: &str, args: Value) -> Vec<Part> {
    tools
        .call(name, args)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match call(tools, name, args).remove(0) {
        Part::Json(v) => v,
        p => panic!("{name}: expected JSON first, got {p:?}"),
    }
}

fn jpeg(part: &Part) -> image::RgbImage {
    match part {
        Part::Jpeg(b) => image::load_from_memory(b).unwrap().to_rgb8(),
        p => panic!("expected a JPEG, got {p:?}"),
    }
}

fn ids(tools: &Tools) -> Vec<i64> {
    json1(tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect()
}

#[test]
fn every_tool_has_an_object_schema_and_a_unique_name() {
    let specs = Tools::specs();
    let mut names: Vec<_> = specs.iter().map(|s| s.name).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), specs.len());
    for s in &specs {
        assert_eq!(s.input_schema["type"], "object", "{}", s.name);
        assert!(!s.description.is_empty());
    }
}

#[test]
fn list_and_preview() {
    let (_tmp, tools) = library(3);
    let shoots = json1(&tools, "list_shoots", json!(null));
    assert_eq!(shoots[0]["name"], "2026-10-12_tahoe");
    assert_eq!(shoots[0]["photos"], 3);

    let photos = json1(&tools, "list_photos", json!({ "shoot_id": 1 }));
    assert_eq!(photos.as_array().unwrap().len(), 3);
    assert_eq!(photos[0]["verdict"], "unflagged");
    assert!(
        photos[0].get("preview").is_none(),
        "no Library-internal paths"
    );

    let parts = call(
        &tools,
        "get_preview",
        json!({ "photo_id": photos[0]["id"], "long_edge": 100 }),
    );
    assert_eq!(parts.len(), 2);
    let img = jpeg(&parts[1]);
    assert_eq!((img.width(), img.height()), (100, 75));
}

#[test]
fn contact_sheet_pages_number_every_photo_in_capture_order() {
    let (_tmp, tools) = library(32);
    let parts = call(&tools, "get_contact_sheet", json!({ "shoot_id": 1 }));
    let Part::Json(info) = &parts[0] else {
        panic!()
    };
    assert_eq!(
        (info["pages"].as_u64(), info["photos_in_shoot"].as_u64()),
        (Some(2), Some(32))
    );
    assert_eq!(info["photos"].as_array().unwrap().len(), 30);
    assert_eq!(info["photos"][0]["n"], 1);
    let sheet = jpeg(&parts[1]);
    assert_eq!((sheet.width(), sheet.height()), (6 * 256, 5 * 256));
    // The number tag sits in each cell's top-left corner: black with white digit strokes.
    let tag = sheet.get_pixel(5, 5);
    assert!(tag.0.iter().all(|&c| c < 40), "tag background {tag:?}");
    let stroke = sheet.get_pixel(13, 9); // top of the "1" in cell 1
    assert!(stroke.0.iter().all(|&c| c > 200), "digit stroke {stroke:?}");

    let page2 = call(
        &tools,
        "get_contact_sheet",
        json!({ "shoot_id": 1, "page": 2 }),
    );
    let Part::Json(info2) = &page2[0] else {
        panic!()
    };
    assert_eq!(info2["photos"][0]["n"], 31);
    assert_eq!(jpeg(&page2[1]).width(), 2 * 256);

    let err = tools
        .call("get_contact_sheet", json!({ "shoot_id": 1, "page": 3 }))
        .unwrap_err();
    assert!(err.0.contains("out of range"), "{err}");
}

#[test]
fn ai_verdicts_never_overwrite_the_persons_flags() {
    let (_tmp, tools) = library(3);
    let ids = ids(&tools);
    json1(
        &tools,
        "set_verdict",
        json!({ "photo_ids": [ids[0]], "verdict": "pick", "as": "person" }),
    );
    let r = json1(
        &tools,
        "set_verdict",
        json!({ "photo_ids": ids, "verdict": "reject", "reason": "soft focus" }),
    );
    assert_eq!(r["changed"], json!([ids[1], ids[2]]));
    assert_eq!(r["kept_person_verdicts"], json!([ids[0]]));

    let picks = json1(&tools, "list_photos", json!({ "verdict": "pick" }));
    assert_eq!(picks[0]["id"], ids[0]);
    let rejects = json1(&tools, "list_photos", json!({ "verdict": "reject" }));
    assert_eq!(rejects[0]["reason"], "soft focus");
    assert_eq!(rejects[0]["verdict_by"], "ai");
}

#[test]
fn stacks_round_trip_best_first() {
    let (_tmp, tools) = library(4);
    let ids = ids(&tools);
    let r = json1(
        &tools,
        "set_stacks",
        json!({ "shoot_id": 1, "stacks": [[ids[2], ids[1]], [ids[3]]] }),
    );
    assert_eq!(r, json!([[ids[2], ids[1]]]), "singletons are dropped");
    assert_eq!(json1(&tools, "list_stacks", json!({ "shoot_id": 1 })), r);
}

struct Fake;
impl Provider for Fake {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        Ok(CullResult {
            summary: "fake".into(),
            photos: sheet
                .photos
                .iter()
                .map(|p| CullPhoto {
                    n: p.n,
                    verdict: if p.n == 1 {
                        Verdict::Pick
                    } else {
                        Verdict::Reject
                    },
                    reason: "fake".into(),
                    stack: Some(1),
                    rank: Some(p.n as u32),
                    stars: Some(if p.n == 1 { 4 } else { 1 }),
                    best: (p.n == 1).then_some(1),
                })
                .collect(),
        })
    }
}

#[test]
fn cull_shoot_checks_with_the_rules_by_default_and_asks_no_model() {
    let (_tmp, tools) = library(3);
    let tools = tools.with_providers(|name, _| panic!("asked the model {name:?}"));
    let r = json1(&tools, "cull_shoot", json!({ "provider": "ollama" }));
    assert_eq!(r["picks"].as_u64(), Some(0), "the rules never pick");
    assert_eq!(
        r["rated"].as_u64(),
        Some(3),
        "Taste still rates every Photo"
    );
}

#[test]
fn cull_shoot_defaults_to_the_newest_shoot_and_passes_provider_choice() {
    let (_tmp, tools) = library(3);
    let tools = tools.with_providers(|name, compute| {
        assert_eq!(name, Some("ollama"));
        assert_eq!(compute.map(|c| format!("{c:?}")), Some("Gentle".into()));
        Ok(Box::new(Fake))
    });
    let r = json1(
        &tools,
        "cull_shoot",
        json!({ "provider": "ollama", "compute": "gentle", "check": "model" }),
    );
    assert_eq!(
        (
            r["picks"].as_u64(),
            r["rejects"].as_u64(),
            r["stacks"].as_u64()
        ),
        (Some(0), Some(2), Some(1))
    );
    assert_eq!(
        json1(&tools, "list_stacks", json!({ "shoot_id": 1 }))
            .as_array()
            .unwrap()[0]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn recipes_version_per_photo_in_one_batch() {
    let (_tmp, tools) = library(2);
    let ids = ids(&tools);
    assert_eq!(
        json1(&tools, "read_recipe", json!({ "photo_id": ids[0] })),
        Value::Null
    );
    let recipe = json!({ "schema": 1, "global": { "exposure": 0.3 } });
    let r = json1(
        &tools,
        "write_recipe",
        json!({ "photo_ids": ids, "recipe": recipe }),
    );
    assert_eq!(r["versions"][ids[0].to_string()], 1);
    json1(
        &tools,
        "write_recipe",
        json!({ "photo_ids": [ids[0]], "recipe": { "schema": 1 } }),
    );
    let v = json1(&tools, "read_recipe", json!({ "photo_id": ids[0] }));
    assert_eq!(
        (v["version"].as_i64(), v["author"].as_str()),
        (Some(2), Some("ai"))
    );
    assert_eq!(
        json1(&tools, "read_recipe", json!({ "photo_id": ids[1] }))["recipe"]["global"],
        recipe["global"]
    );
    // A setting outside `global` is a mistake a model can fix, not something to keep silently.
    let err = tools
        .call(
            "write_recipe",
            json!({ "photo_ids": [ids[0]], "recipe": { "exposure": 0.3 } }),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("global"), "{err}");
}

fn luma(img: &image::RgbImage) -> f64 {
    let p = img.as_raw();
    p.chunks(3)
        .map(|c| 0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64)
        .sum::<f64>()
        / (p.len() / 3) as f64
}

#[test]
fn auto_edit_sets_lightrooms_auto_sliders_as_one_undoable_batch() {
    let (_tmp, tools) = library(3);
    let ids = ids(&tools);
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [ids[0]], "set": { "saturation": 15, "exposure": 2.0 } }),
    );
    let r = json1(&tools, "auto_edit", json!({ "shoot_id": 1 }));
    assert_eq!(r["photos"].as_object().unwrap().len(), 3, "{r}");
    let g = &json1(&tools, "read_recipe", json!({ "photo_id": ids[0] }))["recipe"]["global"];
    // Auto replaces its own sliders (the +2 EV) and keeps the rest.
    assert_eq!(g["saturation"], 15.0, "{g}");
    assert!(g["exposure"].as_f64().unwrap_or(0.0) < 2.0, "{g}");
    // Auto twice gives the same answer.
    let again = json1(&tools, "auto_edit", json!({ "photo_ids": [ids[0]] }));
    assert_eq!(again["photos"][ids[0].to_string()]["global"], *g);
    // The whole Shoot's Auto undoes in one step.
    json1(
        &tools,
        "undo_batch",
        json!({ "batch_id": again["batch_id"] }),
    );
    json1(&tools, "undo_batch", json!({ "batch_id": r["batch_id"] }));
    let g = &json1(&tools, "read_recipe", json!({ "photo_id": ids[0] }))["recipe"]["global"];
    assert_eq!(g["exposure"], 2.0, "{g}");
    assert!(tools.call("auto_edit", json!({})).is_err());
}

#[test]
fn every_lightroom_panel_renders_through_the_tools() {
    let (_tmp, tools) = library(1);
    let id = ids(&tools)[0];
    let plain = jpeg(&call(&tools, "render_photo", json!({ "photo_id": id }))[1]);
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [id], "set": {
            "curve.rgb": [[0, 30], [255, 255]], "grading.shadows.hue": 220,
            "grading.shadows.sat": 40, "calibration.blue.sat": 30, "clarity": 40,
            "dehaze": 20, "vignette": -50, "grain": 30, "sharpen": 60 } }),
    );
    let v = json1(&tools, "read_recipe", json!({ "photo_id": id }));
    assert_eq!(
        v["recipe"]["global"]["curve"]["rgb"],
        json!([[0.0, 30.0], [255.0, 255.0]])
    );
    assert_eq!(v["recipe"]["global"]["clarity"], 40.0);
    let edited = jpeg(&call(&tools, "render_photo", json!({ "photo_id": id }))[1]);
    assert_eq!(edited.dimensions(), plain.dimensions());
    let diff: f64 = plain
        .pixels()
        .zip(edited.pixels())
        .map(|(a, b)| (a[2] as f64 - b[2] as f64).abs())
        .sum::<f64>()
        / (plain.width() * plain.height()) as f64;
    assert!(diff > 3.0, "the settings show in the render: {diff}");
}

#[test]
fn white_balance_presets_and_eyedropper() {
    let (_tmp, tools) = library(1);
    let id = ids(&tools)[0];
    let wb = |args: serde_json::Value| json1(&tools, "white_balance", args);
    let auto = wb(json!({ "photo_ids": [id], "preset": "auto", "dry_run": true }));
    assert!(auto["photos"][id.to_string()]["temp"].is_number());
    let picked = wb(json!({ "photo_ids": [id], "at": { "x": 0.5, "y": 0.5 } }));
    let temp = picked["photos"][id.to_string()]["temp"].clone();
    let v = json1(&tools, "read_recipe", json!({ "photo_id": id }));
    assert_eq!(v["recipe"]["global"]["temp"], temp);
    wb(json!({ "photo_ids": [id], "preset": "as_shot" }));
    let v = json1(&tools, "read_recipe", json!({ "photo_id": id }));
    assert!(v["recipe"]["global"]["temp"].is_null());
    // The fixtures are JPEGs: Kelvin presets need a RAW.
    let err = tools
        .call(
            "white_balance",
            json!({ "photo_ids": [id], "preset": "tungsten" }),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("RAW"), "{err}");
}

#[test]
fn edit_recipe_changes_settings_and_render_shows_them() {
    let (tmp, tools) = library(2);
    let ids = ids(&tools);
    let before = jpeg(&call(&tools, "render_photo", json!({ "photo_id": ids[0] }))[1]);
    let r = json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": ids, "set": { "exposure": 0.5, "hsl.blue.sat": -20 } }),
    );
    assert_eq!(r["photos"][ids[0].to_string()]["version"], 1);
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [ids[0]], "set": { "exposure": 0.5 }, "relative": true }),
    );
    let v = json1(&tools, "read_recipe", json!({ "photo_id": ids[0] }));
    assert_eq!(v["recipe"]["global"]["exposure"], 1.0);
    assert_eq!(v["recipe"]["global"]["hsl"]["blue"]["sat"], -20.0);
    let after = jpeg(&call(&tools, "render_photo", json!({ "photo_id": ids[0] }))[1]);
    assert!(luma(&after) > luma(&before) + 10.0);
    // A draft Recipe renders without being saved.
    let draft = call(
        &tools,
        "render_photo",
        json!({ "photo_id": ids[1], "recipe": { "global": { "exposure": -2.0 } } }),
    );
    assert!(luma(&jpeg(&draft[1])) < luma(&before));
    let v1 = json1(&tools, "read_recipe", json!({ "photo_id": ids[1] }));
    assert_eq!(v1["version"], 1);
    let err = tools
        .call(
            "edit_recipe",
            json!({ "photo_ids": [ids[0]], "set": { "clarty": 10 } }),
        )
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("highlights") && err.contains("clarity"),
        "{err}"
    );

    let out = tmp.path().join("2026-10-18_export");
    let e = json1(
        &tools,
        "export_photos",
        json!({ "photo_ids": [ids[0], ids[0]], "folder": out, "long_edge": 200 }),
    );
    let paths: Vec<&str> = e["exported"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["path"].as_str().unwrap())
        .collect();
    assert!(
        paths[0].ends_with("/2026-10-12_tahoe_p00.jpg")
            && paths[1].ends_with("/2026-10-12_tahoe_p00-2.jpg"),
        "{paths:?}"
    );
    let img = image::open(paths[0]).unwrap().to_rgb8();
    assert_eq!(img.width().max(img.height()), 200);
}

#[test]
fn mistakes_come_back_as_messages_a_model_can_act_on() {
    let (_tmp, tools) = library(1);
    for (name, args, want) in [
        ("nope", json!({}), "unknown tool"),
        ("get_preview", json!({}), "missing field `photo_id`"),
        (
            "get_preview",
            json!({ "photo_id": 1, "size": 9 }),
            "unknown field `size`",
        ),
        (
            "get_preview",
            json!({ "photo_id": 99 }),
            "no Photo with id 99",
        ),
        (
            "set_verdict",
            json!({ "photo_ids": [99], "verdict": "pick" }),
            "no Photo with id 99",
        ),
        (
            "set_verdict",
            json!({ "photo_ids": [1], "verdict": "maybe" }),
            "unknown variant",
        ),
        (
            "write_recipe",
            json!({ "photo_ids": [1], "recipe": 3 }),
            "JSON object",
        ),
        (
            "list_stacks",
            json!({ "shoot_id": 7 }),
            "no Shoot with id 7",
        ),
        (
            "cull_shoot",
            json!({ "provider": "gpt", "check": "model" }),
            "unknown provider",
        ),
    ] {
        let e = tools.call(name, args).unwrap_err();
        assert!(e.0.contains(want), "{name}: {e}");
    }
}

#[test]
fn import_can_auto_tone_and_queue_the_cull() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("card");
    for i in 0..2 {
        write_jpeg(&src.join(format!("p{i}.jpg")), 320, 240, i * 5 + 7);
    }
    let tools = Tools::new(tmp.path().join("Argus.library"));
    let r = json1(
        &tools,
        "import_folder",
        json!({ "folder": src, "auto": true, "cull": true }),
    );
    assert_eq!(r["imported"], 2, "{r}");
    for id in ids(&tools) {
        let rec = json1(&tools, "read_recipe", json!({ "photo_id": id }));
        let global = rec["recipe"]["global"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        assert!(
            global
                .values()
                .any(|v| v.as_f64().is_some_and(|x| x != 0.0)),
            "Auto left Photo {id} untouched: {rec}"
        );
    }
    let queued = json1(&tools, "list_jobs", json!({ "status": "queued" }));
    let kinds: Vec<&str> = queued
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|j| j["kind"].as_str())
        .collect();
    assert!(
        // The cull's rules run at once (check_shoot); a model cull would be cull_shoot.
        kinds.contains(&"check_shoot") && kinds.contains(&"suggest_shoot"),
        "{kinds:?}"
    );
}
