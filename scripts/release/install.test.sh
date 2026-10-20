#!/usr/bin/env bash
# Tests install.sh anywhere (the cloud container included) by putting fake macOS commands
# (uname, sw_vers, sysctl, hdiutil, ollama, ...) first on PATH. Each fake logs its call.
# Run: scripts/release/install.test.sh
# shellcheck disable=SC2016  # the fakes' bodies are meant to expand when they run, not here
set -euo pipefail
# The fakes stand in for macOS, and setsid takes the terminal away; both need Linux.
if ! command -v setsid >/dev/null || ! command -v sha256sum >/dev/null; then
  echo "install.sh tests skipped: they need Linux (setsid, sha256sum); the cloud check runs them"; exit 0
fi
here=$(cd "$(dirname "$0")" && pwd)
script="$here/install.sh"
fails=0
base=$(mktemp -d)
trap 'rm -rf "$base"' EXIT

setup() {
  T=$(mktemp -d "$base/t.XXXXXX")
  mkdir -p "$T/bin" "$T/Applications" "$T/home" "$T/tmp"
  export HOME="$T/home" TMPDIR="$T/tmp" ARGUS_APPS_DIR="$T/Applications" LOG="$T/log" STATE="$T"
  export FAKE_ARCH=arm64 FAKE_OS=15.1 FAKE_MEM_GB=64 FAKE_FREE_GB=200
  : > "$LOG"
  fake() { printf '#!/usr/bin/env bash\necho "%s $*" >> "$LOG"\n%s\n' "$1" "$2" > "$T/bin/$1"; chmod +x "$T/bin/$1"; }
  fake uname 'case "$1" in -s) echo Darwin;; -m) echo "$FAKE_ARCH";; esac'
  fake sw_vers 'echo "$FAKE_OS"'
  fake sysctl 'echo $(( FAKE_MEM_GB * 1024 * 1024 * 1024 ))'
  fake df 'echo "Filesystem 1K-blocks Used Available"; echo "/dev/disk3 1 1 $(( FAKE_FREE_GB * 1024 * 1024 ))"'
  # A fake .dmg is a text file holding the app's version; mounting it makes NAME.app.
  fake hdiutil 'if [ "$1" = attach ]; then mnt="${@: -2:1}"; dmg="${@: -1}"; n=Argus; case "$dmg" in *Ollama*) n=Ollama;; esac
    mkdir -p "$mnt/$n.app/Contents/Resources"; cp "$dmg" "$mnt/$n.app/Contents/Info.plist"
    if [ $n = Ollama ]; then cp "$FAKE_OLLAMA" "$mnt/$n.app/Contents/Resources/ollama"; fi; fi; true'
  fake ditto 'cp -R "$1" "$2"'
  fake plutil 'cat "${@: -1}"'
  fake xattr 'true'
  fake open 'touch "$STATE/ollama-up"'
  fake pgrep 'false'
  fake osascript 'true'
  fake shasum 'sha256sum "${@: -1}"'
  # Ollama starts out not installed, so its fake lives outside PATH until something installs it.
  fake ollama 'case "$1" in
    list) [ -f "$STATE/ollama-up" ] || exit 1; echo "NAME ID SIZE"; if [ -f "$STATE/model" ]; then cat "$STATE/model"; fi;;
    pull) echo "$2 x 17GB" > "$STATE/model";; --version) echo "ollama version 0.40.6";; esac'
  export FAKE_OLLAMA="$T/ollama"
  mv "$T/bin/ollama" "$FAKE_OLLAMA"
  export PATH="$T/bin:$PATH"
  echo "0.1.4" > "$T/Argus_0.1.4_aarch64.dmg"
  echo "0.40.6" > "$T/Ollama.dmg"
  export ARGUS_VERSION=0.1.4 ARGUS_DMG_NAME=Argus_0.1.4_aarch64.dmg ARGUS_OLLAMA_MODEL=qwen3.8:27b
  export ARGUS_DMG_URL="file://$T/Argus_0.1.4_aarch64.dmg" ARGUS_OLLAMA_URL="file://$T/Ollama.dmg"
  ARGUS_DMG_SHA256=$(sha256sum "$T/Argus_0.1.4_aarch64.dmg" | cut -d' ' -f1); export ARGUS_DMG_SHA256
}
use_ollama_on_path() { cp "$FAKE_OLLAMA" "$T/bin/ollama"; touch "$STATE/ollama-up"; }

check() { # name, then a command that must succeed
  local name="$1"; shift
  if "$@"; then echo "ok   $name"; else echo "FAIL $name"; fails=$((fails + 1)); fi
}
run() { setsid bash "$script" "$@" > "$T/out" 2>&1 < /dev/null; }
has() { grep -q -- "$1" "$T/out"; }

setup
check "dry run changes nothing" run --dry-run
check "dry run says it's a dry run" has "nothing was changed"
check "dry run plans the model" has "Download the AI model qwen3.8:27b"
check "dry run left Applications empty" test -z "$(ls "$T/Applications")"
check "dry run works piped into bash, as curl | bash does" \
  sh -c "setsid bash -s -- --dry-run --no-open < '$script' > '$T/out' 2>&1"
