//! Google Photos: send exported files to the person's Google Photos, into an album named
//! after the Shoot (docs/export.md, "Send to photo apps").
//!
//! Google cut the Library API back on 2025-03-31: apps can no longer read a person's whole
//! library, but the `photoslibrary.appendonly` scope still uploads, creates albums and adds
//! to the albums the app made. That is all Argus needs, so it asks for nothing else and
//! never lists albums: the album it made for each name is remembered in the Library
//! (setting `google_photos_albums`), and made again if the person deleted it.
//!
//! Sign-in is OAuth for installed apps in the browser (loopback redirect + PKCE). No key
//! ships with Argus: the person makes a Desktop OAuth client in their own Google Cloud
//! project once and pastes its ID and secret into Settings › Export. The client and the
//! refresh token live in one 0600 file on this Mac ([`Config::account_file`]), never in
//! the Library, so they don't travel with Library backups.
//!
//! Tests point everything at a fake server with `ARGUS_GOOGLE_AUTH`, `ARGUS_GOOGLE_TOKEN`,
//! `ARGUS_GOOGLE_PHOTOS_API` and `ARGUS_GOOGLE_ACCOUNT`.

use crate::library::Library;
use crate::{Error, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const SCOPE: &str = "https://www.googleapis.com/auth/photoslibrary.appendonly";
/// Google takes at most 50 new items per batchCreate.
const BATCH: usize = 50;
/// How long the browser has to come back to Argus.
const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);
/// Library setting: album title → the album id Argus made for it.
const ALBUMS: &str = "google_photos_albums";

/// Where Argus talks to Google, and where the account is kept.
#[derive(Debug, Clone)]
pub struct Config {
    pub auth_url: String,
    pub token_url: String,
    pub api: String,
    pub account_file: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        let account_file = std::env::var_os("ARGUS_GOOGLE_ACCOUNT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default();
                if cfg!(target_os = "macos") {
                    home.join("Library/Application Support/Argus/google-photos.json")
                } else {
                    home.join(".local/share/argus/google-photos.json")
                }
            });
        Config {
            auth_url: env(
                "ARGUS_GOOGLE_AUTH",
                "https://accounts.google.com/o/oauth2/v2/auth",
            ),
            token_url: env("ARGUS_GOOGLE_TOKEN", "https://oauth2.googleapis.com/token"),
            api: env(
                "ARGUS_GOOGLE_PHOTOS_API",
                "https://photoslibrary.googleapis.com/v1",
            ),
            account_file,
        }
    }
}

/// The person's own OAuth client and, once signed in, the refresh token.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Account {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

fn load(c: &Config) -> Option<Account> {
    let raw = std::fs::read(&c.account_file).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn save(c: &Config, a: &Account) -> Result<()> {
    if let Some(dir) = c.account_file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error(format!("Google Photos: {e}")))?;
    }
    let tmp = c.account_file.with_extension("json.tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o
            .open(&tmp)
            .map_err(|e| Error(format!("Google Photos: can't save the account: {e}")))?;
        f.write_all(&serde_json::to_vec_pretty(a).unwrap_or_default())
            .map_err(|e| Error(format!("Google Photos: can't save the account: {e}")))?;
    }
    std::fs::rename(&tmp, &c.account_file)
        .map_err(|e| Error(format!("Google Photos: can't save the account: {e}")))
}

/// Sign-in in progress (one at a time per process), and how the last one ended.
#[derive(Default)]
struct SignIn {
    waiting: bool,
    started: Option<Instant>,
    auth_url: Option<String>,
    error: Option<String>,
}
static SIGN_IN: Mutex<SignIn> = Mutex::new(SignIn {
    waiting: false,
    started: None,
    auth_url: None,
    error: None,
});

/// What Settings › Export shows: {client, connected, waiting, waited_s, auth_url, error}.
pub fn status(c: &Config) -> Value {
    let a = load(c);
    let s = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner());
    json!({
        "client": a.as_ref().is_some_and(|a| !a.client_id.is_empty()),
        "client_id": a.as_ref().map(|a| a.client_id.clone()),
        "connected": a.as_ref().is_some_and(|a| a.refresh_token.is_some()),
        "waiting": s.waiting,
        "waited_s": s.started.filter(|_| s.waiting).map(|t| t.elapsed().as_secs()),
        "auth_url": s.auth_url.clone().filter(|_| s.waiting),
        "error": s.error.clone(),
    })
}

