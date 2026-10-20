#!/bin/bash
# One-command install of Argus for friends: the app, Ollama and the default local model.
#
#   curl -fsSL https://github.com/gregorymfoster/argus-releases/releases/latest/download/install.sh | bash
#
# Flags go after `bash -s --`, e.g. `... | bash -s -- --yes --no-model`:
#   --yes          don't ask before the big model download
#   --dry-run      print what would happen and change nothing
#   --no-model     install the app (and Ollama) but pull no model
#   --no-ollama    install only the app (for people using their Claude plan)
#   --model NAME   pull NAME instead of the default model
#   --no-open      don't open Argus at the end
#
# scripts/release/local.sh (through stamp-install.sh) fills in the placeholders below with the release it ships with
# (version, .dmg name and checksum, default model) and attaches the result to the release.
# Safe to run again: it skips whatever is already in place. Never uses sudo.
# Written for macOS's own /bin/bash 3.2, so no bash 4 features.
set -euo pipefail

ARGUS_VERSION="${ARGUS_VERSION:-@@VERSION@@}"
DMG_NAME="${ARGUS_DMG_NAME:-@@DMG_NAME@@}"
DMG_SHA256="${ARGUS_DMG_SHA256:-@@DMG_SHA256@@}"
DEFAULT_MODEL="@@MODEL@@"
RELEASE_REPO=gregorymfoster/argus-releases
DMG_URL="${ARGUS_DMG_URL:-https://github.com/$RELEASE_REPO/releases/download/v$ARGUS_VERSION/$DMG_NAME}"
OLLAMA_URL="${ARGUS_OLLAMA_URL:-https://ollama.com/download/Ollama.dmg}"
APPS="${ARGUS_APPS_DIR:-/Applications}"
MIN_MACOS=13
MODEL_RAM_GB=32     # the default model wants about this much memory (README "What you need")
MODEL_DISK_GB=20    # the default model is about 17 GB at Q4, plus room to unpack

YES=0 DRY=0 PULL_MODEL=1 WANT_OLLAMA=1 OPEN=1 MODEL="" MODEL_EXPLICIT=0
DONE=()
WORK=""

say() { printf '%s\n' "$*"; }
step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
did() { DONE+=("$*"); }
# Runs a command, or only prints it under --dry-run.
run() { if [ "$DRY" = 1 ]; then say "  would run: $*"; else "$@"; fi; }
cleanup() {
  if [ -n "$WORK" ]; then
    for m in "$WORK"/mnt-*; do [ -d "$m" ] && hdiutil detach -quiet "$m" >/dev/null 2>&1 || true; done
    rm -rf "$WORK"
  fi
}

usage() {
  cat <<'EOF_USAGE'
Installs Argus, Ollama and the default local model. Options:
  --yes          don't ask before the big model download
  --dry-run      print what would happen and change nothing
  --no-model     install the app (and Ollama) but pull no model
  --no-ollama    install only the app (for people using their Claude plan)
  --model NAME   pull NAME instead of the default model
  --no-open      don't open Argus at the end
EOF_USAGE
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      -y|--yes) YES=1 ;;
      -n|--dry-run) DRY=1 ;;
      --no-model) PULL_MODEL=0 ;;
      --no-ollama) WANT_OLLAMA=0; PULL_MODEL=0 ;;
      --model) [ $# -ge 2 ] || die "--model needs a name"; MODEL="$2"; MODEL_EXPLICIT=1; shift ;;
      --model=*) MODEL="${1#--model=}"; MODEL_EXPLICIT=1 ;;
      --no-open) OPEN=0 ;;
      -h|--help) usage; exit 0 ;;
      *) die "unknown option $1 (try --help)" ;;
    esac
    shift
  done
  if [ -z "$MODEL" ]; then MODEL="${ARGUS_OLLAMA_MODEL:-$DEFAULT_MODEL}"; fi
}

# Asks a yes/no question on the terminal even when the script itself arrives on stdin.
confirm() {
  [ "$YES" = 1 ] && return 0
  if [ "$DRY" = 1 ]; then say "  would ask: $1 [Y/n]"; return 0; fi
  local answer
  if ! { exec 3</dev/tty; } 2>/dev/null; then
    die "no terminal to ask on; run again with --yes (… | bash -s -- --yes)"
  fi
  printf '%s [Y/n] ' "$1"
  read -r answer <&3 || answer=n
  exec 3<&-
  case "$answer" in ""|y|Y|yes|Yes) return 0 ;; *) return 1 ;; esac
}

stamped() { case "$1" in *@"@"*) return 1 ;; *) return 0 ;; esac; }

gb() { echo $(( $1 / 1024 / 1024 / 1024 )); }