check "unstamped copy dry-runs" sh -c "ARGUS_VERSION= ARGUS_DMG_NAME= ARGUS_DMG_SHA256= setsid bash '$script' --dry-run > '$T/out' 2>&1 < /dev/null"

check "no terminal and no --yes stops before changing anything" sh -c "! setsid bash '$script' > '$T/out' 2>&1 < /dev/null"
check "  and says to pass --yes" has "run again with --yes"
check "  and installed nothing" test -z "$(ls "$T/Applications")"

check "full install" run --yes --no-open
check "  installed Argus" test -f "$T/Applications/Argus.app/Contents/Info.plist"
check "  installed Ollama" test -x "$T/Applications/Ollama.app/Contents/Resources/ollama"
check "  pulled the model" grep -q "qwen3.8:27b" "$STATE/model"
check "  cleared quarantine on Argus.app only" grep -q "xattr -dr com.apple.quarantine $T/Applications/Argus.app" "$LOG"
check "  never used sudo" sh -c "! grep -q sudo '$LOG'"
check "  reports what it did" has "Installed Argus 0.1.4"
check "  cleaned its temp folder" test -z "$(ls "$T/tmp")"

check "second run" run --yes --no-open
check "  has nothing to do" has "Everything was already in place"

check "newer release replaces the app" sh -c "echo 0.1.5 > '$T/Argus_0.1.4_aarch64.dmg' && ARGUS_VERSION=0.1.5 ARGUS_DMG_SHA256=\$(sha256sum '$T/Argus_0.1.4_aarch64.dmg' | cut -d' ' -f1) setsid bash '$script' --yes --no-open > '$T/out' 2>&1 < /dev/null"
check "  plans the replace" has "Replace Argus 0.1.4"
check "  installed 0.1.5" grep -qx 0.1.5 "$T/Applications/Argus.app/Contents/Info.plist"

setup
check "bad checksum fails" sh -c "! ARGUS_DMG_SHA256=deadbeef setsid bash '$script' --yes --no-open > '$T/out' 2>&1 < /dev/null"
check "  says why" has "checksum doesn't match"
check "  installed nothing" test -z "$(ls "$T/Applications")"

setup; use_ollama_on_path
check "existing Ollama is reused" run --yes --no-open
check "  doesn't install Ollama again" test ! -e "$T/Applications/Ollama.app"
check "  pulled the missing model" grep -q "ollama pull qwen3.8:27b" "$LOG"
: > "$LOG"
check "with the model there too" run --yes --no-open
check "  doesn't pull it again" sh -c "! grep -q 'ollama pull' '$LOG'"
check "  says it's already downloaded" has "already downloaded"

setup; export FAKE_MEM_GB=16
check "16 GB Mac skips the default model" run --yes --no-open
check "  explains why" has "this Mac has 16 GB"
check "  but --model pulls a smaller one" sh -c "setsid bash '$script' --yes --no-open --model gemma3:4b > '$T/out' 2>&1 < /dev/null && grep -q gemma3:4b '$STATE/model'"
check "  and says to choose it in Settings" has "Choose gemma3:4b in Argus"

setup; export FAKE_FREE_GB=10
check "too little disk for the model stops" sh -c "! setsid bash '$script' --yes --no-open > '$T/out' 2>&1 < /dev/null"
check "  says how much is needed" has "needs about 20 GB"

setup; export FAKE_ARCH=x86_64
check "Intel Mac is refused" sh -c "! setsid bash '$script' --yes > '$T/out' 2>&1 < /dev/null"
check "  says Apple Silicon" has "Apple Silicon"

setup; export FAKE_OS=12.7
check "macOS 12 is refused" sh -c "! setsid bash '$script' --yes > '$T/out' 2>&1 < /dev/null"

setup
check "--no-ollama installs only the app" run --yes --no-open --no-ollama
check "  no Ollama" test ! -e "$T/Applications/Ollama.app"

setup
check "stamping a release copy" sh -c "'$here/stamp-install.sh' 0.1.4 '$T/Argus_0.1.4_aarch64.dmg' > '$T/stamped.sh'"
check "  fills every value" sh -c "! grep -Eq '@@[A-Z_]+@@' '$T/stamped.sh' && grep -q 'DEFAULT_MODEL=\"qwen' '$T/stamped.sh'"
check "  points at the release's .dmg" grep -q "releases/download/v\$ARGUS_VERSION/\$DMG_NAME" "$T/stamped.sh"
check "  installs with no overrides but the download URL" sh -c "unset ARGUS_VERSION ARGUS_DMG_NAME ARGUS_DMG_SHA256 ARGUS_OLLAMA_MODEL; setsid bash '$T/stamped.sh' --yes --no-open > '$T/out' 2>&1 < /dev/null"
check "  and pulled the engine's default model" grep -q "qwen3.8:27b" "$STATE/model"

if [ "$fails" != 0 ]; then echo "$fails failed"; exit 1; fi
echo "all install.sh tests passed"