/// Keep the person's OAuth client. A new client forgets any sign-in made with the old one.
pub fn set_client(c: &Config, client_id: &str, client_secret: &str) -> Result<Value> {
    let id = client_id.trim();
    if !id.ends_with(".apps.googleusercontent.com") {
        return Err(Error(
            "That isn't a Google OAuth client ID: it ends in .apps.googleusercontent.com".into(),
        ));
    }
    let old = load(c);
    let same = old.as_ref().is_some_and(|o| o.client_id == id);
    save(
        c,
        &Account {
            client_id: id.to_string(),
            client_secret: client_secret.trim().to_string(),
            refresh_token: old.and_then(|o| o.refresh_token).filter(|_| same),
        },
    )?;
    Ok(status(c))
}

/// Forget the sign-in (the client stays, so connecting again is one click).
pub fn disconnect(c: &Config) -> Result<Value> {
    if let Some(mut a) = load(c) {
        a.refresh_token = None;
        save(c, &a)?;
    }
    Ok(status(c))
}

fn random(n: usize) -> String {
    let mut buf = vec![0u8; n];
    let ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok();
    if !ok {
        // No /dev/urandom: the clock and the address are still unguessable enough for a
        // ten-minute loopback sign-in.
        let seed = format!("{:?}{:p}", std::time::SystemTime::now(), &buf);
        buf = <sha2::Sha256 as sha2::Digest>::digest(seed.as_bytes()).to_vec();
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn pkce_challenge(verifier: &str) -> String {
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier))
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                o.push(b as char)
            }
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            x => out.push(x),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Start signing in: listen on a loopback port, open Google's consent page in the browser
/// and return at once with {auth_url}; a thread waits for the browser to come back, swaps
/// the code for a refresh token and saves it. [`status`] says how it went.
pub fn connect(c: &Config, open_browser: bool) -> Result<Value> {
    let a = load(c)
        .filter(|a| !a.client_id.is_empty())
        .ok_or_else(|| Error("Add your Google OAuth client ID first".into()))?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| Error(format!("Google Photos: can't listen for the sign-in: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| Error(format!("Google Photos: {e}")))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = random(48);
    let state = random(16);
    let url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        c.auth_url,
        enc(&a.client_id),
        enc(&redirect),
        enc(SCOPE),
        pkce_challenge(&verifier),
        state
    );
    {
        let mut s = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner());
        *s = SignIn {
            waiting: true,
            started: Some(Instant::now()),
            auth_url: Some(url.clone()),
            error: None,
        };
    }
    let c2 = c.clone();
    std::thread::spawn(move || {
        let r = await_redirect(&listener, &state).and_then(|code| {
            let token = exchange(&c2, &a, &code, &verifier, &redirect)?;
            let mut a = a;
            a.refresh_token = Some(token);
            save(&c2, &a)
        });
        let mut s = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner());
        s.waiting = false;
        s.auth_url = None;
        s.error = r.err().map(|e| e.0);
    });
    if open_browser {
        open_in_browser(&url);
    }
    Ok(json!({ "auth_url": url }))
}

/// Where the person makes their OAuth client.
pub const CONSOLE: &str = "https://console.cloud.google.com/apis/credentials";

/// Open Google Cloud's Credentials page, for Settings › Export's "Open Google Cloud".
pub fn open_console() {
    open_in_browser(CONSOLE);
}

fn open_in_browser(url: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Wait for Google to send the browser back with ?code=…&state=…, answer it with a page
/// that says what happened, and return the code.
fn await_redirect(listener: &TcpListener, state: &str) -> Result<String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| Error(format!("Google Photos: {e}")))?;
    let deadline = Instant::now() + SIGN_IN_WAIT;
    loop {
        if Instant::now() > deadline {
            return Err(Error(
                "Google sign-in timed out after 10 minutes; try Connect again".into(),
            ));
        }
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(Error(format!("Google Photos: {e}"))),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let path = req.split_whitespace().nth(1).unwrap_or("/");
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let get = |k: &str| {
            query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(key, _)| *key == k)
                .map(|(_, v)| dec(v))
        };
        // A favicon or a stray request: not the redirect, keep waiting.
        if get("code").is_none() && get("error").is_none() {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            continue;
        }
        let result = if get("state").as_deref() != Some(state) {
            Err(Error(
                "Google sign-in came back for another request; try Connect again".into(),
            ))
        } else if let Some(err) = get("error") {
            Err(Error(if err == "access_denied" {
                "Google sign-in was cancelled".into()
            } else {
                format!("Google sign-in failed: {err}")
            }))
        } else {
            Ok(get("code").unwrap_or_default())
        };
        let body = match &result {
            Ok(_) => "Argus is connected to Google Photos. You can close this tab.".to_string(),
            Err(e) => format!("{} You can close this tab.", e.0),
        };
        let page = format!(
            "<!doctype html><meta charset=utf-8><title>Argus</title><body style=\"font:14px -apple-system,sans-serif;background:#1f1e1d;color:#e8e6e3;display:grid;place-items:center;height:90vh\"><p>{body}</p>"
        );
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            )
            .as_bytes(),
        );
        return result;
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(300))
        .build()
}

