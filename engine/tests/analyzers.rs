//! Specialist models behind the Analyzer seam (ADR 0013): Facts are computed once, kept in
//! the Library and recomputed when the preview changes; each specialist is checked on
//! Greg's bench JPEGs (private repo only); and every shipped model has a known hash and an allowed
//! licence.

use argus_engine::analyzers::{self, embedding, eyes, faces, REGISTRY};
use argus_engine::library::{Library, Photo, PhotoFilter};
use argus_engine::masks::Analysis;
use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn bench(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/bench/cull")
        .join(p)
}

/// A real blink: both eyes shut, mid-kiss.
const BLINK: &str = "2025-03-22/photos/DSC00597.jpg";
/// One face, eyes wide open, looking at the camera.
const OPEN: &str = "2025-03-22/photos/DSC00624.jpg";
/// Two people close up (the Mask tests' photo).
const PEOPLE: &str = "2024-08-10/photos/DSC05579.jpg";

/// A Library with `photos` imported, and its Photos in capture order.
fn library(photos: &[&str]) -> (tempfile::TempDir, Tools, Vec<Photo>) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2025-03-22_party");
    std::fs::create_dir_all(&src).unwrap();
    for p in photos {
        std::fs::copy(bench(p), src.join(Path::new(p).file_name().unwrap())).unwrap();
    }
    let tools = Tools::new(tmp.path().join("Argus.library"));
    tools
        .call("import_folder", json!({ "folder": src }))
        .unwrap();
    let lib = Library::open(tools.library()).unwrap();
    let all = lib.list_photos(&PhotoFilter::default()).unwrap();
    (tmp, tools, all)
}

fn by_name<'a>(photos: &'a [Photo], p: &str) -> &'a Photo {
    let name = Path::new(p).file_name().unwrap().to_str().unwrap();
    photos.iter().find(|x| x.file_name == name).unwrap()
}

#[test]
fn eyes_closed_and_open_are_told_apart() {
    if !crate::has_bench() {
        return;
    }
    let (_tmp, tools, photos) = library(&[BLINK, OPEN]);
    let lib = Library::open(tools.library()).unwrap();
    let read = |p: &str| {
        let f = analyzers::facts(&lib, by_name(&photos, p), &["eyes"]).unwrap();
        eyes::read(&f["eyes"])
    };
    let blink = read(BLINK);
    assert!(
        blink.iter().any(|r| r.read && r.eyes_closed),
        "the kiss has closed eyes: {blink:?}"
    );
    let open: Vec<_> = read(OPEN).into_iter().filter(|r| r.read).collect();
    assert_eq!(open.len(), 1, "one face near enough to read: {open:?}");
    assert!(open[0].read && !open[0].eyes_closed, "{open:?}");
    assert!(open[0].blink.iter().all(|&b| b < 0.3), "{open:?}");
    assert!(open[0].focus > eyes::SOFT, "a sharp portrait: {open:?}");
}

#[test]
fn landmarks_land_on_the_face() {
    if !crate::has_bench() {
        return;
    }
    let (_tmp, tools, photos) = library(&[OPEN]);
    let lib = Library::open(tools.library()).unwrap();
    let f = analyzers::facts(&lib, &photos[0], &["eyes", "faces"]).unwrap();
    let read = eyes::read(&f["eyes"]);
    let i = read.iter().position(|r| r.read).unwrap();
    let face = faces::read(&f["faces"])[read[i].face];
    let marks = &eyes::landmarks(&f["eyes"])[0];
    assert_eq!(marks.len(), 478);
    let inside = marks
        .iter()
        .filter(|p| {
            p[0] > face.x - face.w * 0.2
                && p[0] < face.x + face.w * 1.2
                && p[1] > face.y - face.h * 0.3
                && p[1] < face.y + face.h * 1.2
        })
        .count();
    assert!(inside > 470, "{inside} of 478 landmarks near the face box");
}

