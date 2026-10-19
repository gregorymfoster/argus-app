#!/usr/bin/env bash
# Builds and publishes an Argus release from Greg's Mac, free (no GitHub Actions minutes).
# Usage, from the repo root on main: scripts/release/local.sh 0.2.0   (or: just release-local 0.2.0)
# Two halves, so a build can be checked before anyone says go: `--build-only` stops before
# publishing, and `--publish-only` publishes that exact build later (main may have moved on).
# Needs the update-signing key from scripts/release/setup.sh in ~/.tauri and a logged-in gh.
# ARGUS_BUILD_PRIORITY picks how hard the build leans on the Mac (see docs/agents/build-speed.md):
#   gentle (default)  utility QoS + nice: uses the performance cores but yields to whatever Greg is doing
#   background        background QoS: efficiency cores only, the old default, several times slower
#   full              no limits, for when Greg is away or says go
# See docs/release.md.
set -euo pipefail

VERSION="${1:-}"
MODE="${2:-all}"
case "$MODE" in all|--build-only|--publish-only) ;; *) echo "usage: $0 X.Y.Z [--build-only|--publish-only]" >&2; exit 1;; esac
VERSION="${VERSION#v}"
TAG="v$VERSION"
SOURCE_REPO=gregorymfoster/argus
RELEASE_REPO=${RELEASE_REPO:-gregorymfoster/argus-releases}
PLATFORM=darwin-aarch64
KEY="$HOME/.tauri/argus-updater.key"

die() { echo "release: $*" >&2; exit 1; }
step() { echo; echo "== $* ($(date +%H:%M:%S))"; }

echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' || die "usage: $0 X.Y.Z"
[ "$(uname -s)" = Darwin ] || die "builds the Mac app, so run it on the Mac"
cd "$(git rev-parse --show-toplevel)"
[ -f "$KEY" ] && [ -f "$KEY.password" ] && [ -f "$KEY.pub" ] || die "no signing key in ~/.tauri: run scripts/release/setup.sh once"
gh auth status >/dev/null 2>&1 || die "run 'gh auth login' first"

OUT="out/release/$TAG"
if [ "$MODE" = --publish-only ]; then
  # Publish exactly what --build-only built and was checked, even if main has moved on since.
  SHA=$(cat "$OUT/built-from" 2>/dev/null) || die "no build of $TAG; run with --build-only first"
else
step "Checking the source is main as pushed"
git fetch -q origin main
[ -z "$(git status --porcelain --untracked-files=no)" ] || die "uncommitted changes; commit or stash them first"
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || die "HEAD isn't origin/main; check out main and pull first"
SHA=$(git rev-parse HEAD)
fi
if gh release view "$TAG" --repo "$RELEASE_REPO" >/dev/null 2>&1; then die "$TAG is already published"; fi
latest=$(gh release view --repo "$RELEASE_REPO" --json tagName -q .tagName 2>/dev/null || true)
if [ -n "$latest" ]; then
  newest=$(printf '%s\n%s\n' "${latest#v}" "$VERSION" | sort -V | tail -n1)
  [ "$newest" = "$VERSION" ] && [ "${latest#v}" != "$VERSION" ] || die "$TAG isn't newer than $latest; installed copies would ignore it"
fi
name="Argus_${VERSION}_aarch64"

if [ "$MODE" != --publish-only ]; then
case "${ARGUS_BUILD_PRIORITY:-gentle}" in
  gentle) PRIO=(taskpolicy -c utility nice -n 10) ;;
  background) PRIO=(taskpolicy -b nice -n 10) ;;
  full) PRIO=() ;;
  *) die "ARGUS_BUILD_PRIORITY must be gentle, background or full" ;;
esac
step "Building Argus $VERSION (${ARGUS_BUILD_PRIORITY:-gentle} priority)"
rm -rf "$OUT" && mkdir -p "$OUT"
node -e '
  const [v, k, e] = process.argv.slice(1);
  console.log(JSON.stringify({ version: v,
    bundle: { createUpdaterArtifacts: true, macOS: { signingIdentity: "-" } },
    plugins: { updater: { pubkey: k, endpoints: [e] } } }));' \
  "$VERSION" "$(cat "$KEY.pub")" "https://github.com/$RELEASE_REPO/releases/latest/download/latest.json" \
  > "$OUT/release.conf.json"
