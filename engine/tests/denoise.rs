//! AI Denoise end to end through the tool layer: saving a Denoise Amount queues the base,
//! the Queue computes it, renders blend it in by Amount, and an export that needs it makes
//! it. The model is swapped for a box blur (`ARGUS_DENOISE_FAKE`) so a full frame takes a
//! second; `denoise::tests` checks the real model. Skips without sample RAWs.

use argus_engine::compute::Compute;
use argus_engine::library::Library;
use argus_engine::tools::{Part, Tools};
use argus_engine::{denoise, jobs};
use serde_json::{json, Value};
use std::path::Path;

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match tools.call(name, args).unwrap().remove(0) {
        Part::Json(v) => v,
        _ => panic!("{name}: expected JSON"),
    }
}

fn jpeg(tools: &Tools, photo: i64) -> image::RgbImage {
    let parts = tools
        .call(
            "render_photo",
            json!({ "photo_id": photo, "long_edge": 1024 }),
        )
        .unwrap();
    let bytes = parts
        .iter()
        .find_map(|p| match p {
            Part::Jpeg(b) => Some(b.clone()),
            _ => None,
        })
        .unwrap();
    image::load_from_memory(&bytes).unwrap().to_rgb8()
}

/// Mean absolute difference between horizontal neighbours: grain and fine detail.
fn roughness(img: &image::RgbImage) -> f64 {
    let (w, h) = img.dimensions();
    let mut sum = 0.0;
    for y in 0..h {
        for x in 1..w {
            let (a, b) = (img.get_pixel(x, y), img.get_pixel(x - 1, y));
            sum += (0..3)
                .map(|c| (a[c] as f64 - b[c] as f64).abs())
                .sum::<f64>();
        }
    }
    sum / (w as f64 * h as f64 * 3.0)
}

#[test]
fn denoise_amount_queues_the_base_and_renders_blend_it() {
    let raws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw");
    let Some(raw) = std::fs::read_dir(&raws).ok().and_then(|d| {
        d.flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("arw")))
    }) else {
        eprintln!("denoise: no fixtures/raw/*.ARW, skipping");
        return;
    };
    let _env = crate::env_lock();
    std::env::set_var("ARGUS_DENOISE_FAKE", "1");
    let tmp = tempfile::tempdir().unwrap();
    let card = tmp.path().join("card");
    std::fs::create_dir_all(&card).unwrap();
    std::fs::copy(&raw, card.join(raw.file_name().unwrap())).unwrap();
    let tools = Tools::new(tmp.path().join("Argus.library"));
    json1(
        &tools,
        "import_folder",
        json!({ "folder": card.to_str().unwrap(), "shoot_name": "2026-10-21_Denoise" }),
    );
    let id = 1;
    let plain = jpeg(&tools, id);

    // Denoise off: nothing queued.
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [id], "set": { "exposure": 0.5 } }),
    );
    let queued = |tools: &Tools| json1(tools, "list_jobs", json!({ "kind": "denoise" }));
    assert_eq!(queued(&tools).as_array().unwrap().len(), 0);

    // Turning it on queues the base; until it exists the Photo renders as before.
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [id], "set": { "denoise": 100 } }),
    );
    let jobs_now = queued(&tools);
    assert_eq!(jobs_now.as_array().unwrap().len(), 1, "{jobs_now}");
    let lib = Library::open(tools.library()).unwrap();
    let photo = lib.photo(id).unwrap().unwrap();
    assert!(denoise::ready(&photo).is_none());
    let before = jpeg(&tools, id);
    let status = |tools: &Tools| {
        argus_engine::commands::dispatch(tools, "denoise_status", json!({ "photo_id": id }))
            .unwrap()
    };
    let s = status(&tools);
    assert_eq!(
        (s["raw"].as_bool(), s["ready"].as_bool()),
        (Some(true), Some(false)),
        "{s}"
    );
    assert_eq!(s["job"]["status"], "queued", "{s}");

    jobs::drain(&tools, Compute::Full).unwrap();
    let done = queued(&tools);
    assert_eq!(done[0]["status"], "done", "{done}");
    let file = denoise::ready(&photo).expect("base written");
    let s = status(&tools);
    assert_eq!(
        (s["ready"].as_bool(), &s["job"]),
        (Some(true), &Value::Null),
        "{s}"
    );
    // Canonical on both sides: macOS temp dirs live behind the /var -> /private/var link.
    let cache = tmp
        .path()
        .canonicalize()
        .unwrap()
        .join("Argus.library/cache/denoise");
    assert!(file.canonicalize().unwrap().starts_with(&cache), "{file:?}");

    let full = jpeg(&tools, id);
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [id], "set": { "denoise": 50 } }),
    );
    let half = jpeg(&tools, id);
    let (r0, r50, r100) = (roughness(&before), roughness(&half), roughness(&full));
    eprintln!(
        "roughness: off {r0:.2}, 50 {r50:.2}, 100 {r100:.2} (plain {:.2})",
        roughness(&plain)
    );
    assert!(r100 < r50 && r50 < r0, "{r0} {r50} {r100}");
    assert_eq!(
        queued(&tools).as_array().unwrap().len(),
        1,
        "base exists: no new job"
    );

    // Clear cache drops the base; an export that needs it makes it again.
    json1(&tools, "clear_cache", json!({}));
    assert!(denoise::ready(&photo).is_none());
    let out = tmp.path().join("out");
    json1(
        &tools,
        "export_photos",
        json!({ "photo_ids": [id], "folder": out.to_str().unwrap(), "long_edge": 800 }),
    );
    assert!(denoise::ready(&photo).is_some(), "export computed the base");
    std::env::remove_var("ARGUS_DENOISE_FAKE");
}
