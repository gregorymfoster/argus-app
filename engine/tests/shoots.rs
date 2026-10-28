//! Renaming a Shoot through the tool layer: the name follows the naming rules, a copied
//! Shoot's originals folder moves with it, and every Photo still finds its original.

use argus_engine::library::{ImportOptions, Library, PhotoFilter};
use argus_engine::tools::Tools;
use serde_json::{json, Value};
use std::path::Path;

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..64 * 48 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 64, 48, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn import(lib: &mut Library, dir: &Path, name: &str, seeds: &[u8], copy: bool) -> i64 {
    let src = dir.join(format!("card-{name}"));
    for s in seeds {
        write_jpeg(&src.join(format!("DSC{s:05}.JPG")), *s);
    }
    let report = lib
        .import(
            &src,
            &ImportOptions {
                shoot_name: Some(name.into()),
                copy,
                ..Default::default()
            },
        )
        .unwrap();
    report.shoot_id.unwrap()
}

fn rename(tools: &Tools, shoot: i64, name: &str) -> Value {
    let parts = tools
        .call("rename_shoot", json!({ "shoot_id": shoot, "name": name }))
        .unwrap();
    match &parts[0] {
        argus_engine::tools::Part::Json(v) => v.clone(),
        _ => panic!("rename_shoot answers JSON"),
    }
}

fn paths(lib: &Library, shoot: i64) -> Vec<String> {
    lib.list_photos(&PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })
    .unwrap()
    .into_iter()
    .map(|p| p.path)
    .collect()
}

#[test]
fn renaming_a_copied_shoot_moves_its_folder_and_keeps_its_date() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let mut lib = Library::open(&root).unwrap();
    let shoot = import(&mut lib, dir.path(), "2026-10-10", &[3, 5], true);
    let before = paths(&lib, shoot);
    assert!(
        before.iter().all(|p| p.contains("/originals/2026-10-10/")),
        "{before:?}"
    );
    drop(lib);

    let tools = Tools::new(&root);
    let r = rename(&tools, shoot, "Boat tour");
    assert_eq!(r["name"], "2026-10-10_Boat-tour");
    assert_eq!(r["originals"], "originals/2026-10-10_Boat-tour");
    assert!(!root.join("originals/2026-10-10").exists());

    let lib = Library::open(&root).unwrap();
    let after = paths(&lib, shoot);
    assert_eq!(after.len(), 2);
    for p in &after {
        assert!(p.contains("/originals/2026-10-10_Boat-tour/"), "{p}");
        assert!(Path::new(p).is_file(), "{p} is missing");
    }
    assert_eq!(lib.shoots().unwrap()[0].name, "2026-10-10_Boat-tour");
    assert!(lib.missing_originals().unwrap().is_empty());

    // A name typed with its own date moves the Shoot to that date; a case-only change
    // renames in place rather than to `-2`.
    let r = rename(&tools, shoot, "2026-10-11 Boat Tour");
    assert_eq!(r["name"], "2026-10-11_Boat-Tour");
    let r = rename(&tools, shoot, "2026-10-11_boat-tour");
    assert_eq!(r["originals"], "originals/2026-10-11_boat-tour");
}

#[test]
fn rename_avoids_a_taken_folder_and_leaves_in_place_originals_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let mut lib = Library::open(&root).unwrap();
    let a = import(&mut lib, dir.path(), "2026-10-10_Lake", &[3], true);
    let b = import(&mut lib, dir.path(), "2026-10-10_Hike", &[5], true);
    let c = import(&mut lib, dir.path(), "2026-10-10_Town", &[7], false);
    let in_place = paths(&lib, c);
    drop(lib);

    let tools = Tools::new(&root);
    let r = rename(&tools, b, "Lake");
    assert_eq!(r["name"], "2026-10-10_Lake");
    assert_eq!(r["originals"], "originals/2026-10-10_Lake-2");

    let r = rename(&tools, c, "Old town");
    assert_eq!(r["originals"], Value::Null);
    let lib = Library::open(&root).unwrap();
    assert_eq!(paths(&lib, c), in_place);
    assert!(paths(&lib, a).iter().all(|p| Path::new(p).is_file()));
    assert!(paths(&lib, b)
        .iter()
        .all(|p| p.contains("_Lake-2/") && Path::new(p).is_file()));

    let err = tools
        .call("rename_shoot", json!({ "shoot_id": a, "name": "  " }))
        .unwrap_err();
    assert!(err.0.contains("needs a name"), "{}", err.0);
}

