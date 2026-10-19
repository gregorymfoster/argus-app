//! Export through the tool layer: formats, names, metadata and never overwriting.

use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// A JPEG with camera EXIF, including a location that must never leave Argus.
fn write_jpeg_with_exif(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..320 * 240 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 90)
        .encode(&data, 320, 240, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    let ascii = |s: &str| exif::Value::Ascii(vec![s.as_bytes().to_vec()]);
    let f = |tag, value| exif::Field {
        tag,
        ifd_num: exif::In::PRIMARY,
        value,
    };
    let fields = [
        f(exif::Tag::Make, ascii("SONY")),
        f(exif::Tag::Model, ascii("ILCE-7RM5")),
        f(exif::Tag::DateTimeOriginal, ascii("2026:10:18 09:30:00")),
        f(exif::Tag::LensModel, ascii("FE 24-70mm F2.8 GM II")),
        f(exif::Tag::BodySerialNumber, ascii("1234567")),
        f(exif::Tag::GPSLatitudeRef, ascii("N")),
        f(
            exif::Tag::GPSLatitude,
            exif::Value::Rational(vec![(39, 1).into(), (5, 1).into(), (0, 1).into()]),
        ),
    ];
    let mut w = exif::experimental::Writer::new();
    for x in &fields {
        w.push_field(x);
    }
    let mut tiff = Cursor::new(Vec::new());
    w.write(&mut tiff, false).unwrap();
    let tiff = tiff.into_inner();
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    out.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    out.extend_from_slice(&jpeg[2..]);
    std::fs::write(path, out).unwrap();
}

struct Lib {
    tmp: tempfile::TempDir,
    tools: Tools,
    ids: Vec<i64>,
    src: PathBuf,
}

fn library(shoot_name: &str) -> Lib {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("card");
    for i in 0..3u8 {
        write_jpeg_with_exif(&src.join(format!("DSC0{i}.JPG")), i * 2 + 3);
    }
    let tools = Tools::new(tmp.path().join("library"));
    json1(
        &tools,
        "import_folder",
        json!({ "folder": src, "shoot_name": shoot_name }),
    );
    let ids = json1(&tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    Lib {
        tmp,
        tools,
        ids,
        src,
    }
}

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match tools
        .call(name, args)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .remove(0)
    {
        Part::Json(v) => v,
        p => panic!("{name}: expected JSON, got {p:?}"),
    }
}

fn paths(v: &Value) -> Vec<PathBuf> {
    v["exported"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| PathBuf::from(x["path"].as_str().unwrap()))
        .collect()
}

fn names(dir: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    n.sort();
    n
}

fn read_exif(path: &Path) -> exif::Exif {
    exif::Reader::new()
        .read_from_container(&mut std::io::BufReader::new(
            std::fs::File::open(path).unwrap(),
        ))
        .unwrap()
}

#[test]
fn jpegs_are_named_by_shoot_keep_capture_metadata_and_drop_location() {
    let l = library("Boat trip");
    let before: Vec<Vec<u8>> = l
        .ids
        .iter()
        .enumerate()
        .map(|(i, _)| std::fs::read(l.src.join(format!("DSC0{i}.JPG"))).unwrap())
        .collect();
    let out = l.tmp.path().join("Exports");
    let r = json1(
        &l.tools,
        "export_photos",
        json!({ "photo_ids": l.ids, "folder": out, "long_edge": 160, "quality": 80 }),
    );
    assert_eq!(r["failed"], json!([]));
    assert_eq!(names(&out), ["2026-10-18_Boat-trip"], "a folder per Shoot");
    assert_eq!(
        names(&out.join("2026-10-18_Boat-trip")),
        [
            "2026-10-18_Boat-trip_DSC00.jpg",
            "2026-10-18_Boat-trip_DSC01.jpg",
            "2026-10-18_Boat-trip_DSC02.jpg"
        ]
    );
    let p = &paths(&r)[0];
    let img = image::open(p).unwrap().to_rgb8();
    assert_eq!((img.width(), img.height()), (160, 120));

    let e = read_exif(p);
    let s = |t| {
        e.get_field(t, exif::In::PRIMARY)
            .map(|f| f.display_value().to_string())
    };
    assert_eq!(s(exif::Tag::Model).as_deref(), Some("\"ILCE-7RM5\""));
    assert_eq!(
        s(exif::Tag::DateTimeOriginal).as_deref(),
        Some("2026-10-18 09:30:00")
    );
    assert!(s(exif::Tag::LensModel).is_some());
    assert_eq!(
        s(exif::Tag::Orientation).as_deref(),
        Some("row 0 at top and column 0 at left")
    );
    assert!(
        s(exif::Tag::GPSLatitude).is_none(),
        "location must never be exported"
    );
    assert!(s(exif::Tag::BodySerialNumber).is_none());

    for (i, b) in before.iter().enumerate() {
        assert_eq!(
            &std::fs::read(l.src.join(format!("DSC0{i}.JPG"))).unwrap(),
            b
        );
    }
}

