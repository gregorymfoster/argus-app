//! Exemplars: a fresh Suggestion shows the model the person's own finished edit of a
//! similar scene, and only that.

use argus_engine::agent::{Chat, Msg};
use argus_engine::commands::dispatch;
use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::tools::{Part, Tools};
use argus_engine::Result;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Records each request's text and how many images came with it.
struct Fake(Arc<Mutex<Vec<(String, usize)>>>);

impl Chat for Fake {
    fn next(&self, _: &str, msgs: &[Msg]) -> Result<String> {
        self.0
            .lock()
            .unwrap()
            .push((msgs[0].text.clone(), msgs[0].images.len()));
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

/// A 160x120 JPEG: a smooth gradient in one tint, or (`busy`) small high-contrast checks.
fn write_jpeg(path: &Path, tint: [u8; 3], busy: bool) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut data = Vec::with_capacity(160 * 120 * 3);
    for y in 0..120u32 {
        for x in 0..160u32 {
            for c in tint {
                data.push(if busy {
                    if (x / 6 + y / 6) % 2 == 0 {
                        c
                    } else {
                        255 - c
                    }
                } else {
                    ((x * 200 / 160 + y / 4) as u8) / 2 + c / 2
                });
            }
        }
    }
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 160, 120, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn ask(tools: &Tools, id: i64) {
    call(
        tools,
        "suggest_edits",
        json!({ "photo_ids": [id], "count": 1 }),
    );
    while jobs::run_next(tools, Compute::Full).unwrap().is_some() {}
}

#[test]
fn a_fresh_take_sees_the_persons_edit_of_a_similar_scene() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    // Two frames of one scene, and an unrelated one.
    write_jpeg(&src.join("a.jpg"), [200, 120, 60], false);
    write_jpeg(&src.join("b.jpg"), [205, 118, 62], false);
    write_jpeg(&src.join("c.jpg"), [20, 60, 230], true);
    let variant = tmp.path().join("v.json");
    let plain = json!({ "takes": [{ "name": "Natural", "brief": "b" }], "rounds": 0,
                        "adaptive": false, "style": {} });
    std::fs::write(&variant, plain.to_string()).unwrap();
    let seen: Arc<Mutex<Vec<(String, usize)>>> = Arc::default();
    let s = seen.clone();
    let tools = Tools::new(tmp.path().join("Argus.library"))
        .with_variant(&variant)
        .with_chats(move |_, _, _| Ok(Box::new(Fake(s.clone())) as Box<dyn Chat>));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    // Import queues the Facts (embeddings) among its jobs; run them all.
    while jobs::run_next(&tools, Compute::Full).unwrap().is_some() {}
    let photos = dispatch(&tools, "photos", json!({})).unwrap();
    let id = |name: &str| {
        photos
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["file_name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    };
    let (a, b, c) = (id("a.jpg"), id("b.jpg"), id("c.jpg"));

    // Nothing edited by hand yet: no exemplar.
    ask(&tools, b);
    assert!(!seen.lock().unwrap()[0].0.contains("finished edit"));
    let plain_images = seen.lock().unwrap()[0].1;

    // The person edits a by hand; b now sees it, with one more image.
    let edit = json!({ "photo_id": a, "recipe": { "global": { "exposure": 0.6, "temp": 5.0 } } });
    dispatch(&tools, "edit", edit).unwrap();
    ask(&tools, b);
    let (text, images) = seen.lock().unwrap()[1].clone();
    assert!(text.contains("finished edit of a similar scene"), "{text}");
    assert!(text.contains("\"exposure\":0.6"), "{text}");
    assert_eq!(images, plain_images + 1);

    // The unrelated scene doesn't get it.
    ask(&tools, c);
    let other = seen.lock().unwrap()[2].0.clone();
    assert!(!other.contains("finished edit"), "{other}");

    // A variant can turn exemplars off.
    let off = json!({ "takes": [{ "name": "Natural", "brief": "b" }], "rounds": 0,
                      "adaptive": false, "style": {}, "exemplars": 0 });
    std::fs::write(&variant, off.to_string()).unwrap();
    let s = seen.clone();
    let tools = Tools::new(tmp.path().join("Argus.library"))
        .with_variant(&variant)
        .with_chats(move |_, _, _| Ok(Box::new(Fake(s.clone())) as Box<dyn Chat>));
    ask(&tools, b);
    assert!(!seen.lock().unwrap()[3].0.contains("finished edit"));
}
