//! Import shows a RAW's camera preview at once, then `render_previews` swaps in Argus's
//! own render. Runs on whichever sample RAWs are present (fixtures/raw, and the A7R V
//! files in fixtures/raw-a7rv); skips when there are none.

use argus_engine::compute::Compute;
use argus_engine::library::{ImportOptions, Library, PhotoFilter};
use argus_engine::{decode, raw_size, Orientation};
use std::path::{Path, PathBuf};

fn samples() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures");
    let mut out = Vec::new();
    for dir in ["raw", "raw-a7rv"] {
        if let Ok(entries) = std::fs::read_dir(root.join(dir)) {
            for e in entries.flatten() {
                if e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("arw"))
                {
                    out.push(e.path());
                }
            }
        }
    }
    out.sort();
    out
}

#[test]
fn header_size_matches_decoded_size() {
    for path in samples() {
        let data = std::fs::read(&path).unwrap();
        let raw = decode(&path).unwrap();
        let (cw, ch) = raw.crop_size();
        let want = match raw.orientation {
            Orientation::Rotate90 | Orientation::Rotate270 => (ch, cw, raw.orientation),
            _ => (cw, ch, raw.orientation),
        };
        assert_eq!(raw_size(&path, &data).unwrap(), want, "{}", path.display());
    }
}

#[test]
fn camera_previews_first_then_argus_renders() {
    let samples = samples();
    if samples.is_empty() {
        eprintln!("no sample RAWs, skipping");
        return;
    }
    // Two RAWs keep it quick: one A7R V when present, and one other.
    let picks: Vec<&PathBuf> = samples
        .iter()
        .rev()
        .take(1)
        .chain(samples.first())
        .collect();
    let card = tempfile::tempdir().unwrap();
    for p in &picks {
        std::os::unix::fs::symlink(p, card.path().join(p.file_name().unwrap())).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::open(dir.path()).unwrap();
    let report = lib.import(card.path(), &ImportOptions::default()).unwrap();
    let shoot = report.shoot_id.unwrap();
    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    assert_eq!(photos.len(), picks.len());
    let mut camera_files = Vec::new();
    for p in &photos {
        let name = p.preview.to_string_lossy().into_owned();
        assert!(name.ends_with(".camera.jpg"), "{name}");
        let img = image::open(&p.preview).unwrap();
        // Upright, with the Photo's own aspect.
        assert_eq!(
            img.width() > img.height(),
            p.width > p.height,
            "{}",
            p.file_name
        );
        camera_files.push(p.preview.clone());
    }
    assert_eq!(lib.pending_previews(Some(shoot)).unwrap(), photos.len());

    let seen = std::sync::Mutex::new(Vec::new());
    let done = lib
        .render_previews(Some(shoot), Compute::Gentle, &|d, t| {
            seen.lock().unwrap().push((d, t))
        })
        .unwrap();
    assert_eq!(done.rendered, photos.len());
    assert_eq!(
        *seen.lock().unwrap().last().unwrap(),
        (photos.len(), photos.len())
    );
    assert_eq!(lib.pending_previews(None).unwrap(), 0);
    for p in lib.list_photos(&PhotoFilter::default()).unwrap() {
        assert!(!p.preview.to_string_lossy().contains(".camera."));
        let img = image::open(&p.preview).unwrap();
        assert_eq!(img.width().max(img.height()), 2048);
    }
    assert!(
        camera_files.iter().all(|f| !f.exists()),
        "camera previews are cleaned up"
    );
}
