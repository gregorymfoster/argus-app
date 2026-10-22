//! The Library's own Edit head: every exported RAW teaches it, through the Queue, and Auto
//! (with the head on) then adds the person's style learned from their own edits.

use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::path::Path;

fn call(tools: &Tools, name: &str, args: Value) -> Value {
    match tools.call(name, args).unwrap().remove(0) {
        Part::Json(v) => v,
        Part::Jpeg(_) => panic!("{name} returned an image first"),
    }
}

#[test]
fn exported_raws_teach_the_librarys_edit_head() {
    let raws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw");
    let Some(raw) = std::fs::read_dir(&raws).ok().and_then(|d| {
        d.flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("arw")))
    }) else {
        eprintln!("edit_head: no fixtures/raw/*.ARW, skipping");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let card = tmp.path().join("card");
    std::fs::create_dir_all(&card).unwrap();
    std::fs::copy(&raw, card.join(raw.file_name().unwrap())).unwrap();
    let tools = Tools::new(tmp.path().join("Argus.library"));
    call(
        &tools,
        "import_folder",
        json!({ "folder": card.to_str().unwrap(), "shoot_name": "2026-10-22_Head-Test" }),
    );
    for j in call(&tools, "list_jobs", json!({ "status": "queued" }))
        .as_array()
        .unwrap()
    {
        call(&tools, "cancel_job", json!({ "job_id": j["id"] }));
    }
    let empty = call(&tools, "learn_edit_head", json!({}));
    assert_eq!(empty["photos"], 0, "{empty}");
    assert!(
        !tools.library().join("edit-head.json").exists() || empty["photos"] == 0,
        "nothing learned yet"
    );

    // The person finishes it far punchier than any head would, and exports it.
    call(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [1], "set": { "contrast": 90, "temp": 30 } }),
    );
    call(
        &tools,
        "export_photos",
        json!({ "photo_ids": [1], "folder": tmp.path().join("out"), "long_edge": 256 }),
    );
    let queued = call(&tools, "list_jobs", json!({ "kind": "learn_edits" }));
    assert_eq!(queued.as_array().unwrap().len(), 1, "{queued}");
    jobs::drain(&tools, Compute::Full).unwrap();
    let done = call(&tools, "list_jobs", json!({ "kind": "learn_edits" }));
    assert_eq!(done[0]["status"], "done", "{done}");
    let learned = &done[0]["result"];
    assert_eq!(learned["photos"], 1, "{done}");
    assert_eq!(learned["read"], 1, "{done}");
    // One edit moves the head a fifth of the way (1 / (1 + 4)) toward it.
    let shipped = 36.7;
    let c = learned["mean"]["contrast"].as_f64().unwrap();
    assert!(c > shipped + 3.0, "contrast offset {c}");
    // Learning again reads nothing new: the sample is cached by Recipe.
    let again = call(&tools, "learn_edit_head", json!({}));
    assert_eq!(again["read"], 0, "{again}");
    assert_eq!(again["photos"], 1, "{again}");
}