#[test]
fn facts_are_kept_and_redone_when_the_preview_changes() {
    if !crate::has_bench() {
        return;
    }
    let (_tmp, tools, photos) = library(&[PEOPLE]);
    let lib = Library::open(tools.library()).unwrap();
    let photo = &photos[0];
    let first = analyzers::facts(&lib, photo, &["faces", "signals"]).unwrap();
    assert_eq!(faces::read(&first["faces"]).len(), 2);
    let computed_at = |lib: &Library| -> String {
        let db = rusqlite::Connection::open(lib.root().join("library.db")).unwrap();
        db.query_row(
            "SELECT computed_at FROM facts WHERE analyzer = 'faces'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let stamp = computed_at(&lib);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let again = analyzers::facts(&lib, photo, &["faces", "signals"]).unwrap();
    assert_eq!(again, first);
    assert_eq!(computed_at(&lib), stamp, "read back, not recomputed");

    // A new preview (Argus's render replacing the camera's, say) makes the Facts stale.
    image::RgbImage::from_pixel(64, 48, image::Rgb([90, 90, 90]))
        .save(&photo.preview)
        .unwrap();
    let redone = analyzers::facts(&lib, photo, &["faces", "signals"]).unwrap();
    assert!(
        faces::read(&redone["faces"]).is_empty(),
        "a grey frame has no faces"
    );
    assert_ne!(computed_at(&lib), stamp);
}

#[test]
fn mask_analysis_from_facts_matches_a_fresh_one() {
    if !crate::has_bench() {
        return;
    }
    let (_tmp, tools, photos) = library(&[PEOPLE]);
    let lib = Library::open(tools.library()).unwrap();
    let photo = &photos[0];
    let stored = argus_engine::render::analysis(&lib, photo).unwrap();
    let img = image::open(&photo.preview).unwrap().to_rgb8();
    let fresh = Analysis::of(&argus_engine::RgbImage {
        width: img.width() as usize,
        height: img.height() as usize,
        data: img.into_raw(),
    })
    .unwrap();
    assert_eq!(stored.faces, fresh.faces);
    for (a, b) in stored.seg.iter().zip(&fresh.seg) {
        let worst = a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0f32, f32::max);
        assert!(worst < 0.003, "planes are stored at 8 bits: {worst}");
    }
}

#[test]
fn the_queue_analyses_a_shoot_and_analyze_photo_reports_it() {
    if !crate::has_bench() {
        return;
    }
    let (_tmp, tools, _photos) = library(&[BLINK, OPEN]);
    {
        let mut lib = Library::open(tools.library()).unwrap();
        analyzers::enqueue(&mut lib, None, None).unwrap();
    }
    argus_engine::jobs::drain(&tools, argus_engine::compute::Compute::Gentle).unwrap();
    let db = rusqlite::Connection::open(tools.library().join("library.db")).unwrap();
    let n: i64 = db
        .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        n as usize,
        2 * REGISTRY.len(),
        "every Analyzer on both Photos"
    );

    let out = tools
        .call("analyze_photo", json!({ "photo_id": 1 }))
        .unwrap();
    let Part::Json(v) = &out[0] else { panic!() };
    // Faces too small to read get no eye Facts, so look for one that was read.
    let read = v["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["eyes_closed"].is_boolean());
    assert!(read.is_some_and(|f| f["focus"].is_number()), "{v}");
    // The eyes Analyzer's caption describes the main faces ("face: smiling", "2 faces all
    // look good", "faces left to right: ...").
    let summary = v["summary"].as_str().unwrap();
    assert!(
        ["face:", "look good", "looks good", "left to right"]
            .iter()
            .any(|w| summary.contains(w)),
        "{v}"
    );

    // The grid's badges read the same stored Facts, without computing anything.
    let flags = argus_engine::commands::dispatch(&tools, "eye_flags", json!({})).unwrap();
    for id in [1, 2] {
        let out = tools
            .call("analyze_photo", json!({ "photo_id": id }))
            .unwrap();
        let Part::Json(v) = &out[0] else { panic!() };
        let closed = v["faces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["eyes_closed"] == true);
        let flagged = flags
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == id && f["closed"] == true);
        assert_eq!(closed, flagged, "photo {id}: {v} vs {flags}");
    }
}

#[test]
fn the_embedding_tells_a_burst_from_a_new_scene() {
    if !crate::has_bench() {
        return;
    }
    // DSC00530-31 are one burst; DSC00534 and DSC00535 are different moments.
    let names = [
        "2025-03-22/photos/DSC00530.jpg",
        "2025-03-22/photos/DSC00531.jpg",
        "2025-03-22/photos/DSC00534.jpg",
        "2025-03-22/photos/DSC00535.jpg",
    ];
    let (_tmp, tools, photos) = library(&names);
    let lib = Library::open(tools.library()).unwrap();
    let v: Vec<Vec<f32>> = names
        .iter()
        .map(|n| {
            let f = analyzers::facts(&lib, by_name(&photos, n), &["embedding"]).unwrap();
            embedding::vector(&f["embedding"]).unwrap()
        })
        .collect();
    let burst = embedding::similarity(&v[0], &v[1]);
    let apart = embedding::similarity(&v[2], &v[3]);
    assert!(burst > 0.85, "burst frames: {burst}");
    assert!(apart < 0.6, "different moments: {apart}");
    assert!((embedding::similarity(&v[0], &v[0]) - 1.0).abs() < 1e-4);
}

#[test]
fn every_model_is_in_the_manifest_with_an_allowed_licence() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let models = manifest["models"].as_array().unwrap();
    // Permissive licences only: weights we can ship in a distributed app (ADR 0013).
    const ALLOWED: [&str; 4] = ["Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause"];
    let ids: Vec<&str> = REGISTRY.iter().map(|a| a.id()).collect();
    for m in models {
        let file = m["file"].as_str().unwrap();
        assert!(ALLOWED.contains(&m["license"].as_str().unwrap()), "{m}");
        assert!(ids.contains(&m["analyzer"].as_str().unwrap()), "{m}");
        assert!(m["source"].as_str().unwrap().starts_with("https://"), "{m}");
        if m["embedded"] == true {
            let bytes = std::fs::read(dir.join(file)).unwrap();
            assert_eq!(bytes.len() as u64, m["bytes"].as_u64().unwrap(), "{file}");
            let sha = sha256(&bytes);
            assert_eq!(
                sha,
                m["sha256"].as_str().unwrap(),
                "{file}: update the manifest"
            );
        }
    }
    for entry in std::fs::read_dir(&dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.ends_with(".onnx") {
            assert!(
                models.iter().any(|m| m["file"] == name.as_str()),
                "{name} is not in engine/models/manifest.json"
            );
        }
    }
}

