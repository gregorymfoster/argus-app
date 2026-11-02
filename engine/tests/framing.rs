//! Crop & straighten: the AI proposes a straighten where a frame has architecture to level
//! by, leaves frames already cropped alone, and applies its proposals as one Undo that
//! changes nothing but the crop.

use argus_engine::library::{Author, Library};
use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};

fn call(tools: &Tools, name: &str, args: Value) -> Value {
    match tools
        .call(name, args)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .remove(0)
    {
        Part::Json(v) => v,
        _ => panic!("{name}: not JSON"),
    }
}

#[test]
fn a_crooked_room_is_straightened_and_a_hand_crop_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-11-01_Party");
    std::fs::create_dir_all(&src).unwrap();
    let bench = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/bench/edit/greg/");
    // A room whose window and shelves lean about 2° (the bench's turn on it, checked by eye),
    // and a second frame the person has cropped already.
    for (from, to) in [
        ("2024-11-01_DSC07697.jpg", "A_room.jpg"),
        ("2024-06-04_DSC04396.jpg", "B_cropped.jpg"),
    ] {
        std::fs::copy(format!("{bench}{from}"), src.join(to)).unwrap();
    }
    let root = tmp.path().join("Argus.library");
    let tools = Tools::new(root.clone());
    call(&tools, "import_folder", json!({ "folder": src }));
    let photos = call(&tools, "list_photos", json!({}));
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
    let (room, cropped) = (id("A_room.jpg"), id("B_cropped.jpg"));
    let hand = json!({ "exposure": 0.2, "crop": { "x": 0.1, "y": 0.1, "w": 0.8, "h": 0.8 } });
    Library::open(&root)
        .unwrap()
        .write_recipe(cropped, &hand, Author::Person, None)
        .unwrap();
    call(
        &tools,
        "edit_recipe",
        json!({ "photo_ids": [room], "set": { "exposure": 0.3 } }),
    );

    let frames = call(
        &tools,
        "propose_framing",
        json!({ "photo_ids": [room, cropped] }),
    );
    let f = frames.as_array().unwrap();
    let angle = f[0]["crop"]["angle"]
        .as_f64()
        .unwrap_or_else(|| panic!("a turn: {frames}"));
    assert!((-3.0..=-1.0).contains(&angle), "levels the room: {frames}");
    assert!(
        f[0]["reasons"][0]
            .as_str()
            .unwrap()
            .starts_with("Straighten"),
        "{frames}"
    );
    assert_eq!(f[1]["kept"], "already cropped", "{frames}");

    let done = call(
        &tools,
        "apply_framing",
        json!({ "photo_ids": [room, cropped] }),
    );
    assert_eq!(done["applied"], json!([room]), "{done}");
    assert_eq!(done["skipped"][0]["photo_id"], cropped, "{done}");
    let lib = Library::open(&root).unwrap();
    let after = lib.read_recipe(room).unwrap().unwrap().recipe;
    assert_eq!(after["crop"]["angle"].as_f64(), Some(angle), "{after}");
    assert_eq!(
        after["global"]["exposure"],
        json!(0.3),
        "the rest of the edit stays: {after}"
    );
    assert_eq!(lib.read_recipe(cropped).unwrap().unwrap().recipe, hand);
    drop(lib);

    // One Undo takes it all back.
    call(
        &tools,
        "undo_batch",
        json!({ "batch_id": done["batch_id"] }),
    );
    let back = Library::open(&root)
        .unwrap()
        .read_recipe(room)
        .unwrap()
        .unwrap()
        .recipe;
    assert!(back.get("crop").is_none(), "{back}");
}
