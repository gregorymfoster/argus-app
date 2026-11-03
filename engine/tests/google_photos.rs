//! Google Photos (docs/export.md): sign-in in the browser with the person's own OAuth
//! client, then exports go up through the Queue into an album named after the Shoot,
//! made once and made again if the person deleted it. A fake Google on a loopback port
//! stands in for accounts.google.com and the Library API.

use crate::{env_lock, must_finish};
use argus_engine::commands::dispatch;
use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::library::{JobStatus, Library};
use argus_engine::tools::Tools;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fake {
    /// (method, path, auth header, body) of every call.
    calls: Vec<(String, String, String, String)>,
    albums: Vec<String>,
    /// Albums the person deleted in Google Photos.
    gone: Vec<String>,
}

fn serve(fake: Arc<Mutex<Fake>>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let fake = fake.clone();
            std::thread::spawn(move || handle(s, &fake));
        }
    });
    base
}

fn handle(s: TcpStream, fake: &Mutex<Fake>) {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let (mut len, mut auth) = (0usize, String::new());
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap();
        match k.to_ascii_lowercase().as_str() {
            "content-length" => len = v.trim().parse().unwrap(),
            "authorization" => auth = v.trim().to_string(),
            _ => {}
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).unwrap();
    let body = String::from_utf8_lossy(&body).into_owned();
    let mut f = fake.lock().unwrap();
    f.calls.push((method, path.clone(), auth, body.clone()));
    let (code, reply) = match path.as_str() {
        "/token" if body.contains("grant_type=authorization_code") => {
            assert!(body.contains("code=good-code"), "{body}");
            assert!(
                body.contains("code_verifier="),
                "PKCE verifier sent: {body}"
            );
            assert!(body.contains("client_secret=s3cret"), "{body}");
            (200, json!({ "access_token": "a1", "refresh_token": "r1" }))
        }
        "/token" => {
            assert!(body.contains("refresh_token=r1"), "{body}");
            (200, json!({ "access_token": "a2" }))
        }
        "/v1/uploads" => {
            return reply_raw(s, 200, &format!("upload-{}", f.calls.len()));
        }
        "/v1/albums" => {
            let id = format!("album-{}", f.albums.len() + 1);
            f.albums.push(id.clone());
            (
                200,
                json!({ "id": id, "title": serde_json::from_str::<Value>(&body).unwrap()["album"]["title"] }),
            )
        }
        "/v1/mediaItems:batchCreate" => {
            let v: Value = serde_json::from_str(&body).unwrap();
            let album = v["albumId"].as_str().unwrap_or("").to_string();
            if f.gone.contains(&album) {
                (400, json!({ "error": { "message": "Invalid album ID" } }))
            } else {
                let n = v["newMediaItems"].as_array().unwrap().len();
                let res: Vec<Value> = (0..n)
                    .map(|_| json!({ "status": { "message": "Success" } }))
                    .collect();
                (200, json!({ "newMediaItemResults": res }))
            }
        }
        _ => (404, json!({})),
    };
    drop(f);
    reply_raw(s, code, &reply.to_string());
}

