//! Remove from Library: Photos and Shoots leave with everything that belongs to them, come
//! back exactly on restore, and originals only move (to the Trash) when asked.

use argus_engine::commands::dispatch;
use argus_engine::library::{Author, Library, PhotoFilter, Verdict};
use argus_engine::tools::Tools;
use serde_json::{json, Value};
use std::path::Path;

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..160 * 120 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 160, 120, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn ids(v: &Value) -> Vec<i64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect()
}

/// A Library with two Shoots of three Photos, the first copied into the Library.
fn setup() -> (tempfile::TempDir, Tools, i64, i64) {
    let dir = tempfile::tempdir().unwrap();
    for (shoot, seed) in [("2026-10-12_tahoe", 3u8), ("2026-10-13_lake", 40)] {
        for i in 0..3u8 {
            write_jpeg(
                &dir.path().join(format!("{shoot}/{i}.jpg")),
                seed + i * 2 + 1,
            );
        }
    }
    let tools = Tools::new(dir.path().join("lib"));
    let a = dispatch(
        &tools,
        "import_folder",
        json!({ "folder": dir.path().join("2026-10-12_tahoe"), "copy": true }),
    )
    .unwrap();
    let b = dispatch(
        &tools,
        "import_folder",
        json!({ "folder": dir.path().join("2026-10-13_lake") }),
    )
    .unwrap();
    (
        dir,
        tools,
        a["shoot_id"].as_i64().unwrap(),
        b["shoot_id"].as_i64().unwrap(),
    )
}

#[test]
fn removing_photos_takes_their_rows_and_restore_puts_them_back_exactly() {
    let (dir, tools, shoot, _) = setup();
    let photos = ids(&dispatch(&tools, "photos", json!({ "shoot_id": shoot })).unwrap());
    let gone = photos[0];
    dispatch(&tools, "flag", json!({ "ids": [gone], "verdict": "pick" })).unwrap();
    dispatch(
        &tools,
        "edit",
        json!({ "photo_id": gone, "recipe": { "exposure": 0.4 } }),
    )
    .unwrap();
    let originals = || {
        std::fs::read_dir(dir.path().join("lib/originals"))
            .unwrap()
            .flat_map(|d| std::fs::read_dir(d.unwrap().path()).unwrap())
            .count()
    };
    let before_files = originals();

    let r = dispatch(&tools, "remove_photos", json!({ "photo_ids": [gone] })).unwrap();
    assert_eq!(r["photos"], 1);
    assert_eq!(r["originals_trashed"], 0);
    let left = ids(&dispatch(&tools, "photos", json!({ "shoot_id": shoot })).unwrap());
    assert_eq!(left, photos[1..].to_vec());
    assert_eq!(
        originals(),
        before_files,
        "Remove never touches originals unless asked"
    );
    assert_eq!(
        dispatch(&tools, "list_removed", Value::Null)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let back = dispatch(&tools, "restore_removed", json!({})).unwrap();
    assert_eq!(back["photos"], 1);
    let all = dispatch(&tools, "photos", json!({ "shoot_id": shoot })).unwrap();
    assert_eq!(ids(&all), photos, "same ids, same order");
    let p = all
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == gone)
        .unwrap();
    assert_eq!(p["verdict"], "pick", "its Verdict came back");
    let r = dispatch(&tools, "read_recipe", json!({ "photo_id": gone })).unwrap();
    assert_eq!(r["recipe"]["exposure"], 0.4, "its edit came back");
    assert!(dispatch(&tools, "list_removed", Value::Null)
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn removing_a_shoot_takes_its_photos_and_stacks() {
    let (_dir, tools, shoot, other) = setup();
    let mut lib = Library::open(tools.library()).unwrap();
    let photos: Vec<i64> = lib
        .list_photos(&PhotoFilter {
            shoot: Some(shoot),
            ..Default::default()
        })
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    lib.replace_stacks(shoot, &[photos[..2].to_vec()]).unwrap();
    let batch = lib.begin_batch(Author::Ai, "cull").unwrap();
    lib.set_verdict(
        &photos,
        Verdict::Reject,
        Author::Ai,
        Some("soft"),
        Some(batch),
    )
    .unwrap();
    drop(lib);

    let r = dispatch(&tools, "remove_photos", json!({ "shoot_ids": [shoot] })).unwrap();
    assert_eq!(
        (r["shoots"].as_i64(), r["photos"].as_i64()),
        (Some(1), Some(3))
    );
    let shoots = dispatch(&tools, "list_shoots", Value::Null).unwrap();
    assert_eq!(
        shoots
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![other]
    );

    dispatch(
        &tools,
        "restore_removed",
        json!({ "removal": r["removal"] }),
    )
    .unwrap();
    let lib = Library::open(tools.library()).unwrap();
    let back = lib
        .list_photos(&PhotoFilter {
            shoot: Some(shoot),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(back.len(), 3);
    assert_eq!(
        back.iter().filter(|p| p.stack.is_some()).count(),
        2,
        "the Stack came back"
    );
}

#[test]
fn delete_originals_moves_them_to_the_trash_and_restore_brings_them_back() {
    let (dir, tools, shoot, _) = setup();
    let _env = crate::env_lock();
    let trash = dir.path().join("Trash");
    std::env::set_var("ARGUS_TRASH", &trash);
    let lib = Library::open(tools.library()).unwrap();
    let p = &lib
        .list_photos(&PhotoFilter {
            shoot: Some(shoot),
            ..Default::default()
        })
        .unwrap()[0];
    let original = std::path::PathBuf::from(&p.path);
    drop(lib);
    assert!(original.is_file());

    let r = dispatch(
        &tools,
        "remove_photos",
        json!({ "photo_ids": [p.id], "delete_originals": true }),
    )
    .unwrap();
    assert_eq!(r["originals_trashed"], 1);
    assert!(!original.exists());
    assert_eq!(std::fs::read_dir(&trash).unwrap().count(), 1);

    let back = dispatch(&tools, "restore_removed", Value::Null).unwrap();
    assert_eq!(back["originals_back"], 1);
    assert!(original.is_file());
    std::env::remove_var("ARGUS_TRASH");
}

#[test]
fn a_photo_imported_again_can_be_removed_and_its_old_removal_wont_clash() {
    let (dir, tools, _, lake) = setup();
    let first = ids(&dispatch(&tools, "photos", json!({ "shoot_id": lake })).unwrap());
    dispatch(&tools, "remove_photos", json!({ "shoot_ids": [lake] })).unwrap();
    // Removed Photos are new again to Import, like Lightroom.
    let again = dispatch(
        &tools,
        "import_folder",
        json!({ "folder": dir.path().join("2026-10-13_lake") }),
    )
    .unwrap();
    assert_eq!(again["imported"], 3);
    let err = dispatch(&tools, "restore_removed", Value::Null).unwrap_err();
    assert!(err.0.contains("in the Library again"), "{}", err.0);
    // Emptying it leaves the new Photos' previews alone.
    dispatch(&tools, "empty_removed", Value::Null).unwrap();
    let now = dispatch(&tools, "photos", json!({ "shoot_id": again["shoot_id"] })).unwrap();
    for p in now.as_array().unwrap() {
        let preview = p["preview"].as_str().unwrap();
        assert!(tools.library().join(preview).is_file(), "{preview} kept");
    }
    assert_eq!(first.len(), 3);
}
