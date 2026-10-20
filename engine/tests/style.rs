//! The style profile: how the person corrects kept takes before exporting, learned from the
//! usage log, goes on top of fresh Suggestions.

use argus_engine::agent::{Chat, Msg};
use argus_engine::commands::dispatch;
use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::library::Library;
use argus_engine::tools::{Part, Tools};
use argus_engine::Result;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

struct Fake(Arc<Mutex<Vec<String>>>);

impl Chat for Fake {
    fn next(&self, _: &str, msgs: &[Msg]) -> Result<String> {
        self.0.lock().unwrap().push(msgs[0].text.clone());
        Ok(
            json!({ "suggestions": [{ "title": "x", "why": "w", "set": { "exposure": 0.3 } }] })
                .to_string(),
        )
    }
}

fn call(tools: &Tools, name: &str, args: Value) -> Value {
    match tools.call(name, args).unwrap().remove(0) {
        Part::Json(v) => v,
        Part::Jpeg(_) => panic!("{name} returned an image first"),
    }
}

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

/// Ask for one fresh take on Photo `id` and return its exposure.
fn new_take_exposure(tools: &Tools, id: i64) -> f64 {
    call(
        tools,
        "suggest_edits",
        json!({ "photo_ids": [id], "count": 1 }),
    );
    while jobs::run_next(tools, Compute::Full).unwrap().is_some() {}
    let list = call(tools, "list_suggestions", json!({ "photo_id": id }));
    list[0]["recipe"]["global"]["exposure"].as_f64().unwrap()
}

#[test]
fn corrections_on_kept_takes_go_on_top_of_new_takes() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    for i in 0..5 {
        write_jpeg(&src.join(format!("p{i}.jpg")), 3 + i as u8);
    }
    let variant = tmp.path().join("v.json");
    let plain = json!({ "takes": [{ "name": "Natural", "brief": "b" }], "rounds": 0,
                        "keep_brightness": false, "adaptive": false });
    std::fs::write(&variant, plain.to_string()).unwrap();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let s = seen.clone();
    let tools = Tools::new(tmp.path().join("Argus.library"))
        .with_variant(&variant)
        .with_chats(move |_, _, _| Ok(Box::new(Fake(s.clone())) as Box<dyn Chat>));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    for j in call(&tools, "list_jobs", json!({ "status": "queued" }))
        .as_array()
        .unwrap()
    {
        call(&tools, "cancel_job", json!({ "job_id": j["id"] }));
    }
    let photos = dispatch(&tools, "photos", json!({})).unwrap();
    let ids: Vec<i64> = photos
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();

    // No corrections yet: nothing learned, takes as the model made them.
    let empty = call(&tools, "style_profile", json!({}));
    assert_eq!(empty["moves"], json!({}), "{empty}");
    assert_eq!(new_take_exposure(&tools, ids[4]), 0.3);
    assert!(!seen.lock().unwrap()[0].contains("habit"));

    // On four Photos the person keeps the Natural take, brightens and warms it, exports.
    for &id in &ids[..4] {
        let take = Library::open(tools.library())
            .unwrap()
            .replace_suggestions(
                id,
                None,
                None,
                &[(
                    "Natural".into(),
                    "n".into(),
                    json!({ "exposure": 0.2 }),
                    None,
                )],
            )
            .unwrap()[0];
        call(
            &tools,
            "accept_suggestion",
            json!({ "suggestion_id": take }),
        );
        let edit = json!({ "photo_id": id, "recipe": { "exposure": 0.6, "temp": 5.0 } });
        dispatch(&tools, "edit", edit).unwrap();
    }
    let out = tmp.path().join("exports");
    call(
        &tools,
        "export_photos",
        json!({ "photo_ids": &ids[..4], "folder": out, "long_edge": 100 }),
    );

    // Four corrections at half strength: +0.2 EV, temp +3 (2.5 rounded).
    let p = call(&tools, "style_profile", json!({}));
    assert_eq!(p["photos"], 4, "{p}");
    assert_eq!(p["moves"], json!({ "exposure": 0.2, "temp": 3.0 }), "{p}");

    // A new take gets the habit on top of the model's own exposure, and the model is told.
    assert_eq!(new_take_exposure(&tools, ids[4]), 0.5);
    let texts = seen.lock().unwrap().clone();
    assert!(
        texts.last().unwrap().contains("exposure +0.20"),
        "{}",
        texts.last().unwrap()
    );

    // A variant can pin it, or turn it off with {}.
    let off = json!({ "takes": [{ "name": "Natural", "brief": "b" }], "rounds": 0,
                      "keep_brightness": false, "adaptive": false, "style": {} });
    std::fs::write(&variant, off.to_string()).unwrap();
    assert_eq!(new_take_exposure(&tools, ids[4]), 0.3);
}

#[test]
fn a_list_of_corrections_can_be_learned_without_a_library() {
    let tmp = tempfile::tempdir().unwrap();
    let tools = Tools::new(tmp.path().join("Argus.library"));
    let c = json!({ "take": "Natural", "moved": { "exposure": 0.5, "shadows": 12 } });
    let p = call(
        &tools,
        "style_profile",
        json!({ "corrections": vec![c; 6] }),
    );
    assert_eq!(
        p["moves"],
        json!({ "exposure": 0.3, "shadows": 7.0 }),
        "{p}"
    );
    assert_eq!(p["line"], "exposure +0.30, shadows +7");
}
