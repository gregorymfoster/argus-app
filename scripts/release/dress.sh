#!/usr/bin/env bash
# Makes a published release page look like a real download page (Greg, 2026-10-20): the notes
# from notes.sh, plain-English labels on the assets, and the repo's description and homepage.
# Changes only text, never the files or tags, so it's safe to re-run on any release.
# local.sh runs it after publishing; for an older release: scripts/release/dress.sh v0.1.4
# Releases without docs/release-notes/X.Y.Z.md get a short "earlier version" note.
set -euo pipefail
TAG="${1:?usage: $0 vX.Y.Z}"
VERSION="${TAG#v}"
RELEASE_REPO=${RELEASE_REPO:-gregorymfoster/argus-releases}
cd "$(git rev-parse --show-toplevel)"

# One line per asset: id, name, sha256.
assets=$(gh api "repos/$RELEASE_REPO/releases/tags/$TAG" --jq '.assets[] | [.id, .name, ((.digest // "") | sub("^sha256:"; ""))] | @tsv')
dmg=$(printf '%s\n' "$assets" | awk -F'\t' '$2 ~ /\.dmg$/ {print $2; exit}')
sha=$(printf '%s\n' "$assets" | awk -F'\t' '$2 ~ /\.dmg$/ {print $3; exit}')
[ -n "$dmg" ] || { echo "dress: $TAG has no .dmg" >&2; exit 1; }

notes=$(mktemp)
trap 'rm -f "$notes"' EXIT
if [ -f "docs/release-notes/$VERSION.md" ]; then
  RELEASE_REPO=$RELEASE_REPO scripts/release/notes.sh "$VERSION" "$dmg" "$sha" > "$notes"
else
  cat > "$notes" <<MD
An earlier version of Argus, kept for reference. Get the newest one from the [latest release](https://github.com/$RELEASE_REPO/releases/latest); installed copies update themselves.
MD
fi
gh release edit "$TAG" --repo "$RELEASE_REPO" --title "Argus $VERSION" --notes-file "$notes" >/dev/null

label() {
  case "$1" in
    *.dmg) echo "Argus $VERSION for Mac (Apple Silicon).dmg" ;;
    install.sh) echo "install.sh (the one-line installer)" ;;
    *.app.tar.gz) echo "Update bundle (the app downloads this itself)" ;;
    latest.json) echo "Update feed (read by the app)" ;;
  esac
}
printf '%s\n' "$assets" | while IFS=$'\t' read -r id name _; do
  l=$(label "$name")
  [ -z "$l" ] || gh api -X PATCH "repos/$RELEASE_REPO/releases/assets/$id" -f name="$name" -f label="$l" >/dev/null
done

gh repo edit "$RELEASE_REPO" \
  --description "Download Argus, the AI photo editor for the Mac. Source: gregorymfoster/argus-app" \
  --homepage "https://github.com/$RELEASE_REPO/releases/latest" >/dev/null
echo "Dressed https://github.com/$RELEASE_REPO/releases/tag/$TAG"
