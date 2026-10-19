//! `argus serve`: a localhost-only HTTP bridge over the Library, so the React UI can run
//! in a plain browser (and under Playwright) against real Photos. ADR 0005, point 3.
//! It is a thin transport over [`argus_engine::commands`]: `/api/<command>` runs that
//! command with the POST body (or the GET query string) as its arguments.
//!
//! GET  /api/list_shoots              (any tool in engine::tools, by name)
//! POST /api/import_folder           {"folder":"/path","shoot_name":"name"}
//! POST /api/cull_shoot              {"shoot_id":N,"provider":"ollama","compute":"gentle"}
//! GET  /api/photos?shoot_id=N&verdict=pick
//! POST /api/flag                    {"ids":[1,2],"verdict":"pick","author":"person"}
//! POST /api/edit                    {"photo_id":N,"recipe":{...}}
//! POST /api/open_library            {"path":"/path"} (and the other `argus_engine::libraries` commands)
//! GET  /previews/...                the open Library's preview JPEGs
//!
//! Photos come back with `preview` rewritten to its `/previews/...` URL.

use argus_engine::commands::library_file;
use argus_engine::libraries::Libraries;
use serde_json::{json, Map, Value};
use tiny_http::{Header, Method, Request, Response, Server};

type BoxError = Box<dyn std::error::Error>;

pub fn run(library: &std::path::Path, port: u16) -> Result<(), BoxError> {
    // Pinned to --library, with nothing remembered: switching lasts until it exits.
    let libs = Libraries::load(None, Some(library.to_path_buf()), library.to_path_buf());
    if let Some(b) = argus_engine::library::Library::open(library)?.backup_if_due()? {
        eprintln!("argus serve: backed up the Library to {}", b.display());
    }
    // The background Queue works while the bridge is up, as in the app.
    let root = library.to_path_buf();
    let _queue = (std::env::var("ARGUS_QUEUE").as_deref() != Ok("off")).then(|| {
        argus_engine::jobs::Worker::start(
            move || Some(root.clone()),
            argus_engine::tools::Tools::new,
        )
    });
    let server = Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    eprintln!(
        "argus serve: http://127.0.0.1:{port} (Library {})",
        library.display()
    );
    // One thread per request, so `progress` answers while an import runs.
    let libs = std::sync::Arc::new(libs);
    for mut req in server.incoming_requests() {
        let libs = libs.clone();
        std::thread::spawn(move || {
            let (status, resp) = match route(&libs, &mut req) {
                Ok(r) => (200, r),
                Err(e) => (400, Reply::Json(json!({ "error": e.to_string() }))),
            };
            let _ = match resp {
                Reply::Json(v) => req.respond(
                    Response::from_string(v.to_string())
                        .with_status_code(status)
                        .with_header(header("Content-Type", "application/json")),
                ),
                Reply::File(bytes) => req.respond(
                    Response::from_data(bytes)
                        .with_header(header("Content-Type", "image/jpeg"))
                        .with_header(header("Cache-Control", "max-age=31536000, immutable")),
                ),
                Reply::NotFound => {
                    req.respond(Response::from_string("not found").with_status_code(404))
                }
            };
        });
    }
    Ok(())
}

enum Reply {
    Json(Value),
    File(Vec<u8>),
    NotFound,
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k, v).expect("static header")
}

/// A query string as command arguments: numbers become numbers, the rest strings.
fn query_args(query: &str) -> Value {
    let mut m = Map::new();
    for (k, v) in query.split('&').filter_map(|kv| kv.split_once('=')) {
        let v = v
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::from(v));
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

fn route(libs: &Libraries, req: &mut Request) -> Result<Reply, BoxError> {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let method = req.method().clone();
    if method == Method::Get && path.starts_with("/previews/") {
        return Ok(
            match libs
                .current()
                .and_then(|root| library_file(&root, path))
                .map(std::fs::read)
            {
                Some(Ok(bytes)) => Reply::File(bytes),
                _ => Reply::NotFound,
            },
        );
    }
    let Some(command) = path.strip_prefix("/api/") else {
        return Ok(Reply::NotFound);
    };
    let args = if method == Method::Post {
        let mut text = String::new();
        req.as_reader().read_to_string(&mut text)?;
        if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text)?
        }
    } else {
        query_args(query)
    };
    let mut out = libs.call(command, args)?;
    if command == "photos" {
        for p in out.as_array_mut().into_iter().flatten() {
            let url = format!("/{}", p["preview"].as_str().unwrap_or_default());
            p["preview"] = Value::String(url);
        }
    }
    Ok(Reply::Json(out))
}
