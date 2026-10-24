//! The command seam every UI transport (HTTP bridge, Tauri app) sits on: names in, JSON out.

use argus_engine::commands::{dispatch, library_file};
use argus_engine::tools::Tools;
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

#[test]
fn import_list_flag_and_edit_through_commands() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_tahoe");
    write_jpeg(&src.join("a.jpg"), 640, 480, 3);
    write_jpeg(&src.join("b.jpg"), 480, 640, 5);
    let tools = Tools::new(dir.path().join("lib"));

    assert_eq!(
        dispatch(&tools, "list_shoots", Value::Null).unwrap(),
        json!([])
    );
    let report = dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    assert_eq!(report["imported"], 2);
    let shoot = report["shoot_id"].as_i64().unwrap();
    assert_eq!(report["shoot_name"], "2026-10-12_tahoe");

    let photos = dispatch(&tools, "photos", json!({ "shoot_id": shoot })).unwrap();
    let photos = photos.as_array().unwrap();
    assert_eq!(photos.len(), 2);
    let preview = photos[0]["preview"].as_str().unwrap();
    assert!(
        preview.starts_with("previews/"),
        "preview is Library-relative: {preview}"
    );
    assert!(library_file(tools.library(), preview).unwrap().is_file());

    let id = photos[0]["id"].clone();
    dispatch(&tools, "flag", json!({ "ids": [id], "verdict": "pick" })).unwrap();
    let picks = dispatch(&tools, "photos", json!({ "verdict": "pick" })).unwrap();
    assert_eq!(picks.as_array().unwrap().len(), 1);
    assert_eq!(picks[0]["verdict_by"], "person");

    // Undo puts the AI's Verdict back, which the set_verdict tool would refuse to do.
    let undo = json!({ "ids": [id], "verdict": "unflagged", "author": "ai" });
    dispatch(&tools, "flag", undo).unwrap();
    let after = dispatch(&tools, "photos", json!({})).unwrap();
    assert_eq!(after[0]["verdict_by"], "ai");

    let edit = json!({ "photo_id": id, "recipe": { "exposure": 0.3 } });
    assert_eq!(dispatch(&tools, "edit", edit).unwrap()["version"], 1);
    let r = dispatch(&tools, "read_recipe", json!({ "photo_id": id })).unwrap();
    assert_eq!(r["recipe"]["exposure"], 0.3);
    assert_eq!(r["author"], "person");
    // The grid re-renders a thumbnail when its Photo's stamp changes.
    let edited = dispatch(&tools, "photos", json!({})).unwrap();
    let stamp = |v: &Value, id: &Value| {
        v.as_array()
            .unwrap()
            .iter()
            .find(|p| &p["id"] == id)
            .unwrap()["edit"]
            .clone()
    };
    let first = stamp(&edited, &id);
    assert!(first.as_str().unwrap().starts_with("1@"), "{first}");
    assert!(edited
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["edit"].is_null()));
    let again = json!({ "photo_id": id, "recipe": { "exposure": 0.5 } });
    dispatch(&tools, "edit", again).unwrap();
    assert_ne!(
        stamp(&dispatch(&tools, "photos", json!({})).unwrap(), &id),
        first
    );
}

#[test]
fn images_come_back_as_data_urls_and_providers_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_one");
    write_jpeg(&src.join("a.jpg"), 640, 480, 3);
    let tools = Tools::new(dir.path().join("lib"));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    let id = dispatch(&tools, "photos", json!({})).unwrap()[0]["id"].clone();

    let r = dispatch(
        &tools,
        "render_photo",
        json!({ "photo_id": id, "long_edge": 256 }),
    )
    .unwrap();
    assert_eq!(r["photo_id"], id);
    let img = r["images"][0].as_str().unwrap();
    assert!(
        img.starts_with("data:image/jpeg;base64,/9j/"),
        "{}",
        &img[..40]
    );

    let p = dispatch(&tools, "providers", json!({})).unwrap();
    let ids: Vec<_> = p["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].clone())
        .collect();
    // Claude API joins them only when a key is set up (env or Keychain).
    assert_eq!(ids[..2], ["ollama", "claude-code"]);
    assert!(ids.len() == 2 || ids[2] == "claude-api", "{ids:?}");
    assert!(p["default"].is_string());
}