#[test]
fn reveal_and_suggest_name_work_on_a_shoot_through_commands() {
    use argus_engine::commands::dispatch;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let mut lib = Library::open(&root).unwrap();
    let shoot = import(&mut lib, dir.path(), "2026-10-10", &[3, 5], true);
    let photo = lib
        .list_photos(&PhotoFilter {
            shoot: Some(shoot),
            verdict: None,
        })
        .unwrap()[0]
        .clone();
    drop(lib);
    let tools = Tools::new(&root);

    let _env = crate::env_lock();
    std::env::set_var("ARGUS_NO_REVEAL", "1");
    let r = dispatch(&tools, "reveal", json!({ "shoot_id": shoot })).unwrap();
    assert!(
        r["path"]
            .as_str()
            .unwrap()
            .ends_with("originals/2026-10-10"),
        "{r}"
    );
    assert_eq!(r["shown"], false);
    let r = dispatch(&tools, "reveal", json!({ "photo_id": photo.id })).unwrap();
    assert_eq!(r["path"], photo.path.as_str());
    std::env::remove_var("ARGUS_NO_REVEAL");

    // Not the Ollama provider, so it falls back to the Shoot's own date without a model.
    let r = dispatch(
        &tools,
        "suggest_name",
        json!({ "shoot_id": shoot, "provider": "claude-code" }),
    )
    .unwrap();
    assert_eq!(r["by"], "date");
    assert!(r["name"].as_str().unwrap().starts_with("20"), "{r}");
}

fn call(tools: &Tools, name: &str, args: Value) -> Value {
    match &tools.call(name, args).unwrap()[0] {
        argus_engine::tools::Part::Json(v) => v.clone(),
        _ => panic!("{name} answers JSON"),
    }
}

fn ids(lib: &Library, shoot: i64) -> Vec<i64> {
    lib.list_photos(&PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })
    .unwrap()
    .into_iter()
    .map(|p| p.id)
    .collect()
}

#[test]
fn moving_photos_takes_their_originals_along_and_removes_an_emptied_shoot() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let mut lib = Library::open(&root).unwrap();
    let walk = import(&mut lib, dir.path(), "2026-10-25_Walk", &[3, 5, 7], true);
    let beach = import(&mut lib, dir.path(), "2026-10-25_Beach", &[9], true);
    let stray = ids(&lib, beach);
    drop(lib);
    let tools = Tools::new(&root);

    // The Beach's one frame belongs with the Walk.
    let r = call(
        &tools,
        "move_photos",
        json!({ "photo_ids": stray, "shoot_id": walk }),
    );
    assert_eq!(r["moved"], 1);
    assert_eq!(r["removed_shoots"], json!([beach]));
    let lib = Library::open(&root).unwrap();
    assert_eq!(ids(&lib, walk).len(), 4);
    assert_eq!(lib.shoots().unwrap().len(), 1);
    for p in paths(&lib, walk) {
        assert!(p.contains("/originals/2026-10-25_Walk/"), "{p}");
        assert!(Path::new(&p).is_file(), "{p} is missing");
    }
    assert!(!root.join("originals/2026-10-25_Beach").exists());
    assert!(lib.missing_originals().unwrap().is_empty());
}

#[test]
fn splitting_a_shoot_moves_the_rest_into_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let mut lib = Library::open(&root).unwrap();
    let day = import(&mut lib, dir.path(), "2026-10-25_Day", &[3, 5, 7, 9], true);
    let later = ids(&lib, day)[2..].to_vec();
    drop(lib);
    let tools = Tools::new(&root);

    let r = call(
        &tools,
        "move_photos",
        json!({ "photo_ids": later, "new_shoot_name": "Dinner" }),
    );
    let new = r["shoot_id"].as_i64().unwrap();
    assert!(r["name"].as_str().unwrap().ends_with("_Dinner"), "{r}");
    assert_eq!(r["removed_shoots"], json!([]));
    let lib = Library::open(&root).unwrap();
    assert_eq!(ids(&lib, day).len(), 2);
    assert_eq!(ids(&lib, new), later);
    for p in paths(&lib, new) {
        assert!(p.contains("_Dinner/"), "{p}");
        assert!(Path::new(&p).is_file(), "{p} is missing");
    }
    assert!(lib.missing_originals().unwrap().is_empty());

    let err = tools
        .call(
            "move_photos",
            json!({ "photo_ids": later, "shoot_id": day, "new_shoot_name": "x" }),
        )
        .unwrap_err();
    assert!(err.0.contains("not both"), "{}", err.0);
}

#[test]
fn scanning_a_card_proposes_a_shoot_per_outing() {
    use std::time::{Duration, SystemTime};
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("card/DCIM");
    // No EXIF, so capture time is the file's time: a morning, then three hours later.
    let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_000_000);
    for (i, mins) in [0u64, 10, 20, 200, 210, 220].iter().enumerate() {
        let p = card.join(format!("DSC{:05}.JPG", i + 1));
        write_jpeg(&p, i as u8 + 3);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(t0 + Duration::from_secs(mins * 60))
            .unwrap();
    }
    let root = dir.path().join("Library");
    Library::open(&root).unwrap();
    let tools = Tools::new(&root);
    let scan = call(&tools, "scan_folder", json!({ "folder": card }));
    let shoots = scan["shoots"].as_array().unwrap();
    assert_eq!(shoots.len(), 2, "{scan}");
    assert_eq!(shoots[0]["photos"], json!([0, 1, 2]));
    assert_eq!(shoots[1]["photos"], json!([3, 4, 5]));
    assert_eq!(shoots[1]["gap_before_minutes"], 180.0);
    assert_ne!(shoots[0]["name"], shoots[1]["name"]);
}