check_mac() {
  step "Checking this Mac"
  [ "$(uname -s)" = Darwin ] || die "Argus runs on macOS only"
  [ "$(uname -m)" = arm64 ] || die "Argus needs a Mac with Apple Silicon (M1 or later); this one is $(uname -m)"
  local os major
  os=$(sw_vers -productVersion)
  major=${os%%.*}
  [ "$major" -ge "$MIN_MACOS" ] || die "Argus needs macOS $MIN_MACOS or later; this Mac has $os"
  MEM_GB=$(gb "$(sysctl -n hw.memsize)")
  # df -k on the volume that will hold the model (~/.ollama, on the home volume).
  FREE_GB=$(( $(df -k "$HOME" | awk 'NR==2 {print $4}') / 1024 / 1024 ))
  say "macOS $os on Apple Silicon, $MEM_GB GB memory, $FREE_GB GB free"
  stamped "$ARGUS_VERSION$DMG_NAME$DMG_SHA256" || [ "$DRY" = 1 ] \
    || die "this copy of install.sh isn't from a release; use the one at https://github.com/$RELEASE_REPO/releases/latest"
}

app_version() { # the version of an installed Argus.app, or nothing
  plutil -extract CFBundleShortVersionString raw -o - "$1/Contents/Info.plist" 2>/dev/null || true
}

ollama_bin() { # the ollama command, from PATH or inside Ollama.app
  if command -v ollama >/dev/null 2>&1; then command -v ollama; return; fi
  local app
  for app in "$APPS/Ollama.app" "$HOME/Applications/Ollama.app"; do
    if [ -x "$app/Contents/Resources/ollama" ]; then echo "$app/Contents/Resources/ollama"; return; fi
  done
}

have_model() { "$1" list 2>/dev/null | awk 'NR>1 {print $1}' | grep -qx "$2"; }

plan() {
  step "Plan"
  local installed
  installed=$(app_version "$APPS/Argus.app")
  if [ "$installed" = "$ARGUS_VERSION" ]; then
    say "- Argus $ARGUS_VERSION is already in $APPS"
  elif [ -n "$installed" ]; then
    say "- Replace Argus $installed in $APPS with $ARGUS_VERSION"
  else
    say "- Install Argus $ARGUS_VERSION into $APPS (about 30 MB)"
  fi
  if [ "$WANT_OLLAMA" = 1 ]; then
    OLLAMA=$(ollama_bin)
    if [ -n "$OLLAMA" ]; then say "- Ollama is already installed"; else say "- Install Ollama, the free local model runner, into $APPS"; fi
  fi
  if [ "$PULL_MODEL" = 1 ] && [ "$MODEL_EXPLICIT" = 0 ] && [ "$MEM_GB" -lt "$MODEL_RAM_GB" ]; then
    PULL_MODEL=0
    say "- Skip the default model: $MODEL wants about $MODEL_RAM_GB GB of memory and this Mac has $MEM_GB GB."
    say "  Pick a smaller vision model with --model NAME, or use your Claude plan (Settings > AI)."
  fi
  if [ "$PULL_MODEL" = 1 ]; then
    if [ -n "${OLLAMA:-}" ] && have_model "$OLLAMA" "$MODEL"; then
      say "- The model $MODEL is already downloaded"
      PULL_MODEL=0
    else
      say "- Download the AI model $MODEL (about 17 GB for the default; it stays on this Mac)"
      if [ "$MODEL_EXPLICIT" = 0 ] && [ "$FREE_GB" -lt "$MODEL_DISK_GB" ]; then
        die "only $FREE_GB GB free and the model needs about $MODEL_DISK_GB GB; free some space, or run again with --no-model"
      fi
    fi
  fi
  [ "$OPEN" = 1 ] && say "- Open Argus"
  say ""
  say "This never asks for your password, and your photos stay on this Mac."
  if [ "$PULL_MODEL" = 1 ]; then confirm "Go ahead?" || die "stopped; nothing was changed"; fi
}

# Downloads a URL to a file with a progress bar.
fetch() { curl -fL --retry 3 --progress-bar -o "$2" "$1"; }

# Copies the first *.app on a mounted .dmg into $APPS, replacing an older copy safely.
install_app_from_dmg() {
  local dmg="$1" name="$2" mnt="$WORK/mnt-$2" src
  hdiutil attach -nobrowse -readonly -quiet -mountpoint "$mnt" "$dmg"
  src="$mnt/$name.app"
  [ -d "$src" ] || die "$name.app isn't on the downloaded disk image"
  if [ -e "$APPS/$name.app" ]; then
    mv "$APPS/$name.app" "$WORK/$name.old.app"
  fi
  if ! ditto "$src" "$APPS/$name.app"; then
    [ -e "$WORK/$name.old.app" ] && mv "$WORK/$name.old.app" "$APPS/$name.app"
    die "couldn't copy $name.app into $APPS"
  fi
  hdiutil detach -quiet "$mnt"
}