#[test]
fn render_detail_goes_past_the_tool_cap_to_full_size() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_one");
    write_jpeg(&src.join("a.jpg"), 2400, 1600, 3);
    let tools = Tools::new(dir.path().join("lib"));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    let id = dispatch(&tools, "photos", json!({})).unwrap()[0]["id"].clone();
    let args = json!({ "photo_id": id, "long_edge": 6000, "recipe": { "exposure": 0.3 } });

    let capped = dispatch(&tools, "render_photo", args.clone()).unwrap();
    assert_eq!(capped["width"], 1536);
    let detail = dispatch(&tools, "render_detail", args).unwrap();
    assert_eq!(
        (detail["width"].clone(), detail["height"].clone()),
        (json!(2400), json!(1600))
    );
    assert_eq!(detail["recipe"]["exposure"], 0.3);
    assert!(detail["images"][0]
        .as_str()
        .unwrap()
        .starts_with("data:image/jpeg;base64,"));
}

#[test]
fn bad_calls_are_plain_errors() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(dir.path());
    let err = |c: &str, a: Value| dispatch(&tools, c, a).unwrap_err().to_string();
    let e = err("nope", json!({}));
    assert!(e.contains("unknown tool"), "{e}");
    let e = err("flag", json!({ "ids": [1], "verdict": "maybe" }));
    assert!(e.contains("bad arguments"), "{e}");
    let e = err("flag", json!({ "ids": [99], "verdict": "pick" }));
    assert!(e.contains("no Photo"), "{e}");
}

#[test]
fn library_file_stays_inside_previews() {
    let root = Path::new("/lib");
    let ok = library_file(root, "/previews/ab/c.jpg");
    assert_eq!(ok, Some(root.join("previews/ab/c.jpg")));
    assert_eq!(library_file(root, "previews/../library.db"), None);
    assert_eq!(library_file(root, "library.db"), None);
    assert_eq!(library_file(root, "previews//etc"), None);
}

#[test]
fn the_grid_gets_small_thumbnails_kept_until_the_preview_changes() {
    let dir = tempfile::tempdir().unwrap();
    write_jpeg(&dir.path().join("previews/ab/abc.jpg"), 2048, 1365, 1);
    let thumb = library_file(dir.path(), "/previews/thumbs/ab/abc.jpg").unwrap();
    assert_eq!(thumb, dir.path().join("previews/thumbs/ab/abc.jpg"));
    let img = image::open(&thumb).unwrap();
    assert_eq!((img.width(), img.height()), (480, 320));
    let made = std::fs::metadata(&thumb).unwrap().modified().unwrap();
    library_file(dir.path(), "previews/thumbs/ab/abc.jpg").unwrap();
    assert_eq!(
        std::fs::metadata(&thumb).unwrap().modified().unwrap(),
        made,
        "kept, not remade"
    );

    // A new preview (Argus's render, or a rebuild at another size) makes a new thumbnail.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_jpeg(&dir.path().join("previews/ab/abc.jpg"), 1000, 1500, 2);
    library_file(dir.path(), "previews/thumbs/ab/abc.jpg").unwrap();
    let img = image::open(&thumb).unwrap();
    assert_eq!((img.width(), img.height()), (320, 480));
    // Nothing to make one from: no thumbnail, and nothing outside the previews folder.
    assert!(!library_file(dir.path(), "previews/thumbs/cd/none.jpg")
        .unwrap()
        .exists());
    assert_eq!(
        library_file(dir.path(), "previews/thumbs/../../library.db"),
        None
    );
}

#[test]
fn a_replaced_camera_preview_falls_back_to_the_render() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("previews/ab")).unwrap();
    std::fs::write(dir.path().join("previews/ab/abc.jpg"), b"jpeg").unwrap();
    let got = library_file(dir.path(), "previews/ab/abc.camera.jpg");
    assert_eq!(got, Some(dir.path().join("previews/ab/abc.jpg")));
}

