#!/usr/bin/env bash
# SessionStart hook for Claude Code cloud sessions (.claude/settings.json). Makes `just`
# available and warms the cargo registry so the first `just check` only compiles.
# Does nothing on Greg's Mac. Never fails the session: every step is best-effort.
set -u
[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0
cd "$(dirname "$0")/.."

if ! command -v just >/dev/null 2>&1; then
  v=1.40.0
  mkdir -p "$HOME/.local/bin"
  curl -fsSL "https://github.com/casey/just/releases/download/$v/just-$v-x86_64-unknown-linux-musl.tar.gz" \
    | tar -xz -C "$HOME/.local/bin" just 2>/dev/null \
    && ln -sf "$HOME/.local/bin/just" /usr/local/bin/just 2>/dev/null
fi

timeout 300 cargo fetch --locked -q >/dev/null 2>&1 || true

# Start from the latest main. A fresh session often sits on a claude/* branch; when that branch
# holds nothing main lacks and the tree is clean, switch to main so work never starts stale.
if timeout 60 git fetch -q origin main 2>/dev/null && [ -z "$(git status --porcelain --untracked-files=no)" ] \
   && [ -z "$(git rev-list origin/main..HEAD 2>/dev/null)" ]; then
  git checkout -q -B main origin/main 2>/dev/null || true
fi

# Sample RAWs from the project's shared folder and sccache over its shared cache (also run by
# `just check`, since this hook doesn't run in every kind of session).
scripts/cloud-ensure
# The bench RAWs are stored xz-compressed in the shared folder (a third smaller; its 10 GB quota
# filled up, 2026-10-20). Unpack them in the background: ~5 s each, so a session starts at once and
# a bench run started before they're all out uses the camera JPEG for the rest (wait for the
# `xz -dc` job to finish: `pgrep -f bench-raw` prints nothing once it has).
if ls /mnt/project-files/fixtures/bench-raw/*.ARW.xz >/dev/null 2>&1; then
  mkdir -p fixtures/bench-raw target
  nohup nice -n 10 sh -c 'for f in /mnt/project-files/fixtures/bench-raw/*.ARW.xz; do
    out="fixtures/bench-raw/$(basename "$f" .xz)"
    [ -e "$out" ] || { xz -dc "$f" > "$out.part" && mv "$out.part" "$out"; } || rm -f "$out.part"
  done' > target/bench-raw-unpack.log 2>&1 < /dev/null &
fi

# Warm the `fast` build in the background (a cold `just check` is 3-6 min of compiling).
# Detached so the session starts at once; a `just check` started meanwhile waits on cargo's
# lock and reuses whatever finished. `just status` shows how far it got (log: out/logs/cloud-warm.log).
mkdir -p target
nohup nice -n 10 scripts/progress cloud-warm -- sh -c 'cargo test --workspace --profile fast --no-run -q && cargo clippy --workspace --all-targets --profile fast -q' \
  > target/cloud-warm.log 2>&1 < /dev/null &

if ! ls fixtures/raw/*.ARW >/dev/null 2>&1; then
  echo "fixtures/raw is empty and /mnt/project-files/fixtures/raw has none: golden and RAW tests will skip. Copy fixtures/raw/*.ARW from Greg's Mac checkout with the device folder tools (CLAUDE.md)."
fi
exit 0
