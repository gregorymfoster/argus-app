# Argus task runner. See CLAUDE.md "Commands".

default:
    @just --list

# Long steps run under scripts/progress: heartbeat every 60 s, a stall watchdog, logs and a one-line
# status per step in out/logs/. `just status` shows them all (what agents check, at most every 5 min).
status:
    @scripts/progress status

# What the Rust checks depend on: `scripts/cached` skips clippy and tests when none of it changed since they last passed (docs-only changes, re-runs after a failed push). ARGUS_NO_CACHE=1 forces them.
rust_inputs := "--in Cargo.toml --in Cargo.lock --in engine --in cli --in fixtures --stat fixtures/raw --stat fixtures/raw-a7rv --key 'rustc -vV' --key 'env | grep -e ^ARGUS_ -e ^RUSTFLAGS | sort'"

# Format, lint and test everything (incl. golden images).
# Uses the `fast` profile (release speed, no LTO): a one-file engine edit rebuilds in seconds, not minutes.
check:
    scripts/progress fmt -- cargo fmt --all -- --check
    scripts/cached clippy {{rust_inputs}} -- scripts/progress clippy -- cargo clippy --workspace --all-targets --profile fast -- -D warnings
    scripts/cached test {{rust_inputs}} -- scripts/progress test -- cargo test --workspace --profile fast
    scripts/cached install-sh --in scripts/release/install.sh --in scripts/release/install.test.sh --in scripts/release/stamp-install.sh -- scripts/release/install.test.sh
    @ls fixtures/raw/*.ARW >/dev/null 2>&1 || echo "warning: fixtures/raw is empty, so golden and RAW tests were SKIPPED (see CLAUDE.md for getting the RAWs)"

# Render a RAW to out/<stem>.jpg (JPEG q90 by default) and print timings. e.g. `just render fixtures/raw/DSC03945.ARW --long-edge 2048`
render file *args:
    scripts/progress build-cli -- cargo build --profile fast -p argus-cli
    ./target/fast/argus render {{file}} {{args}}

# Prototype against a real Library: bridge + Vite smoke test with screenshots. e.g. `just live ~/tmp/argus-lib`
live library:
    scripts/progress build-cli -- cargo build --profile fast -p argus-cli
    cd prototype && ../scripts/progress live -- npm run live {{library}}

# Crawl every main screen on a real Library with no model running: fails on page errors, console errors and engine calls that error. Shots in prototype/out/shots/explore-*.png.
explore library:
    scripts/progress build-cli -- cargo build --profile fast -p argus-cli
    cd prototype && ../scripts/progress explore -- node scripts/explore.mjs {{library}}

# Every main screen at Greg's window widths (1280, 1500 px), demo data: fails when a header or footer wraps or clips. Shots in prototype/out/shots/layout-*.png.
layout:
    cd prototype && ../scripts/progress layout -- node scripts/layout.mjs

# Import from a fake card of the sample RAWs against the real engine (mock local model): source and New/All menus, time and space, the grid filling while it imports, and the width check on the real Library.
import-check:
    scripts/progress build-cli -- cargo build --profile fast -p argus-cli
    cd prototype && ../scripts/progress import-check -- node scripts/import-check.mjs

# How light the open app is (Linux/cloud only): startup, bundle, idle engine calls, React renders and CPU (visible and hidden), and memory after scrolling a 1,000-Photo grid. Writes prototype/out/perf/<label>.json; compare two with `node prototype/scripts/perf.mjs --diff a.json b.json`. See docs/agents/performance.md.
perf *args:
    scripts/progress build-cli -- cargo build --profile fast -p argus-cli
    cd prototype && npm run build >/dev/null && ../scripts/progress perf -- node scripts/perf.mjs {{args}}

# Time decode, Import, previews and Edit re-renders on the sample RAWs and compare with bench/speed-baseline.json (fails on a >30% slowdown). `just speed --save` records this machine's baseline.
speed *args:
    scripts/progress build-cli -- cargo build --profile fast -p argus-cli
    scripts/progress speed -- ./target/fast/argus speed fixtures/raw fixtures/raw-a7rv {{args}}

# Re-accept golden references after a deliberate pipeline change. View the PNGs before committing!
bless:
    ARGUS_BLESS=1 scripts/progress bless -- cargo test --profile fast -p argus-engine --test engine golden::
    git status --short fixtures/golden

# Gut-check Argus against Greg's past Lightroom picks and edits (fixtures/bench). Runs the AI cull: heavy, prefer overnight. e.g. `just bench --shoot 2024-10-19`, `just bench --no-model`
bench *args:
    scripts/progress build-cli -- cargo build --profile fast -p argus-cli
    scripts/progress bench --stall 3600 -- python3 bench/score.py {{args}}

# Before/after report of an edit bench run: `just bench-report add out/bench/<time>.json` (bench-report skill)
bench-report *args:
    python3 bench/report.py {{args}}

# Run the desktop app in dev mode (hot-reloading UI). Library: $ARGUS_LIBRARY or ~/Pictures/Argus/library.
app:
    cd app && npm install --no-audit --no-fund && npx tauri dev

# Drive the real desktop app end to end over WebDriver (Linux + Xvfb; needs tauri-driver and webkit2gtk-driver). Screens land in out/shots/app-*.png.
app-e2e:
    cd app && ../scripts/progress app-e2e --stall 1200 -- sh -c 'npm install --no-audit --no-fund && npm run e2e'

# The first part of app-e2e only (Import from a card, grid previews, one flag written to the Library), for iterating; run the full app-e2e before landing.
app-e2e-quick:
    cd app && ../scripts/progress app-e2e-quick --stall 1200 -- sh -c 'npm install --no-audit --no-fund && npm run e2e:quick'

# Install what app-e2e needs in a fresh Linux cloud container (WebKit, Xvfb, tauri-driver) and warm the app build. About 6 minutes.
app-e2e-setup:
    # tauri-driver: the cargo-quickinstall prebuilt (what cargo-binstall uses) takes seconds; compiling it takes 1-2 min.
    command -v tauri-driver >/dev/null || (mkdir -p ~/.cargo/bin && curl -fsSL --max-time 60 https://github.com/cargo-bins/cargo-quickinstall/releases/download/tauri-driver-2.1.0/tauri-driver-2.1.0-x86_64-unknown-linux-gnu.tar.gz | tar -xz -C ~/.cargo/bin tauri-driver) || scripts/progress install-tauri-driver -- cargo install tauri-driver --locked
    command -v WebKitWebDriver >/dev/null || scripts/progress install-webkit -- sh -c 'apt-get update -q && apt-get install -y -q libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev xvfb webkit2gtk-driver'
    # Warm the app's debug build: cold it's minutes, warm a rebuild is ~15 s, the checks ~30 s.
    cd app && ../scripts/progress app-build-debug --stall 1800 -- sh -c 'npm install --no-audit --no-fund && npx tauri build --debug --no-bundle'

# Build Argus on this Mac and publish it as an auto-updating release (free; see docs/release.md). e.g. `just release-local 0.2.0`; `--build-only` stops before publishing, `--publish-only` then ships that build
release-local version *mode:
    scripts/progress release --stall 3600 -- scripts/release/local.sh {{version}} {{mode}}

# The Mac check, with no screen control: real engine + the app's UI headless in WebKit (Import from a fake RAW+JPEG card, Esc, ⌘, Settings › Library, grid, flags, AI cull, Agent edit, Undo). Background priority. MAC_E2E_LIBRARY=<dir> also opens that Library read-only. e.g. `just mac-e2e`
mac-e2e *raws:
    scripts/progress build-cli -- {{ if os() == "macos" { "taskpolicy -b nice -n 20" } else { "" } }} cargo build --profile fast -p argus-cli -j 2
    cd prototype && npm install --no-audit --no-fund --silent {{ if os() == "macos" { "&& npx playwright install webkit" } else { "" } }}
    cd prototype && ../scripts/progress mac-e2e -- ../scripts/timeout 900 node scripts/mac-e2e.mjs {{raws}}

# Rebase onto origin/main, run `just check`, push to main (retries; works from any branch). LAND_SKIP_CHECK=1 for docs-only changes.
land:
    scripts/land