#[test]
fn card_import_finds_the_card_skips_what_is_imported_and_imports_a_pick() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("Volumes");
    let card = volumes.join("SONY_A7");
    write_jpeg(&card.join("DCIM/100MSDCF/DSC0001.JPG"), 640, 480, 3);
    write_jpeg(&card.join("DCIM/100MSDCF/DSC0002.JPG"), 640, 480, 5);
    write_jpeg(&card.join("DCIM/100MSDCF/DSC0003.JPG"), 480, 640, 7);
    std::fs::create_dir_all(volumes.join("Backup-Drive")).unwrap(); // no DCIM: not a card
    let _env = crate::env_lock();
    std::env::set_var("ARGUS_VOLUMES", &volumes);
    let tools = Tools::new(dir.path().join("lib"));

    let cards = dispatch(&tools, "find_cards", json!({})).unwrap();
    assert_eq!(cards.as_array().unwrap().len(), 1, "{cards}");
    assert_eq!(cards[0]["name"], "SONY_A7");
    let card_path = cards[0]["path"].as_str().unwrap().to_string();

    let scan = dispatch(&tools, "scan_folder", json!({ "folder": card_path })).unwrap();
    assert_eq!(
        (scan["new"].clone(), scan["imported"].clone()),
        (json!(3), json!(0))
    );
    let first = scan["photos"][0]["path"].as_str().unwrap().to_string();

    // Import only the first; a rescan then marks it as already imported.
    let files = json!([first]);
    let r = dispatch(
        &tools,
        "import_folder",
        json!({ "folder": card_path, "files": files, "shoot_name": "Pickleball" }),
    )
    .unwrap();
    assert_eq!(r["imported"], 1);
    let scan = dispatch(&tools, "scan_folder", json!({ "folder": card_path })).unwrap();
    assert_eq!(
        (scan["new"].clone(), scan["imported"].clone()),
        (json!(2), json!(1))
    );
    let marked: Vec<_> = scan["photos"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["imported"] == true)
        .map(|p| p["path"].clone())
        .collect();
    assert_eq!(marked, [json!(first)]);

    // A file outside the folder is refused.
    let outside = dir.path().join("elsewhere.jpg");
    write_jpeg(&outside, 64, 64, 9);
    let e = dispatch(
        &tools,
        "import_folder",
        json!({ "folder": card_path, "files": [outside] }),
    )
    .unwrap_err();
    assert!(e.to_string().contains("is not a photo in"), "{e}");

    let t = dispatch(
        &tools,
        "thumbnail",
        json!({ "path": first, "long_edge": 128 }),
    )
    .unwrap();
    assert!(t["images"][0]
        .as_str()
        .unwrap()
        .starts_with("data:image/jpeg;base64,"));

    // No model reachable in tests: the name falls back to the date.
    let n = dispatch(
        &tools,
        "suggest_name",
        json!({ "paths": [], "captured_at": "2026-10-12T10:00:00", "provider": "claude-code" }),
    )
    .unwrap();
    assert_eq!(
        (n["name"].clone(), n["by"].clone()),
        (json!("2026-10-12"), json!("date"))
    );

    // What the person types becomes the dated, space-free Shoot name (docs/naming.md).
    let s = dispatch(
        &tools,
        "shoot_name",
        json!({ "title": "Maya's Birthday", "captured_at": "2026-10-12T10:00:00" }),
    )
    .unwrap();
    assert_eq!(s["shoot_name"], json!("2026-10-12_Mayas-Birthday"));
    assert_eq!(s["originals"], json!("originals/2026-10-12_Mayas-Birthday"));
}

#[test]
fn raw_plus_jpeg_frames_scan_as_one_photo_from_the_raw() {
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("card/DCIM/100MSDCF");
    std::fs::create_dir_all(&card).unwrap();
    std::fs::write(card.join("DSC0001.ARW"), b"not really a raw").unwrap();
    write_jpeg(&card.join("DSC0001.JPG"), 640, 480, 3);
    write_jpeg(&card.join("DSC0002.JPG"), 640, 480, 5); // JPEG only: still a Photo
    let tools = Tools::new(dir.path().join("lib"));

    let scan = dispatch(
        &tools,
        "scan_folder",
        json!({ "folder": dir.path().join("card") }),
    )
    .unwrap();
    let mut names: Vec<(String, bool)> = scan["photos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["file_name"].as_str().unwrap().to_string(),
                p["jpeg"].is_string(),
            )
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            ("DSC0001.ARW".to_string(), true),
            ("DSC0002.JPG".to_string(), false)
        ],
        "{scan}"
    );
}

