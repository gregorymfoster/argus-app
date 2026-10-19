//! Library tests through its public interface (the tool-layer seam): import a folder of
//! generated JPEGs, list, flag, and write Recipes. RAW import is covered by
//! `import_fixture_raws` when fixtures/raw has ARWs.

use argus_engine::library::{Author, ImportOptions, Library, PhotoFilter, Verdict};
use std::path::{Path, PathBuf};

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

/// A fake card: three distinct JPEGs (one in a subfolder), one exact copy, one junk file.
fn shoot_folder(dir: &Path) -> PathBuf {
    let src = dir.join("2017-06-20");
    write_jpeg(&src.join("a.jpg"), 640, 480, 3);
    write_jpeg(&src.join("b.JPG"), 3000, 2000, 5);
    write_jpeg(&src.join("DCIM/c.jpeg"), 480, 640, 7);
    std::fs::copy(src.join("a.jpg"), src.join("a-copy.jpg")).unwrap();
    std::fs::write(src.join("notes.txt"), "not a photo").unwrap();
    std::fs::write(src.join(".hidden.jpg"), "skip me").unwrap();
    src
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let m = std::fs::metadata(&p).unwrap().modified().unwrap();
                out.push((p.clone(), std::fs::read(&p).unwrap(), m));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn import_dedupes_builds_previews_and_never_touches_originals() {
    let tmp = tempfile::tempdir().unwrap();
    let src = shoot_folder(tmp.path());
    let before = snapshot(&src);
    let mut lib = Library::open(&tmp.path().join("Argus.library")).unwrap();

    let r = lib.import(&src, &ImportOptions::default()).unwrap();
    assert_eq!((r.found, r.imported, r.duplicates), (4, 3, 1), "{r:?}");
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert_eq!(r.shoot_name, "2017-06-20");

    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    assert_eq!(photos.len(), 3);
    for p in &photos {
        assert_eq!(p.verdict, Verdict::Unflagged);
        let img = image::open(&p.preview).expect("preview is a readable JPEG");
        assert!(img.width().max(img.height()) <= 2048);
        assert!(p.preview.starts_with(lib.root()));
    }
    let big = photos.iter().find(|p| p.width == 3000).unwrap();
    assert_eq!(big.height, 2000);
    assert_eq!(image::open(&big.preview).unwrap().width(), 2048);

    // Importing again is harmless: nothing new, no empty Shoot.
    let again = lib.import(&src, &ImportOptions::default()).unwrap();
    assert_eq!(
        (again.imported, again.duplicates, again.shoot_id),
        (0, 4, None)
    );
    assert_eq!(lib.shoots().unwrap().len(), 1);
    assert_eq!(lib.shoots().unwrap()[0].photos, 3);

    assert_eq!(
        snapshot(&src),
        before,
        "originals must be byte- and mtime-identical"
    );
}

#[test]
fn verdicts_filter_and_person_overrides_are_logged() {
    let tmp = tempfile::tempdir().unwrap();
    let src = shoot_folder(tmp.path());
    let mut lib = Library::open(&tmp.path().join("lib")).unwrap();
    let shoot = lib
        .import(&src, &ImportOptions::default())
        .unwrap()
        .shoot_id;
    let ids: Vec<_> = lib
        .list_photos(&PhotoFilter::default())
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();

    let cull = lib.begin_batch(Author::Ai, "AI cull").unwrap();
    lib.set_verdict(
        &ids[..2],
        Verdict::Pick,
        Author::Ai,
        Some("sharp, eyes open"),
        Some(cull),
    )
    .unwrap();
    lib.set_verdict(
        &ids[2..],
        Verdict::Reject,
        Author::Ai,
        Some("missed focus"),
        Some(cull),
    )
    .unwrap();

    let picks = lib
        .list_photos(&PhotoFilter {
            shoot,
            verdict: Some(Verdict::Pick),
        })
        .unwrap();
    assert_eq!(picks.len(), 2);
    assert_eq!(picks[0].verdict_by, Some(Author::Ai));
    assert_eq!(picks[0].reason.as_deref(), Some("sharp, eyes open"));

    // Greg flips one Pick to Reject and re-confirms the AI Reject: one Override.
    lib.set_verdict(&ids[..1], Verdict::Reject, Author::Person, None, None)
        .unwrap();
    lib.set_verdict(&ids[2..], Verdict::Reject, Author::Person, None, None)
        .unwrap();
    assert_eq!(lib.override_count().unwrap(), 1);
    let rejects = lib
        .list_photos(&PhotoFilter {
            shoot,
            verdict: Some(Verdict::Reject),
        })
        .unwrap();
    assert_eq!(rejects.len(), 2);

    lib.set_verdict(&ids, Verdict::Unflagged, Author::Person, None, None)
        .unwrap();
    let unflagged = lib
        .list_photos(&PhotoFilter {
            shoot: None,
            verdict: Some(Verdict::Unflagged),
        })
        .unwrap();
    assert_eq!(unflagged.len(), 3);

    assert!(lib
        .set_verdict(&[9999], Verdict::Pick, Author::Person, None, None)
        .is_err());
}

#[test]
fn recipes_are_versioned_and_survive_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let src = shoot_folder(tmp.path());
    let root = tmp.path().join("lib");
    let id = {
        let mut lib = Library::open(&root).unwrap();
        lib.import(&src, &ImportOptions::default()).unwrap();
        let id = lib.list_photos(&PhotoFilter::default()).unwrap()[0].id;
        assert!(lib.read_recipe(id).unwrap().is_none());
        let v1 = lib
            .write_recipe(
                id,
                &serde_json::json!({"schema": 1, "exposure": 0.3}),
                Author::Ai,
                None,
            )
            .unwrap();
        let v2 = lib
            .write_recipe(
                id,
                &serde_json::json!({"schema": 1, "exposure": 0.5}),
                Author::Person,
                None,
            )
            .unwrap();
        assert_eq!((v1, v2), (1, 2));
        id
    };
    let lib = Library::open(&root).unwrap();
    let r = lib.read_recipe(id).unwrap().unwrap();
    assert_eq!(r.version, 2);
    assert_eq!(r.author, Author::Person);
    assert_eq!(r.recipe["exposure"], 0.5);
}

