//! `argus work --drain` runs what's queued and exits, as scripts and the Mac smoke test expect.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn write_jpeg(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..320 * 240 * 3)
        .map(|i| (i as u8).wrapping_mul(7))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 320, 240, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

#[test]
fn drain_exits_once_the_queue_is_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    write_jpeg(&src.join("DSC00001.jpg"));
    let lib = tmp.path().join("Library");
    let argus = env!("CARGO_BIN_EXE_argus");
    let ok = Command::new(argus)
        .arg("import")
        .arg(&src)
        .arg("--library")
        .arg(&lib)
        .env("ARGUS_QUEUE", "off")
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    let mut work = Command::new(argus)
        .args(["work", "--drain", "--library"])
        .arg(&lib)
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = work.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            work.kill().unwrap();
            panic!("argus work --drain still running after the queue emptied");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