#[test]
fn raw_plus_jpeg_frames_import_as_one_photo() {
    // No real RAW in the cloud container, so the RAW fails to decode; what matters here
    // is that its JPEG is never offered or imported as a Photo of its own.
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("card");
    std::fs::create_dir_all(&card).unwrap();
    std::fs::write(card.join("DSC0001.ARW"), b"not really a raw").unwrap();
    write_jpeg(&card.join("DSC0001.JPG"), 640, 480, 3);
    let tools = Tools::new(dir.path().join("lib"));
    for args in [
        json!({ "folder": card }),
        json!({ "folder": card, "raw_only": true }),
        json!({ "folder": card, "files": [card.join("DSC0001.ARW")] }),
    ] {
        let r = dispatch(&tools, "import_folder", args.clone()).unwrap();
        assert_eq!(
            (r["found"].clone(), r["imported"].clone()),
            (json!(1), json!(0)),
            "{args}: {r}"
        );
        assert!(
            r["failed"][0]["file"]
                .as_str()
                .unwrap()
                .ends_with("DSC0001.ARW"),
            "{r}"
        );
    }
}

#[test]
fn rate_and_preview_commands_answer_through_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_rate");
    write_jpeg(&src.join("a.jpg"), 640, 480, 3);
    let tools = Tools::new(dir.path().join("lib"));
    let shoot =
        dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap()["shoot_id"].clone();
    let id = dispatch(&tools, "photos", json!({})).unwrap()[0]["id"].clone();

    let r = dispatch(&tools, "rate", json!({ "ids": [id], "stars": 4 })).unwrap();
    assert_eq!(r["rated"], 1);
    let photos = dispatch(&tools, "photos", json!({})).unwrap();
    assert_eq!(photos[0]["stars"], 4);
    assert!(dispatch(&tools, "rate", json!({ "ids": [id], "stars": 9 })).is_err());

    // A JPEG Photo has no camera preview waiting for Argus's own render.
    let pending = dispatch(&tools, "pending_previews", json!({ "shoot_id": shoot })).unwrap();
    assert_eq!(pending["pending"], 0);
    let report = dispatch(
        &tools,
        "render_previews",
        json!({ "shoot_id": shoot, "compute": "full" }),
    )
    .unwrap();
    assert!(report.is_object(), "{report}");
}

/// The engine is the only source of truth for Verdicts and stars: the UI's `flag` and `rate`
/// come back as one batch with the Photos as the Library now has them, Undo is
/// `undo_batch` on that batch, and Redo is `undo_batch` on the Undo's batch.
#[test]
fn person_flags_and_stars_are_batches_that_undo_and_redo_in_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_undo");
    write_jpeg(&src.join("a.jpg"), 640, 480, 3);
    write_jpeg(&src.join("b.jpg"), 640, 480, 7);
    let tools = Tools::new(dir.path().join("lib"));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    let photos = dispatch(&tools, "photos", json!({})).unwrap();
    let (a, b) = (photos[0]["id"].clone(), photos[1]["id"].clone());
    let ai = json!({ "ids": [a, b], "verdict": "reject", "author": "ai" });
    dispatch(&tools, "flag", ai).unwrap();
    dispatch(
        &tools,
        "rate",
        json!({ "ids": [a], "stars": 2, "author": "ai" }),
    )
    .unwrap();

    let flag = json!({ "ids": [a, b], "verdict": "pick", "label": "2 to pick" });
    let r = dispatch(&tools, "flag", flag).unwrap();
    assert_eq!(r["flagged"], 2);
    let batch = r["batch_id"].as_i64().expect("a person's flag is a batch");
    let states = r["photos"].as_array().unwrap();
    assert_eq!(states.len(), 2);
    assert_eq!(states[0]["id"], a);
    assert_eq!(states[0]["verdict"], "pick");
    assert_eq!(states[0]["verdict_by"], "person");
    assert_eq!(states[0]["stars"], 2);
    assert_eq!(states[0]["stars_by"], "ai");

    let undo = dispatch(&tools, "undo_batch", json!({ "batch_id": batch })).unwrap();
    let now = dispatch(&tools, "photo_states", json!({ "ids": [a, b] })).unwrap();
    assert_eq!(now[0]["verdict"], "reject");
    assert_eq!(
        now[0]["verdict_by"], "ai",
        "Undo puts the AI's Verdict back"
    );
    assert_eq!(now[1]["verdict"], "reject");

    let redo = json!({ "batch_id": undo["batch_id"] });
    dispatch(&tools, "undo_batch", redo).unwrap();
    let now = dispatch(&tools, "photo_states", json!({ "ids": [b] })).unwrap();
    assert_eq!(now[0]["verdict"], "pick");
    assert_eq!(now[0]["verdict_by"], "person", "Redo is the Undo undone");

    let r = dispatch(&tools, "rate", json!({ "ids": [a], "stars": 5 })).unwrap();
    assert_eq!(r["photos"][0]["stars"], 5);
    assert_eq!(r["photos"][0]["stars_by"], "person");
    let batch = r["batch_id"].clone();
    dispatch(&tools, "undo_batch", json!({ "batch_id": batch })).unwrap();
    let now = dispatch(&tools, "photo_states", json!({ "ids": [a] })).unwrap();
    assert_eq!(
        (now[0]["stars"].clone(), now[0]["stars_by"].clone()),
        (json!(2), json!("ai"))
    );

    // The person's own Verdict still wins over a later AI write through the tool layer.
    let tool = json!({ "photo_ids": [b], "verdict": "reject" });
    let _ = dispatch(&tools, "set_verdict", tool);
    let now = dispatch(&tools, "photo_states", json!({ "ids": [b] })).unwrap();
    assert_eq!(now[0]["verdict"], "pick");
}

