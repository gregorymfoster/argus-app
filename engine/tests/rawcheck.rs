//! The header check that runs before rawler (engine/src/rawcheck.rs): every real RAW we
//! have passes, and a corrupt header fails as an error instead of aborting the process.

use argus_engine::{decode, rawcheck};
use std::path::{Path, PathBuf};

fn raws() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures");
    let mut out = Vec::new();
    for dir in ["raw", "raw-a7rv", "bench-raw"] {
        for e in std::fs::read_dir(root.join(dir))
            .into_iter()
            .flatten()
            .flatten()
        {
            if e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("arw"))
            {
                out.push(e.path());
            }
        }
    }
    out
}

/// An A7R V RAW with its Exif IFD pointer sent into the pixel data, as a fuzzed file did:
/// one changed byte, and the "entries" there claimed gigabytes and made rawler abort.
fn corrupt_copy(dir: &Path) -> Option<PathBuf> {
    let src = raws()
        .into_iter()
        .find(|p| p.to_string_lossy().contains("raw-a7rv"))?;
    let mut b = std::fs::read(&src).unwrap();
    assert_eq!(&b[0..2], b"II");
    let ifd0 = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let n = u16::from_le_bytes(b[ifd0..ifd0 + 2].try_into().unwrap()) as usize;
    let exif = (0..n)
        .map(|i| ifd0 + 2 + i * 12)
        .find(|&e| u16::from_le_bytes(b[e..e + 2].try_into().unwrap()) == 0x8769)
        .expect("IFD0 has an Exif pointer");
    let into_pixels = (b.len() / 2) as u32;
    b[exif + 8..exif + 12].copy_from_slice(&into_pixels.to_le_bytes());
    let bad = dir.join("BAD.ARW");
    std::fs::write(&bad, &b).unwrap();
    Some(bad)
}

#[test]
fn every_sample_raw_passes() {
    for p in raws() {
        rawcheck::check(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    }
}

#[test]
fn an_exif_pointer_into_the_pixels_fails_instead_of_aborting() {
    let dir = tempfile::tempdir().unwrap();
    let Some(bad) = corrupt_copy(dir.path()) else {
        eprintln!("fixtures/raw-a7rv missing, skipping");
        return;
    };
    let e = rawcheck::check(&bad).expect_err("the check catches it").0;
    assert!(e.starts_with("corrupt RAW file: "), "{e}");
    let e = decode(&bad).err().expect("decode refuses it").0;
    assert!(e.starts_with("corrupt RAW file: "), "{e}");
}

#[test]
fn import_lists_a_corrupt_raw_as_failed_and_takes_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("card");
    std::fs::create_dir_all(&card).unwrap();
    if corrupt_copy(&card).is_none() {
        eprintln!("fixtures/raw-a7rv missing, skipping");
        return;
    }
    let good = raws()
        .into_iter()
        .find(|p| p.to_string_lossy().contains("/raw/"))
        .unwrap();
    std::fs::copy(&good, card.join("GOOD.ARW")).unwrap();
    let tools = argus_engine::tools::Tools::new(dir.path().join("lib"));
    let r = argus_engine::commands::dispatch(
        &tools,
        "import_folder",
        serde_json::json!({ "folder": card }),
    )
    .unwrap();
    assert_eq!(r["imported"], 1, "{r}");
    let failed = r["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1, "{r}");
    assert!(failed[0]["file"].as_str().unwrap().ends_with("BAD.ARW"));
    assert!(
        failed[0]["error"]
            .as_str()
            .unwrap()
            .starts_with("corrupt RAW file: "),
        "{r}"
    );
}

#[test]
fn files_that_are_not_tiff_are_left_to_rawler() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.RAF");
    std::fs::write(&p, b"FUJIFILMCCD-RAW 0201").unwrap();
    assert!(rawcheck::check(&p).is_ok());
}
