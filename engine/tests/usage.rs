//! The usage log: what happened to a Photo survives Undo and new edits, and exports as
//! bench cases in fixtures/bench's layout.

use argus_engine::commands::dispatch;
use argus_engine::library::Library;
use argus_engine::tools::{Part, Tools};
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

fn call(tools: &Tools, name: &str, args: Value) -> Value {
    match tools.call(name, args).unwrap().remove(0) {
        Part::Json(v) => v,
        Part::Jpeg(_) => panic!("{name} returned an image first"),
    }
}

#[test]
fn records_takes_choices_and_exports_as_bench_cases() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_tahoe");
    write_jpeg(&src.join("a.jpg"), 320, 240, 3);
    write_jpeg(&src.join("b.jpg"), 240, 320, 5);
    let tools = Tools::new(dir.path().join("lib"));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    let photos = dispatch(&tools, "photos", json!({})).unwrap();
    let (a, b) = (
        photos[0]["id"].as_i64().unwrap(),
        photos[1]["id"].as_i64().unwrap(),
    );

    // The AI culls, the person overrules one.
    call(
        &tools,
        "set_verdict",
        json!({ "photo_ids": [a, b], "verdict": "reject", "reason": "soft" }),
    );
    dispatch(&tools, "flag", json!({ "ids": [a], "verdict": "pick" })).unwrap();

    // Two takes; the person accepts the second, nudges it, undoes a nudge, exports.
    let takes = {
        let mut lib = Library::open(tools.library()).unwrap();
        lib.replace_suggestions(
            a,
            None,
            None,
            &[
                (
                    "Bright".into(),
                    "lift".into(),
                    json!({ "exposure": 0.8 }),
                    None,
                ),
                (
                    "Warm".into(),
                    "warm".into(),
                    json!({ "exposure": 0.4, "temp": 10.0 }),
                    None,
                ),
            ],
        )
        .unwrap()
    };
    call(
        &tools,
        "dismiss_suggestion",
        json!({ "suggestion_id": takes[0] }),
    );
    call(
        &tools,
        "accept_suggestion",
        json!({ "suggestion_id": takes[1] }),
    );
    let edit = json!({ "photo_id": a, "recipe": { "exposure": 0.5, "temp": 10.0 } });
    dispatch(&tools, "edit", edit).unwrap();
    let edit = json!({ "photo_id": a, "recipe": { "exposure": 0.9, "temp": 10.0 } });
    let wrong = dispatch(&tools, "edit", edit).unwrap()["batch_id"].clone();
    call(&tools, "undo_batch", json!({ "batch_id": wrong }));
    let out = dir.path().join("exports");
    call(
        &tools,
        "export_photos",
        json!({ "photo_ids": [a], "folder": out, "long_edge": 200 }),
    );

    let log = call(&tools, "usage_log", json!({ "photo_id": a }));
    let h = &log["history"];
    assert_eq!(
        h["start"],
        Value::Null,
        "never edited before: the default develop"
    );
    assert_eq!(h["takes"].as_array().unwrap().len(), 2);
    assert_eq!(h["takes"][0]["status"], "dismissed");
    assert_eq!(h["takes"][1]["status"], "accepted");
    assert!(h["takes"][1]["provider"].is_string() && h["takes"][1]["model"].is_string());
    assert_eq!(h["kept"], 1, "built on the accepted take: {h}");
    assert_eq!(h["moved"], json!({ "exposure": 0.1 }), "{h}");
    assert_eq!(h["final_recipe"]["exposure"], 0.5);
    assert_eq!(h["ai_verdict"], "reject");
    assert_eq!(h["person_verdict"], "pick");
    assert!(h["export"]["path"].as_str().unwrap().ends_with(".jpg"));

    // As bench cases: one edit case with renders, one cull Shoot with both Photos.
    let bench = dir.path().join("bench");
    let r = call(
        &tools,
        "export_usage_bench",
        json!({ "folder": bench, "long_edge": 160 }),
    );
    assert_eq!(
        (r["edit_cases"].as_u64(), r["cull_shoots"].as_u64()),
        (Some(1), Some(1)),
        "{r}"
    );
    let cases: Value =
        serde_json::from_slice(&std::fs::read(bench.join("edit/cases.json")).unwrap()).unwrap();
    let case = &cases["cases"][0];
    assert_eq!(case["id"], "2026-10-12_tahoe_a");
    for key in ["greg", "camera"] {
        assert!(
            bench
                .join("edit")
                .join(case[key].as_str().unwrap())
                .is_file(),
            "{key}"
        );
    }
    let labels: Value = serde_json::from_slice(
        &std::fs::read(bench.join("cull/2026-10-12_tahoe/labels.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(labels["picks"], 1);
    let labels = labels["labels"].as_array().unwrap();
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().all(|l| l["ai_verdict"] == "reject"));
    for l in labels {
        let image = bench
            .join("cull/2026-10-12_tahoe")
            .join(l["image"].as_str().unwrap());
        assert!(image.is_file(), "{}", image.display());
    }
    // The bench's own exports stay out of the log.
    assert_eq!(call(&tools, "usage_log", json!({}))["exports"], 1);

    // Off means nothing new is written; clear empties it.
    let before = call(&tools, "usage_log", json!({ "recording": false }))["events"].clone();
    dispatch(&tools, "flag", json!({ "ids": [b], "verdict": "pick" })).unwrap();
    assert_eq!(call(&tools, "usage_log", json!({}))["events"], before);
    let cleared = call(
        &tools,
        "usage_log",
        json!({ "clear": true, "recording": true }),
    );
    assert_eq!(
        (cleared["events"].as_i64(), cleared["recording"].as_bool()),
        (Some(0), Some(true))
    );
}
