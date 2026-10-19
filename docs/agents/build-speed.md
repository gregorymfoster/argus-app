# Build and test speed

Greg (2026-10-18): always keep caching and build-graph ideas in mind. Add an idea here with an estimated saving when you spot one; move it to "Done" when it lands.

## Health

Every long step runs under `scripts/progress` (see its header). `just status` prints one line per step: state, elapsed, done/total, ETA and the last output line; a run whose wrapper stopped heart-beating shows as `DEAD?`. Logs are `out/logs/NAME.log`, timings of the last good run `out/logs/durations.json` (they drive the ETA when a script prints no `::progress` lines). Default stall limit is 15 min without output; bench gets 60, release 30, app-e2e 20.

## How caching works here

Three layers, each doing one job. Cargo is the per-crate build DAG: it rebuilds only crates whose inputs changed, as long as the target folder survives (Greg's checkout keeps `target/` and `app/target/` between runs). sccache sits under it in cloud sessions, where every container starts with an empty target folder. `scripts/cached` sits above it: a content-hash cache for whole steps (test runs, clippy, the frontend build, the release's `npm ci`). It hashes the step's inputs (git-visible files under `--in`, names and sizes under `--stat`, the output of `--key` commands such as `rustc -vV`) and skips the step when the hash matches its last passing run. `just check` with nothing Rust-side changed takes 0.9 s instead of ~15 s, so docs-only lands and re-runs after a failed push are nearly free. `ARGUS_NO_CACHE=1` forces everything.

Why not moon, Turborepo, Nx or Bazel (2026-10-18): the repo is two Rust crates plus the app and one Vite package, and cargo already does per-crate build caching better than a generic runner could. What a task runner adds on top is step-level skip-if-unchanged, which `scripts/cached` gives in one stdlib Python file that runs the same on the Mac and in the cloud with nothing to install. Test results can't be cached finer than per test binary without Bazel-style per-test dependency tracking, which no light tool offers for Rust. Revisit if the repo grows more packages or a shared remote cache between machines starts to matter.

## Ideas, biggest first

- **mac-e2e builds at background QoS with `-j 2`** (CLAUDE.md rule), so on Greg's Mac it runs on efficiency cores only. Moving it to `taskpolicy -c utility` like the release would speed it up several times; it's his rule, so ask first.
- **Skip `npm install` in `live`, `mac-e2e` and `app-e2e`** with `scripts/cached` keyed on the lockfile (a few seconds each).
- **sccache on the Mac.** Only helps after `app/target` is wiped or when switching between worktrees; the persistent target folders already cover the usual case.

## Tried, didn't pay

- **App on the workspace's target dir and `fast` profile** (2026-10-18). `CARGO_TARGET_DIR=../target npx tauri build --no-bundle -- --profile fast` with a matching `[profile.fast]` in `app/Cargo.toml` took 7:30 cold against 3:36 for today's debug build, and reused nothing: the app's dependency features differ, so cargo hashes rawler, image and the engine differently (the root target already holds three rawler builds: engine alone, workspace tests, cli). Optimised code also compiles slower. Not worth it unless the app joins the root workspace with unified features.

## Done

- **Release builds** (2026-10-18). Measured on the 4-core cloud container, `cargo build --release -p argus-cli`, no sccache: thin LTO took 463 s cold and 182 s after an engine change; no LTO with 16 codegen units 330 s and 65 s; the `fast` settings (no LTO, 256 codegen units, incremental) 6 s after an engine change. `just speed`, two runs each, `fast` against thin LTO: decode 83-118 ms vs 90-142, Edit slider 41-42 vs 52, previews 197-200 vs 222-224 ms/photo, Import the same. So `[profile.release]` in both `Cargo.toml` and `app/Cargo.toml` now uses the `fast` settings. (Both were already above the cloud baseline's 27 ms slider before this change.)
- **Release priority** (2026-10-18). `taskpolicy -b` (background QoS) pins a process to the efficiency cores on Apple Silicon (4 of the M4 Max's 16), which made most of the ~25 min. `ARGUS_BUILD_PRIORITY` in `scripts/release/local.sh`: `gentle` (default) is utility QoS plus `nice -n 10`, which uses the performance cores and still yields to Greg's apps; `full` drops all limits; `background` is the old mode. Not yet timed on the Mac.
- `scripts/cached` skips the prototype's Vite build in `tauri build` (release and app-e2e) when `prototype/` is unchanged, and the release's `npm ci` when the app's lockfile is unchanged.
- `fast` Cargo profile for tests and throwaway binaries (one-file edit rebuild ~5 s vs 5 min on `--release`).
- `cloud-setup.sh` warms the `fast` build and clippy in the background at session start, so the first `just check` mostly reuses it.
- `app-e2e-quick` for iterating on the first part of the e2e.
- `app-e2e-setup` downloads the cargo-quickinstall prebuilt tauri-driver (seconds) and only compiles it if the download fails (was 1-2 min).
- `cargo build -p argus-cli` (render, speed, bench, live, mac-e2e) reuses what `just check` built: the cli's normal deps carry the same tokio and rmcp features as its tests. Before, the feature mismatch re-hashed libc, tokio and ~15 crates above them, a 3:55 rebuild after a cold `just check` (2026-10-18).
- Clippy runs on `--profile fast`, so it shares build scripts and proc-macros with the test build: cold clippy 0:40 instead of 2:07 (2026-10-18).
- Cloud sessions use sccache with its cache in `/mnt/project-files/cache/sccache` (3 GB cap), set up by `cloud-setup.sh` in the container's own `~/.cargo/config.toml`. Measured 2026-10-18 on a fresh target dir: cold `cargo test --workspace --profile fast --no-run` 416 s without it, 466 s while filling the cache, 130 s with a warm cache (100% hits; the remaining time is the incremental workspace crates and linking). Hits need the same target path, so a test in another directory misses everything. The cache is ~500 MB of small files on a network mount: deleting it took over 10 minutes, so don't clear it casually. Greg's Mac doesn't use it.
- Engine integration tests are one binary (`engine/tests/main.rs`, `--test engine`, each old file a module, so `golden::` filters one file). `just check` measured 2026-10-18 on the cloud container, before and after, one run each: no-op 20 s to 12 s, after a one-file engine edit 32 s to 19 s; same 153 tests. Tests now share a process: anything setting an env var takes `crate::env_lock()`, and a test re-running its own binary names itself with the module path (`library_safety::kill_child_writer`).