#[test]
fn import_fixture_raws() {
    let raws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw");
    if !raws.join("DSC03945.ARW").exists() {
        eprintln!("library: no fixtures/raw/*.ARW, skipping RAW import");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(tmp.path()).unwrap();
    let r = lib.import(&raws, &ImportOptions::default()).unwrap();
    assert!(r.imported >= 1 && r.failed.is_empty(), "{r:?}");
    for p in lib.list_photos(&PhotoFilter::default()).unwrap() {
        assert_eq!(p.kind, "raw");
        assert!(p.camera.contains("ILCE-7RM2"), "{}", p.camera);
        assert!(!p.captured_at.starts_with("1970"));
        assert_eq!(p.width.max(p.height), 7952);
        let e = &p.exposure;
        eprintln!("{}: {e:?}", p.file_name);
        assert!(e.shutter.is_some_and(|s| s > 0.0 && s < 60.0), "{e:?}");
        assert!(e.iso.is_some_and(|i| i >= 50), "{e:?}");
        // The camera's embedded 1616 px JPEG until Argus's own render replaces it.
        let img = image::open(&p.preview).unwrap();
        assert_eq!(img.width().max(img.height()), 1616);
    }
    let compute = argus_engine::compute::Compute::Full;
    lib.render_previews(r.shoot_id, compute, &|_, _| {})
        .unwrap();
    for p in lib.list_photos(&PhotoFilter::default()).unwrap() {
        let img = image::open(&p.preview).unwrap();
        assert_eq!(img.width().max(img.height()), 2048);
    }
}
