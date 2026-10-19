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

# Sample RAWs: copy them from the project's shared folder when it holds them (fixtures/raw*/),
# so sessions don't each pull ~1 GB from Greg's Mac. Copies, not links: Import refuses symlinks.
for shared in /mnt/project-files/fixtures/raw /mnt/project-files/fixtures/raw-a7rv; do
  [ -d "$shared" ] || continue
  dest="fixtures/$(basename "$shared")"
  mkdir -p "$dest"
  for f in "$shared"/*.ARW; do [ -e "$f" ] && [ ! -e "$dest/$(basename "$f")" ] && cp "$f" "$dest/"; done
done

# sccache with its cache in the project's shared folder, so a new container reuses the dependency
# builds earlier sessions made: a cold `cargo test --no-run` measured 416 s without, 130 s with
# (2026-10-18, docs/agents/build-speed.md). Hits need the same target path, which cloud sessions share.
# Set in the container's own ~/.cargo/config.toml, never the repo's, so Greg's Mac is untouched.
if [ -d /mnt/project-files ] && [ ! -x "$HOME/.local/bin/sccache" ]; then
  v=0.14.0
  mkdir -p "$HOME/.local/bin" /mnt/project-files/cache/sccache
  curl -fsSL --max-time 60 "https://github.com/mozilla/sccache/releases/download/v$v/sccache-v$v-x86_64-unknown-linux-musl.tar.gz" \
    | tar -xz -C "$HOME/.local/bin" --strip-components=1 "sccache-v$v-x86_64-unknown-linux-musl/sccache" 2>/dev/null
fi
if [ -x "$HOME/.local/bin/sccache" ] && ! grep -q rustc-wrapper "$HOME/.cargo/config.toml" 2>/dev/null; then
  cat > "$HOME/.local/bin/argus-sccache" <<EOS
#!/bin/sh
export SCCACHE_DIR=/mnt/project-files/cache/sccache SCCACHE_CACHE_SIZE=3G
exec "$HOME/.local/bin/sccache" "\$@"
EOS
  chmod +x "$HOME/.local/bin/argus-sccache"
  printf '[build]\nrustc-wrapper = "%s"\n' "$HOME/.local/bin/argus-sccache" >> "$HOME/.cargo/config.toml"
fi

# Warm the `fast` build in the background (a cold `just check` is 3-6 min of compiling).
# Detached so the session starts at once; a `just check` started meanwhile waits on cargo's
# lock and reuses whatever finished. `just status` shows how far it got (log: out/logs/cloud-warm.log).
mkdir -p target
nohup nice -n 10 scripts/progress cloud-warm -- sh -c 'cargo test --workspace --profile fast --no-run -q && cargo clippy --workspace --all-targets --profile fast -q' \
  > target/cloud-warm.log 2>&1 < /dev/null &

if ! ls fixtures/raw/*.ARW >/dev/null 2>&1; then
  echo "fixtures/raw is empty: golden and RAW tests will skip. Copy fixtures/raw/*.ARW from Greg's Mac checkout with the device folder tools (CLAUDE.md)."
fi
exit 0