fn sha256(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// A face read with blink scores `[l, r]`, smile and jaw-open blendshapes, at `x` with `size`.
fn face(x: f32, size: f32, blink: [f32; 2], smile: f32, jaw: f32) -> eyes::FaceRead {
    let mut shapes = vec![0.0; 52];
    shapes[eyes::shape::SMILE[0]] = smile;
    shapes[eyes::shape::SMILE[1]] = smile;
    shapes[eyes::shape::JAW_OPEN] = jaw;
    eyes::FaceRead {
        face: 0,
        read: true,
        blink,
        eyes_closed: blink.iter().all(|&b| b > eyes::CLOSED),
        focus: 0.5,
        shapes,
        turn: 0.0,
        size,
        x,
    }
}

#[test]
fn the_face_caption_says_who_looks_off_and_skips_the_background() {
    let good = face(0.2, 0.07, [0.1, 0.1], 0.9, 0.0);
    let half_blink = face(0.5, 0.07, [0.5, 0.55], 0.1, 0.0);
    let mid_word = face(0.8, 0.07, [0.1, 0.1], 0.05, 0.4);
    let laugh = face(0.4, 0.07, [0.9, 0.9], 0.95, 0.3);
    let background = face(0.9, 0.02, [0.9, 0.9], 0.0, 0.0);
    assert!(
        eyes::awkward(&laugh).is_empty(),
        "a laugh with eyes shut is the moment"
    );
    assert_eq!(eyes::awkward(&half_blink), ["half-blink"]);
    assert_eq!(eyes::awkward(&mid_word), ["mouth open mid-word"]);
    assert_eq!(
        eyes::caption(&[good.clone(), laugh.clone(), background.clone()]).unwrap(),
        "2 faces all look good (+1 smaller)"
    );
    assert_eq!(
        eyes::caption(&[mid_word, good.clone(), half_blink]).unwrap(),
        "3 faces, 1 looks good; 2nd from left: not smiling, half-blink; \
         3rd from left: not smiling, mouth open mid-word"
    );
    assert_eq!(eyes::caption(&[good]).unwrap(), "face: smiling");
}

#[test]
fn the_face_model_reads_expressions_and_head_turn() {
    if !crate::has_bench() {
        return;
    }
    let (_tmp, tools, photos) = library(&[OPEN, BLINK]);
    let lib = Library::open(tools.library()).unwrap();
    let read = |p: &str| {
        let f = analyzers::facts(&lib, by_name(&photos, p), &["eyes"]).unwrap();
        eyes::read(&f["eyes"])
    };
    let open: Vec<_> = read(OPEN).into_iter().filter(|r| r.read).collect();
    assert_eq!(open[0].shapes.len(), 52, "every blendshape kept: {open:?}");
    assert!(open[0].turn.abs() < 0.5, "looking at the camera: {open:?}");
    assert!(
        open[0].size > 0.05 && (0.0..=1.0).contains(&open[0].x),
        "{open:?}"
    );
}