#[test]
fn a_flag_can_join_an_open_batch_so_keep_one_reject_the_rest_undoes_once() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_keep");
    write_jpeg(&src.join("a.jpg"), 64, 48, 3);
    write_jpeg(&src.join("b.jpg"), 64, 48, 7);
    let tools = Tools::new(dir.path().join("lib"));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    let photos = dispatch(&tools, "photos", json!({})).unwrap();
    let (a, b) = (photos[0]["id"].clone(), photos[1]["id"].clone());

    let keep = json!({ "ids": [a], "verdict": "pick", "label": "Kept 1 of 2" });
    let batch = dispatch(&tools, "flag", keep).unwrap()["batch_id"].clone();
    let rest = json!({ "ids": [b], "verdict": "reject", "batch_id": batch });
    assert_eq!(dispatch(&tools, "flag", rest).unwrap()["batch_id"], batch);
    let undo = dispatch(&tools, "undo_batch", json!({ "batch_id": batch })).unwrap();
    assert_eq!(undo["label"], "Kept 1 of 2");
    let now = dispatch(&tools, "photo_states", json!({ "ids": [a, b] })).unwrap();
    assert_eq!(
        (now[0]["verdict"].clone(), now[1]["verdict"].clone()),
        (json!("unflagged"), json!("unflagged"))
    );
}

#[test]
fn a_person_edit_is_a_batch_so_undo_and_redo_are_engine_calls() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_edit");
    write_jpeg(&src.join("a.jpg"), 64, 48, 3);
    let tools = Tools::new(dir.path().join("lib"));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    let id = dispatch(&tools, "photos", json!({})).unwrap()[0]["id"].clone();
    let exposure = |tools: &Tools| {
        let r = dispatch(tools, "read_recipe", json!({ "photo_id": id })).unwrap();
        r["recipe"]["exposure"].as_f64().unwrap_or(0.0)
    };

    let first = json!({ "photo_id": id, "recipe": { "exposure": 0.3 }, "label": "Exposure" });
    dispatch(&tools, "edit", first).unwrap();
    let drag = json!({ "photo_id": id, "recipe": { "exposure": 0.8 }, "label": "Exposure" });
    let r = dispatch(&tools, "edit", drag).unwrap();
    let batch = r["batch_id"].as_i64().expect("an edit is a batch");

    let undo = dispatch(&tools, "undo_batch", json!({ "batch_id": batch })).unwrap();
    assert_eq!(undo["label"], "Exposure");
    assert_eq!(undo["recipes"], json!([id]));
    assert_eq!(
        exposure(&tools),
        0.3,
        "Undo goes back one drag, not to the start"
    );
    dispatch(
        &tools,
        "undo_batch",
        json!({ "batch_id": undo["batch_id"] }),
    )
    .unwrap();
    assert_eq!(exposure(&tools), 0.8, "Redo is the Undo undone");
}

