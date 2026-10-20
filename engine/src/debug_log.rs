//! The Debug log: one JSON object per line in `debug.jsonl`, beside `argus.log` in
//! `~/Library/Logs/Argus`, for tuning speed and tracing failures afterwards (Greg,
//! 2026-10-20: "really thorough debug logs so we can performance tune and fix").
//!
//! Every line has `t` (UTC, milliseconds), `pid`, `proc` (the app, `argus work`, ...) and
//! `kind`, plus fields of its own. Kinds: `start`, `command` (every UI command with its
//! duration; polls only when slow or failing), `job` (each Queue job: duration, outcome
//! and the CPU, energy and GPU the AI used meanwhile), `ollama` and `claude` (each model
//! request: tokens, load, prefill and output speed, cost), `model_wait`, `resources` (the
//! Resource meter, every 30 s while a model works, every 5 min otherwise) and `note`
//! (everything [`crate::logs::note`] says). `jq` reads it; `scripts/debug-log` sums it up.
//!
//! Nothing is written until [`init`] names a folder (the app, `argus serve` and the CLI's
//! long commands do). Several processes append to one file; each line is one write.
//! At [`MAX_BYTES`] it rotates to `debug.1.jsonl` .. `debug.{KEEP}.jsonl`. Local only,
//! never uploaded. `ARGUS_DEBUG_LOG=off` turns it off.

use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Rotate past this size.
pub const MAX_BYTES: u64 = 8_000_000;
/// Rotated files kept: at most (KEEP + 1) x 8 MB on disk.
pub const KEEP: usize = 3;
/// Check the size every this many lines (a stat per line would be wasteful).
const CHECK_EVERY: u32 = 200;

struct Sink {
    path: PathBuf,
    file: std::fs::File,
    proc: String,
    lines: u32,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// Start writing this process's events to `dir/debug.jsonl`, naming it `proc` in each
/// line. Calling it again moves the log. Returns the file's path.
pub fn init(dir: &Path, proc: &str) -> std::io::Result<PathBuf> {
    if std::env::var("ARGUS_DEBUG_LOG").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        return Err(std::io::Error::other("ARGUS_DEBUG_LOG=off"));
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join("debug.jsonl");
    rotate(&path, MAX_BYTES, KEEP)?;
    let file = open(&path)?;
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Sink {
        path: path.clone(),
        file,
        proc: proc.to_owned(),
        lines: 0,
    });
    Ok(path)
}

/// Where this process logs, if it does.
pub fn path() -> Option<PathBuf> {
    SINK.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|s| s.path.clone())
}

/// Whether [`init`] has run: callers can skip building costly fields when nobody logs.
pub fn enabled() -> bool {
    SINK.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

fn open(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// One event: `fields` (an object) after `t`, `pid`, `proc` and `kind`. A no-op until
/// [`init`]; a failed write is dropped rather than disturbing the work it describes.
pub fn event(kind: &str, fields: Value) {
    let mut guard = SINK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sink) = guard.as_mut() else {
        return;
    };
    let mut line = line(kind, &sink.proc, fields);
    line.push('\n');
    let _ = sink.file.write_all(line.as_bytes());
    sink.lines += 1;
    if sink.lines >= CHECK_EVERY {
        sink.lines = 0;
        if std::fs::metadata(&sink.path).is_ok_and(|m| m.len() > MAX_BYTES) {
            let _ = rotate(&sink.path, MAX_BYTES, KEEP);
            if let Ok(f) = open(&sink.path) {
                sink.file = f;
            }
        }
    }
}

/// The text of one line (without the newline). Public for tests.
pub fn line(kind: &str, proc: &str, fields: Value) -> String {
    let mut m = match fields {
        Value::Object(f) => f,
        Value::Null => Map::new(),
        other => Map::from_iter([("value".to_owned(), other)]),
    };
    for k in ["t", "pid", "proc", "kind"] {
        m.remove(k);
    }
    // The common fields first, so lines read and sort by time.
    let head = format!(
        "{{\"t\":{},\"pid\":{},\"proc\":{},\"kind\":{}",
        json!(now_ms()),
        std::process::id(),
        json!(proc),
        json!(kind)
    );
    if m.is_empty() {
        return head + "}";
    }
    let rest = Value::Object(m).to_string();
    format!("{head},{}", &rest[1..])
}

/// Now in UTC with milliseconds: `2026-10-20T12:00:01.234Z`.
pub fn now_ms() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs =
        crate::library::time::from_system(UNIX_EPOCH + std::time::Duration::from_secs(d.as_secs()));
    format!("{secs}.{:03}Z", d.subsec_millis())
}

