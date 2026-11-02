//! All engine integration tests, in one binary (`--test engine`): one link instead of one per
//! file, which made `just check` after an engine edit mostly linking. Run one file's tests with
//! `cargo test --profile fast --test engine golden::` (the module is the old file).
//! Leave out `-p argus-engine`: on its own the engine resolves fewer dependency features (serde,
//! libc, syn, chrono, tracing) than the workspace, so cargo rebuilds ~90 crates (4-5 min cold).
//! Tests share this process, so anything that sets a process-wide env var takes `env_lock()`.

use std::sync::{Mutex, MutexGuard};

/// Greg's bench JPEGs (`fixtures/bench`) stay in his private repo, so tests on real faces
/// and skies skip where they are missing, as the golden test does without RAWs.
pub fn has_bench() -> bool {
    let ok = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/bench/cull")
        .is_dir();
    if !ok {
        eprintln!("no fixtures/bench, skipping");
    }
    ok
}

/// Serialises tests that set env vars (e.g. `ARGUS_VOLUMES`), which every test thread sees.
pub fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run `f` on its own thread and fail the test if it hasn't finished in `secs`: the pattern
/// for any code that goes parallel while sharing a lock, a cache or a one-time init, so a
/// deadlock fails loudly instead of hanging `just check` (docs/agents/concurrency.md). A hung
/// thread is left behind; the test binary exits anyway once the test fails.
pub fn must_finish<T: Send + 'static>(
    secs: u64,
    what: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(secs))
        .unwrap_or_else(|_| {
            panic!("{what} didn't finish in {secs} s: a deadlock? (docs/agents/concurrency.md)")
        })
}

mod agent;
mod analyzers;
mod commands;
mod compute;
mod crop_offer;
mod cull;
mod denoise;
mod edit_head;
mod exemplars;
mod export;
mod framing;
mod golden;
mod jobs;
mod keywords;
mod lens;
mod lens_blur;
mod libraries;
mod library;
mod library_safety;
mod masks;
mod meaning;
mod people;
mod previews;
mod rawcheck;
mod recipe_render;
mod remove;
mod render_fixture;
mod shoots;
mod spots;
mod storage;
mod style;
mod taste;
mod tools;
mod undo_looks;
mod usage;
mod variant;
