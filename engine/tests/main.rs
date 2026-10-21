//! All engine integration tests, in one binary (`--test engine`): one link instead of one per
//! file, which made `just check` after an engine edit mostly linking. Run one file's tests with
//! `cargo test --profile fast -p argus-engine --test engine golden::` (the module is the old file).
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

mod agent;
mod analyzers;
mod commands;
mod compute;
mod cull;
mod denoise;
mod exemplars;
mod export;
mod golden;
mod jobs;
mod lens;
mod libraries;
mod library;
mod library_safety;
mod masks;
mod previews;
mod rawcheck;
mod recipe_render;
mod remove;
mod render_fixture;
mod shoots;
mod spots;
mod storage;
mod style;
mod tools;
mod undo_looks;
mod usage;
mod variant;