#[test]
fn exporting_again_never_overwrites_and_leaves_no_temp_files() {
    let l = library("2026-10-18_Tahoe");
    let out = l.tmp.path().join("out");
    for _ in 0..2 {
        json1(
            &l.tools,
            "export_photos",
            json!({ "photo_ids": [l.ids[0]], "folder": out, "long_edge": 64, "shoot_folder": false }),
        );
    }
    assert_eq!(
        names(&out),
        ["2026-10-18_Tahoe_DSC00-2.jpg", "2026-10-18_Tahoe_DSC00.jpg"]
    );
}

#[test]
fn sequence_names_number_across_calls_and_tiff_decodes() {
    let l = library("2026-10-18_Tahoe");
    let out = l.tmp.path().join("seq");
    // The UI exports one Photo per call, for progress: it passes the position and total.
    for (i, id) in l.ids.iter().enumerate() {
        json1(
            &l.tools,
            "export_photos",
            json!({ "photo_ids": [id], "folder": out, "format": "tiff", "naming": "sequence", "shoot_folder": false,
                    "long_edge": 100, "sequence_start": i + 1, "sequence_total": 3 }),
        );
    }
    assert_eq!(
        names(&out),
        [
            "2026-10-18_Tahoe_001.tif",
            "2026-10-18_Tahoe_002.tif",
            "2026-10-18_Tahoe_003.tif"
        ]
    );
    let img = image::open(out.join("2026-10-18_Tahoe_002.tif")).unwrap();
    assert_eq!(img.width().max(img.height()), 100);
}

#[test]
fn original_format_copies_the_file_untouched_with_an_xmp_lightroom_reads() {
    let l = library("2026-10-18_Tahoe");
    json1(
        &l.tools,
        "edit_recipe",
        json!({ "photo_ids": [l.ids[0]], "set": { "exposure": 0.5, "shadows": 40, "hsl.blue.sat": -20, "temp": 12 } }),
    );
    json1(
        &l.tools,
        "set_verdict",
        json!({ "photo_ids": [l.ids[0]], "verdict": "reject", "as": "person" }),
    );
    let out = l.tmp.path().join("lr");
    // An XMP already there (say, Lightroom's) is never replaced.
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("DSC00.xmp"), "lightroom's own").unwrap();
    let r = json1(
        &l.tools,
        "export_photos",
        json!({ "photo_ids": [l.ids[0]], "folder": out, "format": "original", "naming": "original", "shoot_folder": false }),
    );
    assert_eq!(names(&out), ["DSC00-2.JPG", "DSC00-2.xmp", "DSC00.xmp"]);
    assert_eq!(
        std::fs::read_to_string(out.join("DSC00.xmp")).unwrap(),
        "lightroom's own"
    );
    assert_eq!(
        std::fs::read(out.join("DSC00-2.JPG")).unwrap(),
        std::fs::read(l.src.join("DSC00.JPG")).unwrap()
    );
    assert_eq!(r["exported"][0]["sidecar"], json!(out.join("DSC00-2.xmp")));
    let xmp = std::fs::read_to_string(out.join("DSC00-2.xmp")).unwrap();
    for want in [
        "crs:Exposure2012=\"+0.50\"",
        "crs:Shadows2012=\"+40\"",
        "crs:SaturationAdjustmentBlue=\"-20\"",
        "crs:IncrementalTemperature=\"+12\"",
        "xmp:Rating=\"-1\"",
    ] {
        assert!(xmp.contains(want), "missing {want} in\n{xmp}");
    }
}

#[test]
fn a_photo_that_cant_export_is_reported_and_the_rest_still_go() {
    let l = library("2026-10-18_Tahoe");
    std::fs::rename(l.src.join("DSC01.JPG"), l.tmp.path().join("moved.JPG")).unwrap();
    let out = l.tmp.path().join("out");
    let r = json1(
        &l.tools,
        "export_photos",
        json!({ "photo_ids": l.ids, "folder": out, "long_edge": 64, "shoot_folder": false }),
    );
    assert_eq!(paths(&r).len(), 2);
    assert_eq!(r["failed"][0]["photo_id"], l.ids[1]);
    assert!(r["failed"][0]["error"]
        .as_str()
        .unwrap()
        .contains("not reachable"));
    assert_eq!(
        names(&out).len(),
        2,
        "no empty placeholder for the failed one"
    );
}