install_argus() {
  step "Argus"
  local installed
  installed=$(app_version "$APPS/Argus.app")
  if [ "$installed" = "$ARGUS_VERSION" ]; then say "Argus $ARGUS_VERSION is already installed"; return; fi
  if [ "$DRY" = 1 ]; then
    say "  would download $DMG_URL"
    say "  would check its SHA-256 is $DMG_SHA256"
    say "  would copy Argus.app into $APPS and clear its quarantine flag"
    return
  fi
  if pgrep -xq Argus; then
    say "Quitting the running Argus first"
    osascript -e 'quit app "Argus"' >/dev/null 2>&1 || true
    sleep 2
  fi
  local dmg="$WORK/$DMG_NAME" got
  fetch "$DMG_URL" "$dmg"
  got=$(shasum -a 256 "$dmg" | awk '{print $1}')
  [ "$got" = "$DMG_SHA256" ] || die "the download's checksum doesn't match ($got, expected $DMG_SHA256); nothing was installed"
  install_app_from_dmg "$dmg" Argus
  # Argus isn't notarized, so Gatekeeper would block its first launch. This clears the
  # quarantine flag on Argus.app only; nothing else on the Mac changes.
  xattr -dr com.apple.quarantine "$APPS/Argus.app" 2>/dev/null || true
  did "Installed Argus $ARGUS_VERSION in $APPS (checksum verified)"
}

install_ollama() {
  [ "$WANT_OLLAMA" = 1 ] || return 0
  step "Ollama"
  OLLAMA=$(ollama_bin)
  if [ -z "$OLLAMA" ]; then
    if [ "$DRY" = 1 ]; then
      say "  would download $OLLAMA_URL and copy Ollama.app into $APPS"
      OLLAMA="$APPS/Ollama.app/Contents/Resources/ollama"
    else
      fetch "$OLLAMA_URL" "$WORK/Ollama.dmg"
      install_app_from_dmg "$WORK/Ollama.dmg" Ollama
      OLLAMA="$APPS/Ollama.app/Contents/Resources/ollama"
      did "Installed Ollama in $APPS"
    fi
  else
    say "Using $OLLAMA ($("$OLLAMA" --version 2>/dev/null | tail -n1 || echo "version unknown"))"
  fi
  # The menu bar app runs the model server and starts itself at login from now on.
  if [ "$DRY" = 1 ]; then say "  would start Ollama and wait for it"; return; fi
  if ! "$OLLAMA" list >/dev/null 2>&1; then
    open -g -a Ollama 2>/dev/null || open -g "$APPS/Ollama.app" 2>/dev/null || true
    local i
    for i in $(seq 1 60); do
      "$OLLAMA" list >/dev/null 2>&1 && break
      [ "$i" = 60 ] && die "Ollama didn't start within a minute; open Ollama from Applications and run this again"
      sleep 1
    done
  fi
  say "Ollama is running"
}

pull_model() {
  [ "$PULL_MODEL" = 1 ] || return 0
  step "AI model $MODEL"
  if [ "$DRY" = 1 ]; then say "  would run: ollama pull $MODEL"; return; fi
  if have_model "$OLLAMA" "$MODEL"; then say "Already downloaded"; return; fi
  say "This is a big download. If it stops, run the same command again and it picks up where it left off."
  "$OLLAMA" pull "$MODEL" || die "couldn't download $MODEL. If Ollama says it needs a newer version, update Ollama from its menu bar icon and run this again"
  did "Downloaded the AI model $MODEL"
}

finish() {
  step "Done"
  if [ "$DRY" = 1 ]; then say "Dry run: nothing was changed."; return; fi
  if [ ${#DONE[@]} -eq 0 ]; then say "Everything was already in place."; else
    local d; for d in "${DONE[@]}"; do say "- $d"; done
  fi
  if [ "$MODEL_EXPLICIT" = 1 ] && [ "$MODEL" != "$DEFAULT_MODEL" ]; then
    say "- Choose $MODEL in Argus under Settings (⌘,) > AI, since it isn't the default"
  fi
  say ""
  say "Argus updates itself from now on. On first launch it asks where to keep your Library."
  say "Using your Claude plan instead of a local model: install Claude Code, run \`claude\` once to sign in,"
  say "then pick \"Your Claude plan\" in Settings > AI."
  if [ "$OPEN" = 1 ]; then open "$APPS/Argus.app" 2>/dev/null || warn "couldn't open Argus; open it from Applications"; fi
}

main() {
  parse_args "$@"
  say "Argus $ARGUS_VERSION installer$([ "$DRY" = 1 ] && echo " (dry run)")"
  check_mac
  if [ ! -w "$APPS" ] && [ "$DRY" = 0 ]; then
    APPS="$HOME/Applications"
    mkdir -p "$APPS"
    warn "/Applications isn't writable for this account, so this installs into $APPS"
  fi
  plan
  WORK=$(mktemp -d "${TMPDIR:-/tmp}/argus-install.XXXXXX")
  trap cleanup EXIT
  install_argus
  install_ollama
  pull_model
  finish
}

# Everything runs from here, so a download cut off halfway through runs nothing.
main "$@"