export TAURI_SIGNING_PRIVATE_KEY="$KEY"
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$KEY.password")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# npm ci wipes and reinstalls node_modules; skip it when the lockfile hasn't changed since.
scripts/cached app-npm --in app/package.json --in app/package-lock.json --out app/node_modules \
  -- sh -c 'cd app && npm ci --no-audit --no-fund'
# CI=true makes Tauri's bundle_dmg skip its Finder AppleScript (--skip-jenkins). That script
# mounts the .dmg and lays out its window in Finder, which popped an "Argus" disk window up on
# Greg's screen during every release (Greg, 2026-10-19). The .dmg still has Argus.app, the
# Applications link and the volume icon; only the icon positions are left to Finder.
# hdiutil sometimes fails with "Resource busy" while making the .dmg (often Finder holding that
# window open); the Rust build is cached after the first attempt, so a retry only re-bundles.
for attempt in 1 2 3; do
  if (cd app && CI=true ${PRIO[@]+"${PRIO[@]}"} npx tauri build --bundles app,dmg --config "../$OUT/release.conf.json"); then break; fi
  [ "$attempt" = 3 ] && die "build failed three times"
  echo "bundling failed (attempt $attempt), retrying"; sleep 10
done
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD

step "Collecting the release files"
bundle=app/target/release/bundle
tarball=$(ls "$bundle"/macos/*.app.tar.gz | head -n1)
[ -f "$tarball.sig" ] || die "updater signature missing; is the signing key right?"
cp "$(ls "$bundle"/dmg/*.dmg | head -n1)" "$OUT/$name.dmg"
cp "$tarball" "$OUT/$name.app.tar.gz"
node scripts/release/latest-json.mjs --version "$VERSION" --repo "$RELEASE_REPO" --tag "$TAG" \
  --platform "$PLATFORM" --bundle "$name.app.tar.gz" --sig "$tarball.sig" --notes "Argus $VERSION" \
  > "$OUT/latest.json"
prev=$(git describe --tags --abbrev=0 --match 'v*' "$SHA^" 2>/dev/null || true)
{
  echo "Built from $SOURCE_REPO@${SHA:0:7}."
  echo
  echo "First install: open the .dmg, drag Argus to Applications, then approve it once in System Settings > Privacy & Security > Open Anyway. Later versions install themselves."
  if [ -n "$prev" ]; then echo; echo "Changes since $prev:"; git log --format='- %s' "$prev..$SHA" | head -n 50; fi
} > "$OUT/notes.md"
echo "$SHA" > "$OUT/built-from"
ls -la "$OUT"
fi
if [ "$MODE" = --build-only ]; then echo "Built $TAG in $OUT (app: app/target/release/bundle/macos/Argus.app); publish with --publish-only"; exit 0; fi

step "Publishing $TAG to $RELEASE_REPO"
gh release create "$TAG" --repo "$RELEASE_REPO" --title "Argus $VERSION" --notes-file "$OUT/notes.md" \
  "$OUT/$name.dmg" "$OUT/$name.app.tar.gz" "$OUT/latest.json"
# Tag the source through the API, so this needs no git push credentials.
gh api "repos/$SOURCE_REPO/git/refs" -f ref="refs/tags/$TAG" -f sha="$SHA" >/dev/null \
  || echo "warning: couldn't tag $SOURCE_REPO at $TAG; the release is published anyway"

step "Checking what installed copies will see"
sleep 5
got=$(curl -fsSL "https://github.com/$RELEASE_REPO/releases/latest/download/latest.json" | node -e \
  'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).version))')
[ "$got" = "$VERSION" ] || die "latest.json says $got, expected $VERSION"
echo "Published: https://github.com/$RELEASE_REPO/releases/tag/$TAG"