/// The Look screen: the person tunes and removes the Shoot's Look through the Looks tools,
/// each as a batch of their own (`as_person`), and ⌘Z is `undo_batch` on it.
#[test]
fn a_person_tunes_and_removes_a_look_as_batches_that_undo() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_look");
    write_jpeg(&src.join("a.jpg"), 64, 48, 3);
    write_jpeg(&src.join("b.jpg"), 64, 48, 7);
    let tools = Tools::new(dir.path().join("lib"));
    let shoot =
        dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap()["shoot_id"].clone();
    let photos = dispatch(&tools, "photos", json!({})).unwrap();
    let a = photos[0]["id"].clone();
    let look_of = |id: &Value| {
        dispatch(&tools, "read_recipe", json!({ "photo_id": id })).unwrap()["recipe"]["look"]
            .clone()
    };
    let make = json!({ "shoot_id": shoot, "name": "Warm film", "set": { "temp": 10 } });
    let made = dispatch(&tools, "set_look", make).unwrap();
    let look = made["look"]["id"].clone();
    assert_eq!(
        look_of(&a),
        look,
        "a new Look goes on the Shoot (no Picks: all of it)"
    );

    let tune = json!({ "tool": "set_look", "label": "Look Contrast", "args": {
        "shoot_id": shoot, "look_id": look, "set": { "contrast": 20 }, "relative": true } });
    let r = dispatch(&tools, "as_person", tune).unwrap();
    let batch = r["batch_id"].clone();
    let looks = dispatch(&tools, "list_looks", json!({ "shoot_id": shoot })).unwrap();
    assert_eq!(looks[0]["adjust"]["contrast"], 20.0);
    let undo = dispatch(&tools, "undo_batch", json!({ "batch_id": batch })).unwrap();
    assert_eq!(undo["label"], "Look Contrast");
    let looks = dispatch(&tools, "list_looks", json!({ "shoot_id": shoot })).unwrap();
    assert!(
        looks[0]["adjust"].get("contrast").is_none(),
        "Undo put the Look back: {looks}"
    );

    let remove = json!({ "tool": "remove_look", "label": "Remove Look", "args": {
        "shoot_id": shoot, "look_id": look } });
    let r = dispatch(&tools, "as_person", remove).unwrap();
    assert_eq!(r["removed_from"].as_array().unwrap().len(), 2);
    assert_eq!(look_of(&a), Value::Null);
    dispatch(&tools, "undo_batch", json!({ "batch_id": r["batch_id"] })).unwrap();
    assert_eq!(look_of(&a), look, "Undo put the Photos back under the Look");
}

#[test]
fn redo_of_a_look_tweak_puts_the_tweak_back() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_redo");
    write_jpeg(&src.join("a.jpg"), 64, 48, 3);
    let tools = Tools::new(dir.path().join("lib"));
    let shoot =
        dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap()["shoot_id"].clone();
    let look = dispatch(
        &tools,
        "set_look",
        json!({ "shoot_id": shoot, "name": "Film" }),
    )
    .unwrap()["look"]["id"]
        .clone();
    let tune = json!({ "tool": "set_look", "label": "Look", "args": {
        "shoot_id": shoot, "look_id": look, "set": { "contrast": 7 } } });
    let batch = dispatch(&tools, "as_person", tune).unwrap()["batch_id"].clone();
    let contrast = || {
        dispatch(&tools, "list_looks", json!({ "shoot_id": shoot })).unwrap()[0]["adjust"]
            ["contrast"]
            .as_f64()
            .unwrap_or(0.0)
    };
    let undo = dispatch(&tools, "undo_batch", json!({ "batch_id": batch })).unwrap();
    assert_eq!(contrast(), 0.0);
    dispatch(
        &tools,
        "undo_batch",
        json!({ "batch_id": undo["batch_id"] }),
    )
    .unwrap();
    assert_eq!(contrast(), 7.0, "Redo is the Undo undone, for Looks too");
}