fn reply_raw(mut s: TcpStream, code: u16, body: &str) {
    let _ = s.write_all(
        format!(
            "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
}

fn write_jpeg(path: &std::path::Path, seed: u8) {
    let data: Vec<u8> = (0..32 * 24 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 32, 24, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn query(url: &str, key: &str) -> String {
    url.split_once('?')
        .unwrap()
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
        .unwrap()
        .to_string()
}

fn unenc(s: &str) -> String {
    s.replace("%3A", ":").replace("%2F", "/")
}

#[test]
fn signs_in_and_sends_exports_into_the_shoots_album() {
    let _env = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(Mutex::new(Fake::default()));
    let base = serve(fake.clone());
    std::env::set_var("ARGUS_GOOGLE_AUTH", format!("{base}/auth"));
    std::env::set_var("ARGUS_GOOGLE_TOKEN", format!("{base}/token"));
    std::env::set_var("ARGUS_GOOGLE_PHOTOS_API", format!("{base}/v1"));
    let account = dir.path().join("google-photos.json");
    std::env::set_var("ARGUS_GOOGLE_ACCOUNT", &account);
    let tools = Tools::new(dir.path().join("library"));
    Library::open(&dir.path().join("library")).unwrap();
    let run = |cmd: &str, a: Value| dispatch(&tools, cmd, a);

    // No client yet: nothing to connect with.
    let st = run("google_photos_status", json!({})).unwrap();
    assert_eq!(
        (st["client"].clone(), st["connected"].clone()),
        (json!(false), json!(false))
    );
    assert!(run("google_photos_connect", json!({ "open": false })).is_err());
    assert!(run(
        "google_photos_client",
        json!({ "client_id": "not-a-client" })
    )
    .is_err());
    let id = "123-abc.apps.googleusercontent.com";
    run(
        "google_photos_client",
        json!({ "client_id": id, "client_secret": "s3cret" }),
    )
    .unwrap();

    // Connect: Google's page asks for append-only Photos with PKCE, and the browser comes
    // back to Argus's loopback port.
    let url = run("google_photos_connect", json!({ "open": false })).unwrap()["auth_url"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(url.starts_with(&format!("{base}/auth?")), "{url}");
    assert_eq!(
        unenc(&query(&url, "scope")),
        "https://www.googleapis.com/auth/photoslibrary.appendonly"
    );
    assert_eq!(query(&url, "code_challenge_method"), "S256");
    assert_eq!(query(&url, "access_type"), "offline");
    let redirect = unenc(&query(&url, "redirect_uri"));
    assert!(redirect.starts_with("http://127.0.0.1:"), "{redirect}");
    assert_eq!(
        run("google_photos_status", json!({})).unwrap()["waiting"],
        json!(true)
    );
    // A request for another sign-in is turned away without connecting.
    let wrong = ureq::get(&format!("{redirect}/?code=good-code&state=nope"))
        .call()
        .unwrap()
        .into_string()
        .unwrap();
    assert!(wrong.contains("another request"), "{wrong}");
    must_finish(10, "sign-in failure", {
        let tools = Tools::new(dir.path().join("library"));
        move || loop {
            let st = dispatch(&tools, "google_photos_status", json!({})).unwrap();
            if st["waiting"] == json!(false) {
                assert_eq!(st["connected"], json!(false));
                assert!(st["error"].as_str().unwrap().contains("another request"));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    let url = run("google_photos_connect", json!({ "open": false })).unwrap()["auth_url"]
        .as_str()
        .unwrap()
        .to_string();
    let redirect = unenc(&query(&url, "redirect_uri"));
    let page = ureq::get(&format!(
        "{redirect}/?state={}&code=good-code&scope=x",
        query(&url, "state")
    ))
    .call()
    .unwrap()
    .into_string()
    .unwrap();
    assert!(page.contains("connected to Google Photos"), "{page}");
    must_finish(10, "sign-in", {
        let tools = Tools::new(dir.path().join("library"));
        move || loop {
            let st = dispatch(&tools, "google_photos_status", json!({})).unwrap();
            if st["connected"] == json!(true) {
                assert_eq!(st["error"], json!(null));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    // The account stays on this Mac, readable only by the person.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&account).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // Send two exports: a Queue job uploads both into a new album named for the Shoot.
    let files: Vec<_> = (1..=2u8)
        .map(|i| {
            let p = dir.path().join(format!("2026-11-02_Boat-Trip_DSC0{i}.jpg"));
            write_jpeg(&p, i + 2);
            p
        })
        .collect();
    let send = |files: &[std::path::PathBuf]| {
        let out = run(
            "send_to_photos",
            json!({ "to": "google_photos", "paths": files, "album": "2026-11-02_Boat-Trip" }),
        )
        .unwrap();
        let job = out["job_id"].as_i64().unwrap();
        jobs::drain(&tools, Compute::Gentle).unwrap();
        let lib = Library::open(&dir.path().join("library")).unwrap();
        let j = lib.job(job).unwrap();
        assert_eq!(j.status, JobStatus::Done, "{:?}", j.error);
        assert!(
            j.label.contains("Sending 2 to Google Photos"),
            "{}",
            j.label
        );
        j.result.unwrap()
    };
    let r = send(&files);
    assert_eq!(
        (r["sent"].clone(), r["album_id"].clone()),
        (json!(2), json!("album-1"))
    );
    {
        let f = fake.lock().unwrap();
        let uploads: Vec<_> = f.calls.iter().filter(|c| c.1 == "/v1/uploads").collect();
        assert_eq!(uploads.len(), 2);
        assert!(
            uploads.iter().all(|c| c.2 == "Bearer a2"),
            "the refreshed token is used"
        );
        let album = f.calls.iter().find(|c| c.1 == "/v1/albums").unwrap();
        assert!(album.3.contains("2026-11-02_Boat-Trip"));
        let batch = f
            .calls
            .iter()
            .find(|c| c.1 == "/v1/mediaItems:batchCreate")
            .unwrap();
        assert!(
            batch.3.contains("\"albumId\":\"album-1\"")
                && batch.3.contains("2026-11-02_Boat-Trip_DSC01.jpg"),
            "{}",
            batch.3
        );
    }

    // Again: the same album, not a second one.
    let r = send(&files);
    assert_eq!(r["album_id"], json!("album-1"));
    assert_eq!(fake.lock().unwrap().albums.len(), 1);

    // The person deleted the album in Google Photos: Argus makes it again.
    fake.lock().unwrap().gone.push("album-1".into());
    let r = send(&files);
    assert_eq!(
        (r["sent"].clone(), r["album_id"].clone()),
        (json!(2), json!("album-2"))
    );

    // Disconnect: sending to Google is refused with where to fix it.
    run("google_photos_disconnect", json!({})).unwrap();
    let e = run(
        "send_to_photos",
        json!({ "to": "google_photos", "paths": files }),
    )
    .unwrap_err();
    assert!(e.0.contains("Settings › Export"), "{}", e.0);
    for k in [
        "ARGUS_GOOGLE_AUTH",
        "ARGUS_GOOGLE_TOKEN",
        "ARGUS_GOOGLE_PHOTOS_API",
        "ARGUS_GOOGLE_ACCOUNT",
    ] {
        std::env::remove_var(k);
    }
}
