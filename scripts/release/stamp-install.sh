#!/usr/bin/env bash
# Prints install.sh stamped for one release: its version, .dmg name and checksum, and the
# default model read from the engine source at that release's commit.
# Usage: scripts/release/stamp-install.sh VERSION path/to/Argus_X_aarch64.dmg [SHA]
set -euo pipefail
version="$1" dmg="$2" sha="${3:-HEAD}"
here=$(cd "$(dirname "$0")" && pwd)
model=$(git -C "$here" show "$sha:engine/src/providers/ollama.rs" | sed -n 's/^pub const DEFAULT_OLLAMA_MODEL: &str = "\(.*\)";/\1/p')
[ -n "$model" ] || { echo "stamp-install: no DEFAULT_OLLAMA_MODEL in engine/src/providers/ollama.rs at $sha" >&2; exit 1; }
if command -v shasum >/dev/null; then sum=$(shasum -a 256 "$dmg"); else sum=$(sha256sum "$dmg"); fi
out=$(sed -e "s/@@VERSION@@/$version/" -e "s/@@DMG_NAME@@/$(basename "$dmg")/" \
  -e "s/@@DMG_SHA256@@/${sum%% *}/" -e "s/@@MODEL@@/$model/" "$here/install.sh")
if grep -Eq '@@[A-Z0-9_]+@@' <<<"$out"; then echo "stamp-install: unstamped @@ values left" >&2; exit 1; fi
bash -n <<<"$out"
printf '%s\n' "$out"