#[test]
fn reveal_shows_an_export_folder_and_refuses_anything_else() {
    std::env::set_var("ARGUS_NO_REVEAL", "1");
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(dir.path().join("lib"));
    let out = dir.path().join("exports");
    std::fs::create_dir_all(&out).unwrap();
    let r = dispatch(&tools, "reveal", json!({ "path": out })).unwrap();
    assert_eq!(r["path"], json!(out));
    let file = out.join("a.jpg");
    std::fs::write(&file, b"x").unwrap();
    assert!(dispatch(&tools, "reveal", json!({ "path": file })).is_err());
    assert!(dispatch(&tools, "reveal", json!({ "path": dir.path().join("nope") })).is_err());
}

#[test]
fn a_bad_api_key_is_refused_before_it_reaches_the_keychain() {
    let tools = Tools::new(tempfile::tempdir().unwrap().path().join("lib"));
    for key in [
        "",
        "hello",
        "sk-ant-short",
        "sk-ant-abcdefghijklmnop\" -s x",
    ] {
        let e = dispatch(&tools, "set_api_key", json!({ "key": key })).unwrap_err();
        assert!(e.to_string().contains("sk-ant-"), "{key}: {e}");
    }
    assert!(argus_engine::providers::check_api_key(" sk-ant-api03-abcdefghij_KLMN-op \n").is_ok());
}

#[test]
fn edited_previews_are_cached_on_disk_until_the_recipe_changes() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_one");
    write_jpeg(&src.join("a.jpg"), 640, 480, 3);
    let tools = Tools::new(dir.path().join("lib"));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    let id = dispatch(&tools, "photos", json!({})).unwrap()[0]["id"].clone();
    let key = id.to_string();
    let preview = |edge: u32| {
        let r = dispatch(
            &tools,
            "edited_previews",
            json!({ "photo_ids": [id], "long_edge": edge }),
        )
        .unwrap();
        r[&key].as_str().unwrap().to_string()
    };

    dispatch(
        &tools,
        "edit",
        json!({ "photo_id": id, "recipe": { "exposure": 0.3 } }),
    )
    .unwrap();
    let first = preview(256);
    let file = library_file(tools.library(), &first).unwrap();
    assert!(file.is_file(), "{first} is served like any preview");
    assert_eq!(image::open(&file).unwrap().width(), 256);
    let written = std::fs::metadata(&file).unwrap().modified().unwrap();
    // The same Recipe again: the file on disk, not a second render.
    assert_eq!(preview(256), first);
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        written
    );

    dispatch(
        &tools,
        "edit",
        json!({ "photo_id": id, "recipe": { "exposure": -0.5 } }),
    )
    .unwrap();
    let second = preview(256);
    assert_ne!(second, first, "a new Recipe is a new file");
    assert!(!file.exists(), "the old edit's preview is dropped");

    dispatch(&tools, "clear_cache", json!({})).unwrap();
    assert!(!library_file(tools.library(), &second).unwrap().exists());
}

#[test]
fn flag_and_rate_with_no_ids_make_no_undo_step_and_repeats_count_once() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("2026-10-12_tahoe");
    write_jpeg(&src.join("a.jpg"), 64, 48, 3);
    let tools = Tools::new(dir.path().join("lib"));
    dispatch(&tools, "import_folder", json!({ "folder": src })).unwrap();
    let id = dispatch(&tools, "photos", json!({})).unwrap()[0]["id"].clone();
    let last = || {
        argus_engine::library::Library::open(tools.library())
            .unwrap()
            .last_batch(None)
            .unwrap()
            .map(|b| b.0)
    };
    let before = last();

    let r = dispatch(&tools, "flag", json!({ "ids": [], "verdict": "pick" })).unwrap();
    assert_eq!(r["flagged"], 0);
    assert_eq!(r["batch_id"], Value::Null);
    let r = dispatch(&tools, "rate", json!({ "ids": [], "stars": 3 })).unwrap();
    assert_eq!(r["rated"], 0);
    assert_eq!(last(), before, "nothing changed, so Undo has no new step");

    let r = dispatch(
        &tools,
        "flag",
        json!({ "ids": [id, id, id], "verdict": "pick" }),
    )
    .unwrap();
    assert_eq!(r["flagged"], 1);
    assert_eq!(r["photos"].as_array().unwrap().len(), 1);
    let r = dispatch(&tools, "rate", json!({ "ids": [id, id], "stars": 4 })).unwrap();
    assert_eq!(r["rated"], 1);
}
