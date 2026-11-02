//! Backup places (ADR 0017): a copy of the Library in another folder that is incremental,
//! verified, never deletes, opens as a Library, and runs through the Queue and the tool layer.

use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::library::{BackupWhen, Copies, ImportOptions, Library, PhotoFilter, Place};
use argus_engine::tools::{Part, Tools};
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

fn place(folder: &Path, copies: Copies) -> Place {
    Place {
        id: String::new(),
        name: "Test disk".into(),
        folder: folder.to_path_buf(),
        copies,
        include_elsewhere: false,
        when: BackupWhen::Manual,
        keep_snapshots: 3,
        verify_monthly: true,
        copy_name: String::new(),
    }
}

fn json_of(parts: Vec<Part>) -> Value {
    match &parts[0] {
        Part::Json(v) => v.clone(),
        _ => panic!("expected JSON"),
    }
}

/// A Library with one Shoot copied in (2 Photos) and one read in place (1 Photo).
fn library(dir: &Path) -> std::path::PathBuf {
    let card = dir.join("card");
    write_jpeg(&card.join("a.jpg"), 3);
    write_jpeg(&card.join("b.jpg"), 5);
    let loose = dir.join("loose");
    write_jpeg(&loose.join("c.jpg"), 7);
    let root = dir.join("library");
    let mut lib = Library::open(&root).unwrap();
    lib.import(
        &card,
        &ImportOptions {
            copy: true,
            ..Default::default()
        },
    )
    .unwrap();
    lib.import(&loose, &ImportOptions::default()).unwrap();
    root
}

#[test]
fn backs_up_incrementally_verifies_and_opens_as_a_library() {
    let tmp = tempfile::tempdir().unwrap();
    let root = library(tmp.path());
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(&disk).unwrap();
    let lib = Library::open(&root).unwrap();
    let p = lib
        .add_backup_place(place(&disk, Copies::Originals))
        .unwrap();
    assert_eq!(p.id, "test-disk");
    let copy = p.copy_dir();
    assert_eq!(copy, disk.join("Argus-Backup").join("library"));

    let first = lib
        .run_backup(&p.id, None, false, "First", None, &|_, _| {})
        .unwrap();
    assert!(first.done && first.failed.is_empty());
    assert_eq!(
        first.copied_files, 2,
        "the 2 originals copied in; the loose one is left out"
    );
    assert!(copy.join("library.db").is_file());
    assert_eq!(std::fs::read_dir(copy.join("backups")).unwrap().count(), 1);

    let status = &lib.backup_status(&[]).unwrap()[0];
    assert_eq!((status.photos_in, status.waiting), (2, 0));
    assert_eq!(status.status, "up to date");

    // Nothing new: nothing copied. A new original: just that.
    let again = lib
        .run_backup(&p.id, None, false, "Again", None, &|_, _| {})
        .unwrap();
    assert_eq!((again.copied_files, again.skipped), (0, 2));

    // The copy opens as a Library with the same Photos, its originals inside it.
    let opened = Library::open(&copy).unwrap();
    let photos = opened.list_photos(&PhotoFilter::default()).unwrap();
    assert_eq!(photos.len(), 3);
    assert_eq!(
        opened.missing_originals().unwrap().len(),
        0,
        "the loose original still resolves on this Mac"
    );

    // Check: all match; a damaged file is found.
    let ok = lib
        .run_backup(&p.id, None, true, "Check", None, &|_, _| {})
        .unwrap();
    assert!(ok.differ.is_empty());
    let orig = std::fs::read_dir(copy.join("originals"))
        .unwrap()
        .flatten()
        .flat_map(|d| std::fs::read_dir(d.path()).unwrap().flatten())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "jpg"))
        .unwrap();
    let mut bytes = std::fs::read(&orig).unwrap();
    bytes[100] ^= 0xff;
    std::fs::write(&orig, bytes).unwrap();
    let bad = lib
        .run_backup(&p.id, None, true, "Check", None, &|_, _| {})
        .unwrap();
    assert_eq!(bad.differ.len(), 1);
    assert_eq!(lib.backup_status(&[]).unwrap()[0].status, "failed");

    // Removing the place leaves the copy where it is.
    lib.remove_backup_place(&p.id).unwrap();
    assert!(copy.join("library.db").is_file() && orig.is_file());
}

