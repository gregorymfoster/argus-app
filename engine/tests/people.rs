//! People through the tool layer, on Greg's bench JPEGs (private repo only): faces of one
//! person across a Shoot group together, naming a group names every face in it and keeps
//! `People|<name>` on its Photos, "not this person" and hiding take a face out and keep it
//! out, renaming onto another name merges two people, and face crops come back as files.

use argus_engine::analyzers;
use argus_engine::commands::dispatch;
use argus_engine::library::{Library, PhotoFilter};
use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::path::Path;

/// A party where the same few friends are in most frames.
const PHOTOS: [&str; 7] = [
    "DSC00535.jpg",
    "DSC00536.jpg",
    "DSC00537.jpg",
    "DSC00538.jpg",
    "DSC00539.jpg",
    "DSC00540.jpg",
    "DSC00541.jpg",
];

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match tools.call(name, args).unwrap().remove(0) {
        Part::Json(v) => v,
        Part::Jpeg(_) => panic!("{name} gave an image"),
    }
}

fn library() -> (tempfile::TempDir, Tools) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2025-03-22_party");
    std::fs::create_dir_all(&src).unwrap();
    let bench =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/bench/cull/2025-03-22/photos");
    for p in PHOTOS {
        std::fs::copy(bench.join(p), src.join(p)).unwrap();
    }
    let tools = Tools::new(tmp.path().join("Argus.library"));
    tools
        .call("import_folder", json!({ "folder": src }))
        .unwrap();
    let lib = Library::open(tools.library()).unwrap();
    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    analyzers::facts_for(&lib, &photos, &["identity"]).unwrap();
    (tmp, tools)
}

fn keywords(tools: &Tools, photo: i64) -> Vec<String> {
    let tags = json1(tools, "read_tags", json!({ "photo_ids": [photo] }));
    tags[0]["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["keyword"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn naming_a_group_names_its_faces_and_tags_its_photos() {
    if !crate::has_bench() {
        return;
    }
    let (_tmp, tools) = library();
    let view = json1(&tools, "list_people", json!({}));
    assert_eq!(
        view["people"],
        json!([]),
        "nobody is named until the person names them"
    );
    assert_eq!(view["unread_photos"], 0);
    let groups = view["groups"].as_array().unwrap();
    let biggest = &groups[0];
    let faces = biggest["faces"].as_array().unwrap().clone();
    assert!(faces.len() >= 4, "one friend is in most frames: {biggest}");
    assert_eq!(
        biggest["photos"].as_u64().unwrap() as usize,
        faces.len(),
        "one face per Photo: {biggest}"
    );

    // Name the group: every face is that person, and each Photo keeps the Keyword.
    let ana = json1(
        &tools,
        "name_faces",
        json!({ "faces": faces, "name": "  Ana " }),
    );
    assert_eq!(ana["name"], "Ana");
    let id = ana["id"].clone();
    for f in &faces {
        assert!(
            keywords(&tools, f["photo_id"].as_i64().unwrap()).contains(&"People|Ana".to_string())
        );
    }
    let theirs = dispatch(&tools, "photos", json!({ "person_id": id })).unwrap();
    assert_eq!(
        theirs.as_array().unwrap().len(),
        faces.len(),
        "the grid shows their Photos"
    );
    let view = json1(&tools, "list_people", json!({}));
    assert_eq!(view["people"][0]["name"], "Ana");
    assert_eq!(
        view["people"][0]["photos"].as_u64().unwrap() as usize,
        faces.len()
    );
    assert!(
        view["groups"].as_array().unwrap().iter().all(|g| g["faces"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| !faces.contains(f))),
        "named faces leave the groups"
    );

    // Not Ana: the face is unnamed, its Photo loses the Keyword, and it isn't offered as Ana.
    let wrong = faces[0].clone();
    json1(
        &tools,
        "not_person",
        json!({ "faces": [wrong], "person_id": id }),
    );
    let photo = wrong["photo_id"].as_i64().unwrap();
    assert!(!keywords(&tools, photo).contains(&"People|Ana".to_string()));
    let person = json1(&tools, "get_person", json!({ "person_id": id }));
    assert_eq!(person["faces"].as_array().unwrap().len(), faces.len() - 1);
    assert!(person["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["photo_id"] != wrong["photo_id"] || s["face"] != wrong["face"]));

    // Hidden faces leave the groups; unhide brings them back.
    let stranger = view["groups"][1]["faces"][0].clone();
    json1(&tools, "hide_faces", json!({ "faces": [stranger] }));
    let all_faces = |v: &Value| -> Vec<Value> {
        v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["faces"].as_array().unwrap().clone())
            .collect()
    };
    assert!(!all_faces(&json1(&tools, "list_people", json!({}))).contains(&stranger));
    json1(
        &tools,
        "hide_faces",
        json!({ "faces": [stranger], "unhide": true }),
    );
    assert!(all_faces(&json1(&tools, "list_people", json!({}))).contains(&stranger));

    // Rename moves the Keyword; renaming someone else to the same name merges them in.
    json1(
        &tools,
        "rename_person",
        json!({ "person_id": id, "name": "Ana Lee" }),
    );
    let kept = faces[1]["photo_id"].as_i64().unwrap();
    let ks = keywords(&tools, kept);
    assert!(
        ks.contains(&"People|Ana Lee".to_string()) && !ks.contains(&"People|Ana".to_string()),
        "{ks:?}"
    );
    let other = json1(
        &tools,
        "name_faces",
        json!({ "faces": [stranger], "name": "Bo" }),
    );
    let merged = json1(
        &tools,
        "rename_person",
        json!({ "person_id": other["id"], "name": "ana lee" }),
    );
    assert_eq!(merged["id"], id, "the existing person wins");
    let view = json1(&tools, "list_people", json!({}));
    assert_eq!(
        view["people"].as_array().unwrap().len(),
        1,
        "Bo merged into Ana Lee: {}",
        view["people"]
    );
    let photo = stranger["photo_id"].as_i64().unwrap();
    let ks = keywords(&tools, photo);
    assert!(
        ks.contains(&"People|Ana Lee".to_string()) && !ks.contains(&"People|Bo".to_string()),
        "{ks:?}"
    );
}

#[test]
fn face_crops_are_square_jpegs_in_the_library() {
    if !crate::has_bench() {
        return;
    }
    let (_tmp, tools) = library();
    let view = json1(&tools, "list_people", json!({}));
    let cover = view["groups"][0]["cover"].clone();
    let out = dispatch(
        &tools,
        "face_crops",
        json!({ "faces": [cover, { "photo_id": 999, "face": 0 }], "edge": 96 }),
    )
    .unwrap();
    let key = format!("{}-{}", cover["photo_id"], cover["face"]);
    let rel = out[&key].as_str().unwrap();
    let img = image::open(tools.library().join(rel)).unwrap();
    assert_eq!((img.width(), img.height()), (96, 96));
    assert_eq!(out["999-0"], Value::Null, "an unknown Photo has no crop");
    // The second ask is the cached file.
    let again = dispatch(
        &tools,
        "face_crops",
        json!({ "faces": [cover], "edge": 96 }),
    )
    .unwrap();
    assert_eq!(again[&key], out[&key]);
}
