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
