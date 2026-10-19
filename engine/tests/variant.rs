//! A harness variant (ARGUS_VARIANT, here `Tools::with_variant`) swaps the fresh Suggestions
//! prompt, takes and rules without a rebuild.

use argus_engine::agent::{Chat, Msg};
use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::tools::{Part, Tools};
use argus_engine::Result;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct Fake(Arc<Mutex<Vec<String>>>);

impl Chat for Fake {
    fn next(&self, system: &str, _: &[Msg]) -> Result<String> {
        self.0.lock().unwrap().push(system.to_string());
        Ok(json!({ "suggestions": [
            { "title": "x", "why": "w", "set": { "exposure": 0.3 } },
            { "title": "y", "why": "w", "set": { "saturation": -100 } }
        ] })
        .to_string())
    }
}

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match tools.call(name, args).unwrap().remove(0) {
        Part::Json(v) => v,
        p => panic!("{name}: expected JSON, got {p:?}"),
    }
}

#[test]
fn a_variant_file_replaces_the_prompt_takes_and_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    std::fs::create_dir_all(&src).unwrap();
    let data: Vec<u8> = (0..160 * 120 * 3).map(|i| 60 + (i % 120) as u8).collect();
    jpeg_encoder::Encoder::new_file(src.join("p.jpg"), 90)
        .unwrap()
        .encode(&data, 160, 120, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    let variant = tmp.path().join("v.json");
    std::fs::write(
        &variant,
        json!({
            "name": "two takes",
            "fresh": "VARIANT PROMPT for {count} takes.",
            "takes": [
                { "name": "Alpha", "brief": "a", "start": { "vignette": -33 } },
                { "name": "Beta", "brief": "b" }
            ],
            "rounds": 0,
            "keep_brightness": false,
            "adaptive": false
        })
        .to_string(),
    )
    .unwrap();

    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let s = seen.clone();
    let tools = Tools::new(tmp.path().join("Argus.library"))
        .with_variant(&variant)
        .with_chats(move |_, _, _| Ok(Box::new(Fake(s.clone())) as Box<dyn Chat>));
    tools
        .call("import_folder", json!({ "folder": src }))
        .unwrap();
    let id = json1(&tools, "list_photos", json!({}))[0]["id"].clone();
    for j in json1(&tools, "list_jobs", json!({ "status": "queued" }))
        .as_array()
        .unwrap()
    {
        json1(&tools, "cancel_job", json!({ "job_id": j["id"] }));
    }
    json1(
        &tools,
        "suggest_edits",
        json!({ "photo_ids": [id], "count": 2 }),
    );
    while jobs::run_next(&tools, Compute::Full).unwrap().is_some() {}

    let systems = seen.lock().unwrap().clone();
    assert_eq!(systems.len(), 1, "rounds 0: no review even at Full");
    assert!(systems[0].starts_with("VARIANT PROMPT for 2 takes."));
    let list = json1(&tools, "list_suggestions", json!({ "photo_id": id }));
    let titles: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Alpha", "Beta"]);
    let g = &list[0]["recipe"]["global"];
    assert_eq!(g["vignette"], -33.0, "the take's own start");
    assert_eq!(
        g["exposure"], 0.3,
        "keep_brightness off: the model's exposure"
    );
    assert_eq!(
        g["clarity"].as_f64().unwrap_or(0.0),
        0.0,
        "no Adaptive layer"
    );

    // A typo in a variant fails the job instead of silently running the built-in harness.
    std::fs::write(&variant, r#"{"frsh": "typo"}"#).unwrap();
    json1(
        &tools,
        "suggest_edits",
        json!({ "photo_ids": [id], "count": 2 }),
    );
    while jobs::run_next(&tools, Compute::Full).unwrap().is_some() {}
    let failed = json1(&tools, "list_jobs", json!({ "status": "failed" }));
    assert!(failed[0]["error"]
        .as_str()
        .unwrap()
        .contains("ARGUS_VARIANT"));
}