/// Shift `debug.jsonl` to `debug.1.jsonl` (and older ones up, dropping past `keep`) when
/// it is larger than `max` bytes. Public for tests.
pub fn rotate(path: &Path, max: u64, keep: usize) -> std::io::Result<()> {
    if !std::fs::metadata(path).is_ok_and(|m| m.len() > max) {
        return Ok(());
    }
    let numbered = |n: usize| path.with_extension(format!("{n}.jsonl"));
    let _ = std::fs::remove_file(numbered(keep));
    for n in (1..keep).rev() {
        let _ = std::fs::rename(numbered(n), numbered(n + 1));
    }
    std::fs::rename(path, numbered(1))
}

/// Times something and logs it as one `kind` event when it ends, with `duration_ms` and
/// whatever [`Span::end`] adds (or `ok: false` if it is dropped without ending).
pub struct Span {
    kind: &'static str,
    fields: Value,
    start: Instant,
    done: bool,
}

pub fn span(kind: &'static str, fields: Value) -> Span {
    Span {
        kind,
        fields,
        start: Instant::now(),
        done: false,
    }
}

impl Span {
    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// Log it now, with `more` fields beside the ones it started with.
    pub fn end(mut self, more: Value) {
        self.finish(more);
    }

    fn finish(&mut self, more: Value) {
        self.done = true;
        let mut f = match std::mem::take(&mut self.fields) {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        f.insert(
            "duration_ms".into(),
            json!((self.elapsed_ms() * 10.0).round() / 10.0),
        );
        if let Value::Object(m) = more {
            f.extend(m);
        }
        event(self.kind, Value::Object(f));
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if !self.done {
            self.finish(json!({ "ok": false, "error": "ended early" }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_one_json_object_with_the_common_fields_first() {
        let l = line(
            "job",
            "app",
            json!({ "kind_of_job": "suggest", "ok": true }),
        );
        assert!(!l.contains('\n'));
        let v: Value = serde_json::from_str(&l).unwrap();
        assert_eq!(v["kind"], "job");
        assert_eq!(v["proc"], "app");
        assert_eq!(v["ok"], true);
        assert!(v["t"].as_str().unwrap().ends_with('Z'));
        assert_eq!(
            v["t"].as_str().unwrap().len(),
            "2026-10-20T12:00:01.234Z".len()
        );
        assert!(l.starts_with("{\"t\":"), "{l}");
    }

    #[test]
    fn rotation_keeps_a_few_numbered_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("debug.jsonl");
        for round in 0..5 {
            std::fs::write(&p, format!("round {round} and some padding")).unwrap();
            rotate(&p, 4, 3).unwrap();
        }
        assert!(!p.exists());
        let read = |n: usize| std::fs::read_to_string(dir.path().join(format!("debug.{n}.jsonl")));
        assert!(read(1).unwrap().starts_with("round 4"));
        assert!(read(3).unwrap().starts_with("round 2"));
        assert!(read(4).is_err());
        // Small files stay put.
        std::fs::write(&p, "x").unwrap();
        rotate(&p, 4, 3).unwrap();
        assert!(p.exists());
    }
}