#[test]
fn never_overwrites_a_different_file_and_never_shares_a_copy_between_libraries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = library(tmp.path());
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(&disk).unwrap();
    let lib = Library::open(&root).unwrap();
    let mut wide = place(&disk, Copies::Everything);
    wide.include_elsewhere = true;
    let p = lib.add_backup_place(wide).unwrap();
    // A stranger's file where an original would go.
    let photo = &lib.list_photos(&PhotoFilter::default()).unwrap()[0];
    let rel = Path::new(&photo.path)
        .strip_prefix(&root)
        // macOS temp dirs are /var/... while the Library may store /private/var/...: strip either,
        // never fall back to the absolute path (joined, it would point at the original itself).
        .or_else(|_| Path::new(&photo.path).strip_prefix(root.canonicalize().unwrap()))
        .map(|r| r.to_path_buf())
        .expect("the photo lives in the Library");
    let squat = p.copy_dir().join(&rel);
    std::fs::create_dir_all(squat.parent().unwrap()).unwrap();
    std::fs::write(&squat, b"not a photo").unwrap();
    let r = lib
        .run_backup(&p.id, None, false, "First", None, &|_, _| {})
        .unwrap();
    assert_eq!(r.failed.len(), 1, "{r:?}");
    assert_eq!(std::fs::read(&squat).unwrap(), b"not a photo");
    assert!(
        p.copy_dir().join("previews").is_dir(),
        "Everything copies previews"
    );
    assert!(
        p.copy_dir().join("elsewhere").is_dir(),
        "and the loose original"
    );

    // Another Library with the same folder name gets its own copy folder.
    let other = tmp.path().join("b");
    let root2 = library(&other);
    let lib2 = Library::open(&root2).unwrap();
    let p2 = lib2.add_backup_place(place(&disk, Copies::Edits)).unwrap();
    assert_eq!(p2.copy_dir(), disk.join("Argus-Backup").join("library-2"));
    // And no place inside the Library itself.
    assert!(lib
        .add_backup_place(place(&root.join("cache"), Copies::Edits))
        .is_err());
}

#[test]
fn tools_queue_a_backup_and_import_backs_up_the_new_shoot() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("library");
    let tools = Tools::new(root.clone());
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(&disk).unwrap();
    let added = json_of(
        tools
            .call(
                "add_backup_place",
                json!({ "folder": disk, "name": "T7", "when": "after_import" }),
            )
            .unwrap(),
    );
    assert_eq!(added["place"]["id"], "t7");
    let card = tmp.path().join("card");
    write_jpeg(&card.join("a.jpg"), 3);
    tools
        .call("import_folder", json!({ "folder": card, "copy": true }))
        .unwrap();
    let lib = Library::open(&root).unwrap();
    let queued = lib.jobs(None, Some("backup"), 10).unwrap();
    assert_eq!(queued.len(), 1);
    assert!(
        queued[0].label.starts_with("Backing up to T7"),
        "{}",
        queued[0].label
    );
    jobs::drain(&tools, Compute::Full).unwrap();
    let st = json_of(tools.call("backup_places", json!({})).unwrap());
    assert_eq!(st["places"][0]["photos_in"], 1, "{st}");
    assert_eq!(st["one_copy"], 1);
    assert_eq!(st["places"][0]["history"][0]["check"], "verified");

    // run_backup with a Shoot dedupes while queued; verify is its own job.
    let a = json_of(tools.call("run_backup", json!({})).unwrap());
    let b = json_of(tools.call("run_backup", json!({})).unwrap());
    assert_eq!(a[0]["job_id"], b[0]["job_id"]);
    let v = json_of(
        tools
            .call("run_backup", json!({ "id": "t7", "verify": true }))
            .unwrap(),
    );
    assert_ne!(a[0]["job_id"], v[0]["job_id"]);
    jobs::drain(&tools, Compute::Full).unwrap();
    let est = json_of(
        tools
            .call("estimate_backup", json!({ "folder": disk }))
            .unwrap(),
    );
    assert!(est["bytes"].as_u64().unwrap() > 0 && est["fits"] == true);
}
