//! Search by meaning (SigLIP from the models folder). Skips when the model isn't there:
//! set ARGUS_MODELS to a folder holding siglip-image.onnx, siglip-text.onnx and
//! siglip-vocab.tsv.

use argus_engine::meaning::{self, Tokenizer};

fn vocab() -> Option<Tokenizer> {
    let v = std::fs::read_to_string(meaning::dir().join(meaning::VOCAB_FILE)).ok()?;
    Some(Tokenizer::new(&v))
}

#[test]
fn tokenizer_matches_hugging_face() {
    let Some(tok) = vocab() else {
        eprintln!(
            "skipped: no SigLIP vocabulary in {}",
            meaning::dir().display()
        );
        return;
    };
    // Ids from transformers' SiglipTokenizer on the same text.
    for (text, want) in [
        (
            "a bridge over a river",
            &[262, 266, 3929, 363, 262, 266, 3100][..],
        ),
        ("pickleball", &[17807, 3873]),
        ("Boats at sunset!", &[7824, 289, 8969]),
        ("kids playing in the snow", &[1064, 1376, 268, 260, 2468]),
        (
            "ünïcode café 2026",
            &[262, 20408, 198, 178, 5723, 14983, 705, 4234],
        ),
        ("Dog's  toy, red-ball", &[3014, 265, 322, 1226, 3873]),
    ] {
        let ids = tok.encode(text);
        assert_eq!(ids.len(), 64);
        assert_eq!(&ids[..want.len()], want, "{text}");
        assert!(
            ids[want.len()..].iter().all(|&i| i == 1),
            "{text}: end and padding"
        );
    }
}

#[test]
fn finds_sample_photos_by_what_they_show() {
    let raws = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw-a7rv");
    if !meaning::available() || !raws.join("DSC07856.ARW").exists() {
        eprintln!("skipped: needs the SigLIP model and fixtures/raw-a7rv");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let tools = argus_engine::tools::Tools::new(tmp.path().join("lib"));
    tools
        .call(
            "import_folder",
            serde_json::json!({ "folder": raws, "cull": false, "tag": false }),
        )
        .unwrap();
    argus_engine::jobs::drain(&tools, argus_engine::compute::Compute::Full).unwrap();
    let names = |query: &str| -> Vec<String> {
        let out = tools
            .call("search_photos", serde_json::json!({ "query": query }))
            .unwrap();
        let argus_engine::tools::Part::Json(v) = &out[0] else {
            panic!()
        };
        v["photos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["file_name"].as_str().unwrap().to_string())
            .collect()
    };
    // No Keywords or captions here: only the model can find these.
    assert_eq!(
        names("a bridge over a river").first().map(String::as_str),
        Some("DSC07856.ARW")
    );
    let mut pickle = names("pickleball");
    pickle.truncate(2);
    pickle.sort();
    assert_eq!(pickle, ["DSC08006.ARW", "DSC08009.ARW"]);
    assert!(
        names("a dog on the beach").is_empty(),
        "nothing like it here"
    );
}

/// A tiny HTTP server for `files`, honouring Range. The first request for `cut` sends only
/// its first half and hangs up, like a dropped connection.
fn serve(
    files: Vec<(String, Vec<u8>)>,
    cut: &'static str,
) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = log.clone();
    std::thread::spawn(move || {
        let mut cut_done = false;
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let path = first
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .trim_start_matches('/')
                .to_string();
            let mut from = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(r) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    from = r.trim().trim_end_matches('-').parse().unwrap();
                }
            }
            seen.lock().unwrap().push(format!("{path} from {from}"));
            let Some((_, body)) = files.iter().find(|(n, _)| *n == path) else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n");
                continue;
            };
            let rest = &body[from..];
            let status = if from > 0 {
                "206 Partial Content"
            } else {
                "200 OK"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                rest.len()
            );
            if path == cut && !cut_done {
                cut_done = true;
                let _ = stream.write_all(&rest[..rest.len() / 2]);
                continue;
            }
            let _ = stream.write_all(rest);
        }
    });
    (base, log)
}

#[test]
fn the_model_downloads_resumes_and_is_checked() {
    use sha2::Digest;
    let a: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let b: Vec<u8> = b"\xe2\x96\x81a\t-1.5\n".repeat(1000);
    let file = |name: &str, body: &[u8]| meaning::ModelFile {
        file: name.into(),
        bytes: body.len() as u64,
        sha256: format!("{:x}", sha2::Sha256::digest(body)),
    };
    let files = vec![file("a.onnx", &a), file("b.tsv", &b)];
    let (base, log) = serve(
        vec![("a.onnx".into(), a.clone()), ("b.tsv".into(), b.clone())],
        "a.onnx",
    );
    let tmp = tempfile::tempdir().unwrap();
    let total = (a.len() + b.len()) as u64;

    // The connection drops halfway through the first file: nothing is installed.
    assert!(meaning::download_to(tmp.path(), &base, &files, &|_, _| {}).is_err());
    assert!(!tmp.path().join("a.onnx").exists());
    assert!(tmp.path().join("a.onnx.part").exists());

    // Again: it picks up where it stopped, and reports progress up to the whole size.
    let last = std::sync::Mutex::new(0);
    meaning::download_to(tmp.path(), &base, &files, &|done, t| {
        assert_eq!(t, total);
        *last.lock().unwrap() = done;
    })
    .unwrap();
    assert_eq!(*last.lock().unwrap(), total);
    assert_eq!(std::fs::read(tmp.path().join("a.onnx")).unwrap(), a);
    assert_eq!(std::fs::read(tmp.path().join("b.tsv")).unwrap(), b);
    let log = log.lock().unwrap().clone();
    assert_eq!(log[0], "a.onnx from 0");
    assert!(
        log[1].starts_with("a.onnx from ") && log[1] != "a.onnx from 0",
        "resumed: {log:?}"
    );

    // A file that doesn't match its hash is thrown away, never installed.
    let mut wrong = files.clone();
    wrong[1].file = "c.tsv".into();
    let (base2, _) = serve(vec![("c.tsv".into(), b"not the vocab".repeat(10))], "");
    wrong[1].bytes = 130;
    let err = meaning::download_to(tmp.path(), &base2, &wrong, &|_, _| {}).unwrap_err();
    assert!(err.0.contains("damaged"), "{}", err.0);
    assert!(!tmp.path().join("c.tsv").exists() && !tmp.path().join("c.tsv.part").exists());
}

#[test]
fn the_manifest_lists_the_model_and_status_reports_it() {
    let files = meaning::files();
    let names: Vec<&str> = files.iter().map(|f| f.file.as_str()).collect();
    assert_eq!(
        names,
        [meaning::IMAGE_FILE, meaning::TEXT_FILE, meaning::VOCAB_FILE]
    );
    let s = meaning::status();
    assert_eq!(s.bytes, files.iter().map(|f| f.bytes).sum::<u64>());
    assert!(s.bytes > 400_000_000 && s.bytes < 450_000_000);
}
