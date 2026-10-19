//! Choosing, switching and moving Libraries as the app does it, through
//! `Libraries::call` (the same seam the desktop app and `argus serve` use).

use argus_engine::libraries::Libraries;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..320 * 240 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 320, 240, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn call(l: &Libraries, cmd: &str, args: Value) -> Value {
    l.call(cmd, args).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

#[test]
fn first_launch_has_no_library_until_one_is_created_where_the_person_picks() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config/libraries.json");
    let suggested = dir.path().join("Pictures/Argus/library");
    let libs = Libraries::load(Some(config.clone()), None, suggested.clone());

    let info = call(&libs, "library", json!({}));
    assert_eq!(info["current"], Value::Null);
    assert_eq!(info["suggested"], json!(suggested));
    let e = libs.call("list_shoots", json!({})).unwrap_err();
    assert!(e.0.contains("no Library is open"), "{e}");

    // A folder with other things in it gets an Argus-Library folder inside.
    let pictures = dir.path().join("External/Photos");
    std::fs::create_dir_all(&pictures).unwrap();
    std::fs::write(pictures.join("notes.txt"), "mine").unwrap();
    let info = call(&libs, "create_library", json!({ "path": pictures }));
    let made = canon(&pictures.join("Argus-Library"));
    assert_eq!(info["current"], json!(made));
    assert_eq!(info["exists"], true);
    assert_eq!(call(&libs, "list_shoots", json!({})), json!([]));

    // Remembered across launches.
    let again = Libraries::load(Some(config), None, suggested);
    assert_eq!(again.current(), Some(made));
}

#[test]
fn an_existing_default_library_is_adopted_and_argus_library_pins_one() {
    let dir = tempfile::tempdir().unwrap();
    let suggested = dir.path().join("Pictures/Argus/library");
    argus_engine::library::Library::open(&suggested).unwrap();
    let libs = Libraries::load(Some(dir.path().join("c.json")), None, suggested.clone());
    assert_eq!(libs.current(), Some(canon(&suggested)));

    let pinned = dir.path().join("tmp-lib");
    let libs = Libraries::load(
        Some(dir.path().join("c.json")),
        Some(pinned.clone()),
        suggested,
    );
    assert_eq!(libs.current(), Some(canon(&pinned)));
}

#[test]
fn switch_between_libraries_and_move_the_open_one() {
    let dir = tempfile::tempdir().unwrap();
    let libs = Libraries::load(
        Some(dir.path().join("c.json")),
        None,
        dir.path().join("none"),
    );
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    call(&libs, "create_library", json!({ "path": a }));
    let src = dir.path().join("2026-10-12_tahoe");
    write_jpeg(&src.join("one.jpg"), 3);
    call(
        &libs,
        "import_folder",
        json!({ "folder": src, "copy": true }),
    );
    call(&libs, "create_library", json!({ "path": b }));
    assert_eq!(call(&libs, "list_shoots", json!({})), json!([]));

    let e = libs
        .call("open_library", json!({ "path": src }))
        .unwrap_err();
    assert!(e.0.contains("isn't an Argus Library"), "{e}");
    let info = call(&libs, "open_library", json!({ "path": a }));
    assert_eq!(info["recent"][0]["path"], json!(canon(&a)));
    assert_eq!(info["recent"][1]["path"], json!(canon(&b)));
    assert_eq!(
        call(&libs, "list_shoots", json!({}))
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Into a non-empty folder: the Library keeps its folder name inside it.
    let drive = dir.path().join("drive");
    std::fs::create_dir_all(&drive).unwrap();
    std::fs::write(drive.join("other"), "x").unwrap();
    let r = call(&libs, "move_library", json!({ "to": drive }));
    let moved = canon(&drive.join("a"));
    assert_eq!(r["library"]["current"], json!(moved));
    assert!(!a.exists());
    let photos = call(&libs, "photos", json!({}));
    let p = Path::new(photos[0]["path"].as_str().unwrap());
    assert!(p.starts_with(&moved) && p.is_file(), "{p:?}");
    let recent: Vec<_> = r["library"]["recent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].clone())
        .collect();
    assert_eq!(recent, [json!(moved), json!(canon(&b))]);

    // A Library on a disk that's gone: a clear error, never an empty Library made in its place.
    std::fs::rename(&moved, dir.path().join("unplugged")).unwrap();
    let e = libs.call("list_shoots", json!({})).unwrap_err();
    assert!(e.0.contains("can't be found"), "{e}");
    assert!(!moved.exists());
    let info = call(&libs, "forget_library", json!({ "path": moved }));
    assert_eq!(info["recent"].as_array().unwrap().len(), 1);
}
