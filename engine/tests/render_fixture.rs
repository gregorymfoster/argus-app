use argus_engine::{decode, develop, DevelopOptions};
use std::path::Path;

#[test]
fn renders_fixture_at_512() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw/DSC03945.ARW");
    if !path.exists() {
        eprintln!("fixture missing, skipping");
        return;
    }
    let raw = decode(&path).expect("decode");
    assert!(raw.model.contains("7"), "model: {}", raw.model);
    let img = develop(
        &raw,
        &DevelopOptions {
            long_edge: Some(512),
            ..Default::default()
        },
    );
    assert_eq!(img.width.max(img.height), 512);
    assert_eq!(img.data.len(), img.width * img.height * 3);
    let mean = img.data.iter().map(|&v| v as f64).sum::<f64>() / img.data.len() as f64;
    assert!((30.0..200.0).contains(&mean), "mean brightness {mean}");
}

/// The Edit screen's sliders re-render from cached stages 1-3: the result must be exactly
/// what a fresh decode and develop gives, for each Recipe in turn.
#[test]
fn cached_rerender_matches_fresh_develop() {
    use argus_engine::library::{ImportOptions, Library, PhotoFilter};
    use argus_engine::recipe::Recipe;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw/DSC04949.ARW");
    if !path.exists() {
        eprintln!("fixture missing, skipping");
        return;
    }
    let card = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(&path, card.path().join("DSC04949.ARW")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::open(dir.path()).unwrap();
    lib.import(card.path(), &ImportOptions::default()).unwrap();
    let photo = lib.list_photos(&PhotoFilter::default()).unwrap().remove(0);
    let raw = decode(&path).unwrap();
    for exposure in [0.0, 0.7, -0.4] {
        let recipe =
            Recipe::from_value(&serde_json::json!({ "global": { "exposure": exposure } })).unwrap();
        let cached = argus_engine::render::render_photo(&photo, &recipe, Some(640), None).unwrap();
        let fresh = develop(
            &raw,
            &DevelopOptions {
                long_edge: Some(640),
                recipe,
                ..Default::default()
            },
        );
        assert_eq!((cached.width, cached.height), (fresh.width, fresh.height));
        assert!(cached.data == fresh.data, "exposure {exposure}");
    }
}

#[test]
fn a_file_that_is_not_a_raw_says_so_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("EMPTY.ARW");
    std::fs::write(&empty, b"").unwrap();
    let e = decode(&empty).err().expect("an empty file can't decode").0;
    assert_eq!(e, "not a RAW file Argus can read (0 bytes)");
}
