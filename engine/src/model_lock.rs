//! One local model request at a time across Argus processes on this Mac.
//!
//! Ollama serves one long request at a time, and a request with other options (a Gentle
//! cull beside a Full bench) makes it reload the model. Left to Ollama, the app's cull sat
//! behind CLI runs all night with nothing on screen saying so (2026-10-19). So every Argus
//! process takes a [`Turn`] before it talks to the local model: a marker file per request
//! in a shared folder, first come first served, and whoever waits can say on whom.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static SEQ: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Turns this thread holds, so a request inside a held turn (a cull's model load)
    /// doesn't wait on itself.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
    /// Who this thread is, for others' waiting messages ([`as_who`]).
    static WHO: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The model is this process's until dropped.
pub struct Turn {
    marker: Option<PathBuf>,
}

impl Drop for Turn {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        if let Some(m) = &self.marker {
            let _ = std::fs::remove_file(m);
        }
    }
}

/// Where the markers live: ARGUS_MODEL_LOCK_DIR, else `argus-model` in the per-user temp
/// folder (the same one for the app and the CLI).
fn dir() -> PathBuf {
    std::env::var_os("ARGUS_MODEL_LOCK_DIR")
        .map(Into::into)
        .unwrap_or_else(|| std::env::temp_dir().join("argus-model"))
}

/// What another process sees while this one has the model: ARGUS_MODEL_LABEL, else what
/// this thread said with [`as_who`], else the command ("`argus cull`", "the Argus app").
pub fn who() -> String {
    if let Some(w) = WHO.with(|w| w.borrow().clone()) {
        return w;
    }
    if let Ok(l) = std::env::var("ARGUS_MODEL_LABEL") {
        return l;
    }
    let args: Vec<String> = std::env::args().collect();
    let exe = args
        .first()
        .and_then(|a| Path::new(a).file_name())
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if exe == "argus" {
        match args.get(1).map(String::as_str) {
            Some(cmd @ ("tool" | "agent")) => match args.get(2) {
                Some(name) if cmd == "tool" => format!("`argus tool {name}`"),
                _ => format!("`argus {cmd}`"),
            },
            Some(cmd) => format!("`argus {cmd}`"),
            None => "`argus`".into(),
        }
    } else if exe.contains("argus") {
        "the Argus app".into()
    } else {
        format!("`{exe}`")
    }
}

/// Name this thread's model work ("AI cull · 2026-10-18_Boat-Trip") until the guard drops.
pub fn as_who(label: impl Into<String>) -> impl Drop {
    struct Guard(Option<String>);
    impl Drop for Guard {
        fn drop(&mut self) {
            let prev = self.0.take();
            WHO.with(|w| *w.borrow_mut() = prev);
        }
    }
    Guard(WHO.with(|w| w.borrow_mut().replace(label.into())))
}

/// Wait for the model, then hold it until the Turn drops. While another Argus process has
/// it (or asked first), `waiting` hears who, again whenever that changes, and None once
/// this one goes ahead. Within one process requests don't wait on each other: the queue
/// already starts nothing while the app's own Agent or cull runs.
pub fn take(waiting: &dyn Fn(Option<&str>)) -> Turn {
    let nested = DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get() > 1
    });
    if nested {
        return Turn { marker: None };
    }
    let dir = dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!(
        "{nanos:024}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    );
    let asking = dir.join(format!("{name}.w"));
    if std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&asking, who()))
        .is_err()
    {
        // No shared folder: go ahead as before rather than not at all.
        return Turn { marker: None };
    }
    let poll = std::env::var("ARGUS_MODEL_LOCK_POLL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(Duration::from_millis(500), Duration::from_millis);
    let mut told: Option<String> = None;
    loop {
        match ahead_of(&dir, &name) {
            None => break,
            Some(w) => {
                if told.as_deref() != Some(w.as_str()) {
                    waiting(Some(&w));
                    told = Some(w);
                }
            }
        }
        std::thread::sleep(poll);
    }
    let held = dir.join(format!("{name}.u"));
    let marker = match std::fs::rename(&asking, &held) {
        Ok(()) => Some(held),
        Err(_) => {
            let _ = std::fs::remove_file(&asking);
            None
        }
    };
    if told.is_some() {
        waiting(None);
    }
    Turn { marker }
}

/// Who has the model in another process ("`argus cull` is using the model"), or None.
pub fn busy_elsewhere() -> Option<String> {
    ahead_of(&dir(), "~")
}

/// What `name` waits on: another live process holding the model, else one that asked
/// earlier. Markers of processes that died are swept up.
fn ahead_of(dir: &Path, name: &str) -> Option<String> {
    let me = std::process::id();
    let mut holder = None;
    let mut earlier = None;
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let file = e.file_name().to_string_lossy().into_owned();
        let Some((stem, state)) = file.rsplit_once('.') else {
            continue;
        };
        let Some(pid) = stem.split('-').nth(1).and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        if !crate::compute::alive(pid) {
            let _ = std::fs::remove_file(e.path());
            continue;
        }
        let label = std::fs::read_to_string(e.path()).unwrap_or_default();
        let label = if label.trim().is_empty() {
            "another Argus process".to_owned()
        } else {
            label.trim().to_owned()
        };
        match state {
            "u" if holder.is_none() => holder = Some(format!("{label} is using the model")),
            "w" if earlier.is_none() && stem < name => {
                earlier = Some(format!("{label} asked for the model first"))
            }
            _ => {}
        }
    }
    holder.or(earlier)
}