fn http_err(what: &str, e: ureq::Error) -> Error {
    match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v["error"]["message"]
                        .as_str()
                        .or(v["error_description"].as_str())
                        .or(v["error"].as_str())
                        .map(String::from)
                })
                .unwrap_or(body);
            Error(format!("Google Photos {what}: {code} {}", msg.trim()))
        }
        other => Error(format!("Google Photos {what}: {other}")),
    }
}

fn exchange(c: &Config, a: &Account, code: &str, verifier: &str, redirect: &str) -> Result<String> {
    let r: Value = agent()
        .post(&c.token_url)
        .send_form(&[
            ("client_id", &a.client_id),
            ("client_secret", &a.client_secret),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect),
        ])
        .map_err(|e| http_err("sign-in", e))?
        .into_json()
        .map_err(|e| Error(format!("Google Photos sign-in: {e}")))?;
    r["refresh_token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| Error("Google didn't return a refresh token; try Connect again".into()))
}

/// A fresh access token from the saved refresh token.
fn access_token(c: &Config) -> Result<String> {
    let a = load(c).ok_or_else(not_connected)?;
    let refresh = a.refresh_token.clone().ok_or_else(not_connected)?;
    let r: Value = agent()
        .post(&c.token_url)
        .send_form(&[
            ("client_id", &a.client_id),
            ("client_secret", &a.client_secret),
            ("refresh_token", &refresh),
            ("grant_type", "refresh_token"),
        ])
        .map_err(|e| {
            let e = http_err("sign-in", e);
            if e.0.contains("invalid_grant") || e.0.contains("expired") || e.0.contains("revoked") {
                Error("Google Photos sign-in expired: connect again in Settings › Export".into())
            } else {
                e
            }
        })?
        .into_json()
        .map_err(|e| Error(format!("Google Photos sign-in: {e}")))?;
    r["access_token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| Error("Google didn't return an access token".into()))
}

fn not_connected() -> Error {
    Error("Google Photos isn't connected: connect it in Settings › Export".into())
}

fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("heic") | Some("heif") => "image/heic",
        Some("tif") | Some("tiff") => "image/tiff",
        Some("png") => "image/png",
        _ => "image/jpeg",
    }
}

