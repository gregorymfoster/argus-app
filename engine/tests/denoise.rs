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
    // The person has it open, so an Agent edit's Denoise is made straight away.
    argus_engine::commands::dispatch(&tools, "focus_photo", json!({ "photo_id": id })).unwrap();
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

    // Clear cache drops the base; an export that needs it makes it, uses it and drops it
    // again (a whole exported Shoot would otherwise fill the disk).
    json1(&tools, "clear_cache", json!({}));
    assert!(denoise::ready(&photo).is_none());
    let export = |folder: &str| -> image::RgbImage {
        let out = tmp.path().join(folder);
        let r = json1(
            &tools,
            "export_photos",
            json!({ "photo_ids": [id], "folder": out.to_str().unwrap(), "long_edge": 800 }),
        );
        let file = r["exported"][0]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}"));
        image::open(file).unwrap().to_rgb8()
    };
    let denoised = export("out");
    assert!(
        denoise::ready(&photo).is_none(),
        "the export's own base was dropped"
    );
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [id], "set": { "denoise": 0 } }),
    );
    let plain = export("out-plain");
    assert!(
        roughness(&denoised) < roughness(&plain),
        "the export was denoised: {} vs {}",
        roughness(&denoised),
        roughness(&plain)
    );
    std::env::remove_var("ARGUS_DENOISE_FAKE");
}

/// Auto Denoise: AI edits (here Auto) give a noisy RAW the Amount its measured noise calls
/// for and leave a clean one alone; the base waits until the person opens the Photo; and a
/// Denoise someone set is never overridden.
#[test]
fn auto_sets_denoise_from_the_raws_own_noise() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw-a7rv");
    // DSC08006: a dim room at high ISO; DSC07856: a bright, clean frame.
    let (noisy, clean) = (dir.join("DSC08006.ARW"), dir.join("DSC07856.ARW"));
    if !noisy.is_file() || !clean.is_file() {
        eprintln!("auto denoise: needs fixtures/raw-a7rv DSC08006 and DSC07856, skipping");
        return;
    }
    let _env = crate::env_lock();
    std::env::set_var("ARGUS_DENOISE_FAKE", "1");
    let tmp = tempfile::tempdir().unwrap();
    let card = tmp.path().join("card");
    std::fs::create_dir_all(&card).unwrap();
    for f in [&noisy, &clean] {
        std::fs::copy(f, card.join(f.file_name().unwrap())).unwrap();
    }
    let tools = Tools::new(tmp.path().join("Argus.library"));
    json1(
        &tools,
        "import_folder",
        json!({ "folder": card.to_str().unwrap(), "shoot_name": "2026-11-01_Auto-Denoise" }),
    );
    let lib = Library::open(tools.library()).unwrap();
    let photos = lib.list_photos(&Default::default()).unwrap();
    let id = |name: &str| photos.iter().find(|p| p.file_name == name).unwrap().id;
    let (n, c) = (id("DSC08006.ARW"), id("DSC07856.ARW"));
    drop(lib);
    let denoise_of = |photo: i64| -> Option<f64> {
        let lib = Library::open(tools.library()).unwrap();
        let r = lib.read_recipe(photo).unwrap()?;
        r.recipe["global"]["denoise"].as_f64()
    };

    json1(&tools, "auto_edit", json!({ "photo_ids": [n, c] }));
    let amount = denoise_of(n).expect("the noisy frame got Denoise");
    assert!((25.0..=50.0).contains(&amount), "{amount}");
    assert_eq!(denoise_of(c), None, "the clean frame was left alone");
    let status = |photo: i64| -> Value {
        argus_engine::commands::dispatch(&tools, "denoise_status", json!({ "photo_id": photo }))
            .unwrap()
    };
    assert!(
        status(n)["job"].is_null(),
        "no base until it is opened: {}",
        status(n)
    );
    argus_engine::commands::dispatch(
        &tools,
        "focus_photo",
        json!({ "photo_id": n, "next_ids": [c] }),
    )
    .unwrap();
    assert!(
        status(n)["job"].is_object(),
        "opening it queued the base: {}",
        status(n)
    );
    assert!(
        status(c)["job"].is_null(),
        "the clean frame has nothing to build"
    );

    // Someone turns it off; Auto again keeps their 0.
    json1(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [n], "set": { "denoise": 0 } }),
    );
    json1(&tools, "auto_edit", json!({ "photo_ids": [n] }));
    assert_eq!(denoise_of(n), Some(0.0));
    std::env::remove_var("ARGUS_DENOISE_FAKE");
}
