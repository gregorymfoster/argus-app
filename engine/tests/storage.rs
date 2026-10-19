//! Disk space and portability through the tool layer: card imports copy originals into
//! the Library, the whole Library folder can move and still open, storage is reported by
//! part, the cache clears, preview settings apply, and missing originals relink (ADR 0011).

use argus_engine::commands::dispatch;
use argus_engine::library::{move_library, move_library_via, Library, PhotoFilter};
use argus_engine::tools::Tools;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn write_jpeg(path: &Path, w: u16, h: u16, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..w as usize * h as usize * 3)
        .map(|i| ((i / 7) as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, w, h, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn call(tools: &Tools, cmd: &str, args: Value) -> Value {
    dispatch(tools, cmd, args).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn folder_of_three(dir: &Path) -> PathBuf {
    for (i, name) in ["a.jpg", "b.jpg", "sub/c.jpg"].iter().enumerate() {
        write_jpeg(&dir.join(name), 1200, 800, 3 + i as u8 * 2);
    }
    dir.to_path_buf()
}

fn photos(lib: &Path) -> Vec<argus_engine::library::Photo> {
    Library::open(lib)
        .unwrap()
        .list_photos(&PhotoFilter::default())
        .unwrap()
}

#[test]
fn card_import_copies_originals_into_the_library_and_estimates_first() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("Volumes");
    let card = volumes.join("SONY_A7");
    folder_of_three(&card.join("DCIM/100MSDCF"));
    let _env = crate::env_lock();
    std::env::set_var("ARGUS_VOLUMES", &volumes);
    let tools = Tools::new(dir.path().join("lib"));

    let est = call(&tools, "estimate_import", json!({ "folder": card }));
    assert_eq!(est["photos"], 3);
    assert_eq!(est["copy"], true, "a card copies by default: {est}");
    let originals = est["original_bytes"].as_u64().unwrap();
    assert!(originals > 0);
    assert_eq!(est["copy_bytes"].as_u64().unwrap(), originals);
    assert!(est["preview_bytes"].as_u64().unwrap() > 0);
    assert!(est["free_bytes"].as_u64().unwrap() > 0);

    let r = call(
        &tools,
        "import_folder",
        json!({ "folder": card, "shoot_name": "2026-10-12 Boat Trip" }),
    );
    assert_eq!(r["imported"], 3, "{r}");
    let copied_to = r["copied_to"].as_str().unwrap();
    assert!(
        copied_to.starts_with("originals/") && copied_to.ends_with("_Boat-Trip"),
        "{copied_to}"
    );
    assert!(!copied_to.contains(' '));
    let shoots = call(&tools, "list_shoots", json!({}));
    assert_eq!(
        copied_to,
        format!("originals/{}", shoots[0]["name"].as_str().unwrap()),
        "the folder is the Shoot's name"
    );

    let lib = dir.path().join("lib").canonicalize().unwrap();
    for p in photos(&lib) {
        assert!(p.in_library, "{}", p.path);
        assert!(Path::new(&p.path).starts_with(lib.join(copied_to)));
        assert!(Path::new(&p.path).is_file());
    }
    // The card is untouched and still holds its files.
    assert_eq!(
        std::fs::read_dir(card.join("DCIM/100MSDCF"))
            .unwrap()
            .count(),
        3
    );

    let s = call(&tools, "library_storage", json!({}));
    assert_eq!(s["originals_in_library"]["files"], 3);
    assert_eq!(
        s["originals_in_library"]["bytes"].as_u64().unwrap(),
        originals
    );
    assert_eq!(s["originals_elsewhere"]["files"], 0);
    assert_eq!(s["missing"], 0);

    // A folder that isn't on a card imports in place unless asked to copy.
    let folder = folder_of_three(&dir.path().join("Desktop/phone"));
    let est = call(&tools, "estimate_import", json!({ "folder": folder }));
    assert_eq!(
        (est["copy"].clone(), est["copy_bytes"].clone()),
        (json!(false), json!(0))
    );
}

#[test]
fn a_library_with_copied_originals_moves_to_another_disk_and_still_opens() {
    let dir = tempfile::tempdir().unwrap();
    let src = folder_of_three(&dir.path().join("shoot"));
    let lib = dir.path().join("lib");
    let tools = Tools::new(&lib);
    call(
        &tools,
        "import_folder",
        json!({ "folder": src, "copy": true }),
    );
    let before = photos(&lib);
    std::fs::remove_dir_all(&src).unwrap(); // the card is formatted

    // Across disks: copy, verify, remove the old folder.
    let moved = dir.path().join("external/Argus-Library");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    let r = move_library_via(&lib, &moved, false).unwrap();
    assert!(r.copied);
    assert!(
        !lib.exists(),
        "the old folder is removed after a verified copy"
    );
    let after = photos(&moved);
    assert_eq!(after.len(), before.len());
    for p in &after {
        assert!(Path::new(&p.path).is_file(), "{}", p.path);
        assert!(p.preview.is_file());
    }
    let s = call(&Tools::new(&moved), "library_storage", json!({}));
    assert_eq!(s["missing"], 0);

    // Same disk: a rename. Refuses a non-empty target or a Library inside itself.
    let busy = dir.path().join("busy");
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::write(busy.join("x"), "x").unwrap();
    assert!(move_library(&moved, &busy).is_err());
    assert!(move_library(&moved, &moved.join("inner")).is_err());
    let back = dir.path().join("lib-again");
    assert!(!move_library(&moved, &back).unwrap().copied);
    assert_eq!(photos(&back).len(), 3);
}

#[test]
fn storage_reports_parts_and_clear_cache_frees_only_rebuildable_files() {
    let dir = tempfile::tempdir().unwrap();
    let src = folder_of_three(&dir.path().join("shoot"));
    let lib = dir.path().join("lib");
    let tools = Tools::new(&lib);
    call(&tools, "import_folder", json!({ "folder": src }));

    let s = call(&tools, "library_storage", json!({}));
    assert_eq!(s["photos"], 3);
    assert_eq!(s["originals_elsewhere"]["files"], 3, "read in place: {s}");
    assert_eq!(s["originals_in_library"]["files"], 0);
    assert_eq!(s["previews"]["files"], 3);
    assert!(s["database"]["bytes"].as_u64().unwrap() > 0);
    assert_eq!(s["cache"]["bytes"], 0);

    // A stray preview and a cache file are cache; clearing them leaves the rest alone.
    std::fs::write(
        lib.join("previews/zz/orphan.jpg").tap_parent(),
        vec![0u8; 1000],
    )
    .unwrap();
    std::fs::create_dir_all(lib.join("cache")).unwrap();
    std::fs::write(lib.join("cache/render.bin"), vec![0u8; 500]).unwrap();
    let s = call(&tools, "library_storage", json!({}));
    assert_eq!(s["cache"]["bytes"], 1500);
    assert_eq!(s["previews"]["files"], 3);
    let freed = call(&tools, "clear_cache", json!({}));
    assert_eq!(freed["bytes"], 1500);
    let s = call(&tools, "library_storage", json!({}));
    assert_eq!(
        (s["cache"]["bytes"].clone(), s["previews"]["files"].clone()),
        (json!(0), json!(3))
    );
    assert!(photos(&lib).iter().all(|p| p.preview.is_file()));
}

trait TapParent {
    fn tap_parent(self) -> Self;
}
impl TapParent for PathBuf {
    fn tap_parent(self) -> Self {
        std::fs::create_dir_all(self.parent().unwrap()).unwrap();
        self
    }
}

#[test]
fn smaller_previews_after_a_settings_change_and_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let src = folder_of_three(&dir.path().join("shoot"));
    let tools = Tools::new(dir.path().join("lib"));
    call(&tools, "import_folder", json!({ "folder": src }));
    let big = call(&tools, "library_storage", json!({}))["previews"]["bytes"]
        .as_u64()
        .unwrap();

    assert!(dispatch(
        &tools,
        "library_settings",
        json!({ "preview_long_edge": 100 })
    )
    .is_err());
    let s = call(
        &tools,
        "library_settings",
        json!({ "preview_long_edge": 512 }),
    );
    assert_eq!(
        (s["preview_long_edge"].clone(), s["preview_quality"].clone()),
        (json!(512), json!(85))
    );
    assert_eq!(
        call(&tools, "library_settings", json!({}))["preview_long_edge"],
        512
    );

    let r = call(&tools, "rebuild_previews", json!({}));
    assert_eq!(
        (r["rebuilt"].clone(), r["offline"].clone()),
        (json!(3), json!(0)),
        "{r}"
    );
    let small = r["previews"]["bytes"].as_u64().unwrap();
    assert!(small < big, "{small} < {big}");
    let p = &photos(&dir.path().join("lib"))[0];
    let (w, h) = image_size(&p.preview);
    assert_eq!(w.max(h), 512);
}

fn image_size(p: &Path) -> (u32, u32) {
    let data = std::fs::read(p).unwrap();
    // SOF0 marker: FF C0, then length(2) precision(1) height(2) width(2).
    let i = data.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
    let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
    (w, h)
}

#[test]
fn moved_originals_are_found_and_relinked_by_content() {
    let dir = tempfile::tempdir().unwrap();
    let src = folder_of_three(&dir.path().join("shoot"));
    let tools = Tools::new(dir.path().join("lib"));
    call(&tools, "import_folder", json!({ "folder": src }));

    let new_home = dir.path().join("drive/2026-10-12_boat-trip");
    std::fs::create_dir_all(new_home.parent().unwrap()).unwrap();
    std::fs::rename(&src, &new_home).unwrap();
    // A decoy with the right name but other content must not be linked.
    write_jpeg(&dir.path().join("decoy/a.jpg"), 64, 64, 99);

    let missing = call(&tools, "find_missing_originals", json!({}));
    assert_eq!(missing.as_array().unwrap().len(), 3);
    let r = call(
        &tools,
        "relink_originals",
        json!({ "folder": dir.path().join("decoy") }),
    );
    assert_eq!(
        (r["relinked"].clone(), r["still_missing"].clone()),
        (json!(0), json!(3))
    );
    let r = call(
        &tools,
        "relink_originals",
        json!({ "folder": dir.path().join("drive") }),
    );
    assert_eq!(
        (r["relinked"].clone(), r["still_missing"].clone()),
        (json!(3), json!(0))
    );
    assert_eq!(call(&tools, "library_storage", json!({}))["missing"], 0);
    let new_home = new_home.canonicalize().unwrap();
    assert!(photos(&dir.path().join("lib"))
        .iter()
        .all(|p| Path::new(&p.path).starts_with(&new_home)));
}

#[test]
fn opening_an_older_library_stores_in_library_paths_relative() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    let src = folder_of_three(&lib.join("originals/old"));
    call(&Tools::new(&lib), "import_folder", json!({ "folder": src }));
    // Make it look like a v3 Library that stored absolute paths.
    let abs = lib.canonicalize().unwrap();
    {
        let db = rusqlite::Connection::open(lib.join("library.db")).unwrap();
        db.execute(
            "UPDATE photos SET path = ?1 || '/' || path",
            [abs.to_string_lossy()],
        )
        .unwrap();
        db.execute_batch(
            "DROP TABLE settings; DROP TABLE ratings; DROP TABLE rating_log; DROP TABLE best_photos;
             DROP TABLE suggestions; DROP TABLE jobs; DROP TABLE facts; DROP TABLE usage_events;
             ALTER TABLE photos DROP COLUMN lens; ALTER TABLE photos DROP COLUMN focal; ALTER TABLE photos DROP COLUMN iso; ALTER TABLE photos DROP COLUMN aperture; ALTER TABLE photos DROP COLUMN shutter;
             PRAGMA user_version = 3;",
        )
            .unwrap();
    }
    let ps = photos(&lib);
    assert!(ps
        .iter()
        .all(|p| p.in_library && Path::new(&p.path).is_file()));
    let db = rusqlite::Connection::open(lib.join("library.db")).unwrap();
    let stored: String = db
        .query_row("SELECT path FROM photos LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert!(stored.starts_with("originals/old/"), "{stored}");
}

#[test]
fn a_raw_plus_jpeg_estimate_counts_the_jpegs_unless_raw_only() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("card");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("DSC0001.ARW"), vec![0u8; 50_000]).unwrap();
    std::fs::write(folder.join("DSC0001.JPG"), vec![0u8; 20_000]).unwrap();
    let tools = Tools::new(dir.path().join("lib"));
    let est = |raw_only: bool| {
        call(
            &tools,
            "estimate_import",
            json!({ "folder": folder, "copy": true, "raw_only": raw_only,
                    "files": [folder.join("DSC0001.ARW")] }),
        )
    };
    let (pair, raw) = (est(false), est(true));
    assert_eq!(
        (pair["photos"].clone(), raw["photos"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(pair["original_bytes"], json!(70_000), "{pair}");
    assert_eq!(raw["original_bytes"], json!(50_000), "{raw}");
    assert!(pair["needed_bytes"].as_u64() > raw["needed_bytes"].as_u64());
}
