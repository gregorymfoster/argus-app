#!/usr/bin/env bash
# Prints the public release notes for one version: the curated highlights in
# docs/release-notes/X.Y.Z.md wrapped in the install steps friends need. The page is public,
# so nothing here comes from the private repo's history (Greg, 2026-10-20: "way less sketchy").
# Usage: scripts/release/notes.sh X.Y.Z DMG_NAME [DMG_SHA256]
set -euo pipefail
VERSION="${1:?usage: $0 X.Y.Z DMG_NAME [DMG_SHA256]}"
DMG="${2:?usage: $0 X.Y.Z DMG_NAME [DMG_SHA256]}"
SHA256="${3:-}"
RELEASE_REPO=${RELEASE_REPO:-gregorymfoster/argus-releases}
cd "$(git rev-parse --show-toplevel)"
highlights="docs/release-notes/$VERSION.md"
# Screenshots of the app on public-domain photos live in the public repo, so the page shows them
# for every version; refresh them with prototype/scripts/release-shots.mjs (docs/release.md).
SHOTS="https://raw.githubusercontent.com/$RELEASE_REPO/main/screenshots"
[ -f "$highlights" ] || { echo "notes: write $highlights first (what's new, for people who use the app)" >&2; exit 1; }

cat <<MD
Argus is a photo editor for the Mac that culls and edits a whole shoot with an AI model running on your own computer. Your photos stay on your Mac.

![Argus showing a shoot of 34 photos, each rated with stars by the AI cull]($SHOTS/grid.jpg)

## Install

Paste this into Terminal. It installs Argus, [Ollama](https://ollama.com) and the local AI model, and asks once before the big download.

\`\`\`sh
curl -fsSL https://github.com/$RELEASE_REPO/releases/latest/download/install.sh | bash
\`\`\`

Rather do it by hand? Download **$DMG** below, open it and drag Argus into Applications. The first time you open it, macOS stops it because Argus isn't notarized. Go to System Settings > Privacy & Security, scroll to the note about Argus and click Open Anyway. You only do this once, and later versions install themselves.

## What's new in $VERSION

$(cat "$highlights")

| | |
|---|---|
| ![Bursts become Stacks: the frames side by side, the sharpest marked Best]($SHOTS/stacks.jpg) | ![Edit with three suggested edits under a portrait]($SHOTS/edit.jpg) |
| Bursts become Stacks. Press Enter to keep the best frame. | A few finished edits per photo. Nothing changes until you accept one. |

## About this download

Argus needs a Mac with Apple Silicon and macOS 13 or later. The default model runs best with 32 GB of memory; the [README](https://github.com/gregorymfoster/argus-app#set-up-the-ai) covers smaller models and using your Claude plan instead.

It's a free side project without a paid Apple developer account, which is why macOS asks you to confirm it once. The app is open source under the MIT licence at [gregorymfoster/argus-app](https://github.com/gregorymfoster/argus-app), so you can read every line or build it yourself. The "Source code" archives GitHub adds below are only this download page's files.
MD
if [ -n "$SHA256" ]; then
  cat <<MD

To check the download, run \`shasum -a 256 ~/Downloads/$DMG\` and compare it with:

\`\`\`
$SHA256
\`\`\`
MD
fi
cat <<'MD'

<details>
<summary>Which file do I need?</summary>

Just the <b>.dmg</b>, or none at all if you use the one-line install. <b>install.sh</b> is that installer. The <b>.app.tar.gz</b> and <b>latest.json</b> are what installed copies of Argus download to update themselves.

</details>
MD