fn albums(lib: &Library) -> serde_json::Map<String, Value> {
    lib.setting(ALBUMS)
        .ok()
        .flatten()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn create_album(c: &Config, token: &str, lib: &Library, title: &str) -> Result<String> {
    let r: Value = agent()
        .post(&format!("{}/albums", c.api))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(json!({ "album": { "title": title } }))
        .map_err(|e| http_err("album", e))?
        .into_json()
        .map_err(|e| Error(format!("Google Photos album: {e}")))?;
    let id = r["id"]
        .as_str()
        .ok_or_else(|| Error("Google Photos made no album".into()))?
        .to_string();
    let mut all = albums(lib);
    all.insert(title.to_string(), json!(id));
    lib.set_setting(ALBUMS, &Value::Object(all))?;
    Ok(id)
}

/// What one [`send`] call did.
#[derive(Debug, Default, Serialize)]
pub struct Sent {
    pub sent: usize,
    pub album: Option<String>,
    pub album_id: Option<String>,
    /// Files Google refused, with why; the rest still went.
    pub failed: Vec<(String, String)>,
}

/// Upload `paths` and add them to the album `album` (made the first time, and again if the
/// person deleted it), calling `progress(done, total)` after each file. Stops early, with
/// what it has sent so far still added, once `until` has passed; the caller queues the rest.
pub fn send(
    c: &Config,
    lib: &Library,
    paths: &[PathBuf],
    album: Option<&str>,
    until: Option<Instant>,
    progress: &dyn Fn(usize, usize),
) -> Result<(Sent, Vec<PathBuf>)> {
    let token = access_token(c)?;
    let album = album.map(str::trim).filter(|a| !a.is_empty());
    let mut album_id = match album {
        Some(t) => Some(match albums(lib).get(t).and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => create_album(c, &token, lib, t)?,
        }),
        None => None,
    };
    let mut out = Sent {
        album: album.map(String::from),
        ..Default::default()
    };
    let mut pending: Vec<(String, String)> = Vec::new(); // (upload token, file name)
    let mut left = Vec::new();
    let total = paths.len();
    for (i, path) in paths.iter().enumerate() {
        if until.is_some_and(|u| Instant::now() > u) && !pending.is_empty() {
            left = paths[i..].to_vec();
            break;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("photo.jpg")
            .to_string();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                out.failed.push((path.display().to_string(), e.to_string()));
                continue;
            }
        };
        let up = agent()
            .post(&format!("{}/uploads", c.api))
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/octet-stream")
            .set("X-Goog-Upload-Content-Type", mime(path))
            .set("X-Goog-Upload-File-Name", &name)
            .set("X-Goog-Upload-Protocol", "raw")
            .send_bytes(&bytes)
            .map_err(|e| http_err("upload", e))?
            .into_string()
            .map_err(|e| Error(format!("Google Photos upload: {e}")))?;
        pending.push((up.trim().to_string(), name));
        if pending.len() == BATCH {
            add(c, &token, lib, &mut album_id, album, &mut pending, &mut out)?;
        }
        progress(i + 1, total);
    }
    add(c, &token, lib, &mut album_id, album, &mut pending, &mut out)?;
    out.album_id = album_id;
    Ok((out, left))
}

/// Turn the uploads in `pending` into Photos in the album.
#[allow(clippy::result_large_err)] // ureq::Error is big; it is matched on once and dropped
fn add(
    c: &Config,
    token: &str,
    lib: &Library,
    album_id: &mut Option<String>,
    album: Option<&str>,
    pending: &mut Vec<(String, String)>,
    out: &mut Sent,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let items: Vec<Value> = pending
        .iter()
        .map(|(t, n)| json!({ "simpleMediaItem": { "uploadToken": t, "fileName": n } }))
        .collect();
    let call = |id: &Option<String>| {
        let mut body = json!({ "newMediaItems": items });
        if let Some(id) = id {
            body["albumId"] = json!(id);
        }
        agent()
            .post(&format!("{}/mediaItems:batchCreate", c.api))
            .set("Authorization", &format!("Bearer {token}"))
            .send_json(body)
    };
    let r = match call(album_id) {
        // The album Argus made is gone (deleted in Google Photos): make it again, once.
        Err(ureq::Error::Status(400 | 403 | 404, _)) if album_id.is_some() => {
            *album_id = Some(create_album(c, token, lib, album.unwrap_or("Argus"))?);
            call(album_id)
        }
        r => r,
    };
    let r: Value = r
        .map_err(|e| http_err("add", e))?
        .into_json()
        .map_err(|e| Error(format!("Google Photos add: {e}")))?;
    for (i, res) in r["newMediaItemResults"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let code = res["status"]["code"].as_i64().unwrap_or(0);
        if code == 0 {
            out.sent += 1;
        } else {
            let name = pending.get(i).map(|p| p.1.clone()).unwrap_or_default();
            let msg = res["status"]["message"].as_str().unwrap_or("refused");
            out.failed.push((name, msg.to_string()));
        }
    }
    pending.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_coding_round_trips() {
        let s = "4/0Ab_c d+e%f/é";
        assert_eq!(dec(&enc(s)), s);
        assert_eq!(dec("a+b%2Fc"), "a b/c");
        assert_eq!(dec("bad%2"), "bad%2");
    }

    #[test]
    fn pkce_challenge_is_unpadded_base64url_sha256() {
        // RFC 7636 S256: BASE64URL(SHA256(verifier)), unpadded; the expected value is
        // from Python's hashlib and base64.urlsafe_b64encode.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWjVXp1o"),
            "kqHBn24v7V-zuvlDphJMlBeSIBT02jridycIPXPbW3o"
        );
    }
}
