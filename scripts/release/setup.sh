#!/usr/bin/env bash
# One-time release setup, run on Greg's Mac from the repo root: scripts/release/setup.sh
# Creates the update-signing key (kept in ~/.tauri, never committed), stores it as GitHub
# Actions secrets, creates the public binaries-only releases repo and asks for its token.
# Safe to re-run: it reuses an existing key and skips what's already done.
set -euo pipefail

SOURCE_REPO=gregorymfoster/argus
RELEASE_REPO=${RELEASE_REPO:-gregorymfoster/argus-releases}
KEY="$HOME/.tauri/argus-updater.key"
PASSWORD_FILE="$KEY.password"

command -v gh >/dev/null || { echo "Needs the GitHub CLI: brew install gh && gh auth login"; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "Run 'gh auth login' first."; exit 1; }

mkdir -p "$HOME/.tauri"
if [ -f "$KEY" ]; then
  echo "Reusing the existing update-signing key at $KEY"
else
  umask 077
  openssl rand -hex 32 > "$PASSWORD_FILE"
  npx --yes @tauri-apps/cli@2 signer generate --ci -p "$(cat "$PASSWORD_FILE")" -w "$KEY" >/dev/null
  echo "Created the update-signing key at $KEY"
fi

gh secret set TAURI_SIGNING_PRIVATE_KEY --repo "$SOURCE_REPO" < "$KEY"
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo "$SOURCE_REPO" < "$PASSWORD_FILE"
gh variable set TAURI_UPDATER_PUBKEY --repo "$SOURCE_REPO" --body "$(cat "$KEY.pub")"
echo "Stored the key as Actions secrets on $SOURCE_REPO"

if [ "$RELEASE_REPO" != "$SOURCE_REPO" ]; then
  if ! gh repo view "$RELEASE_REPO" >/dev/null 2>&1; then
    gh repo create "$RELEASE_REPO" --public --add-readme \
      --description "Downloads and auto-updates for Argus (the source is private)"
    echo "Created the public releases repo $RELEASE_REPO"
  fi
  gh variable set RELEASE_REPO --repo "$SOURCE_REPO" --body "$RELEASE_REPO"
  if gh secret list --repo "$SOURCE_REPO" | grep -q '^RELEASES_TOKEN'; then
    echo "RELEASES_TOKEN is already set"
  else
    echo
    echo "Last step: a token that can publish to $RELEASE_REPO and nothing else."
    if [ ! -t 0 ]; then
      echo "No terminal to paste into: create the token as below, then add it as RELEASES_TOKEN at"
      echo "https://github.com/$SOURCE_REPO/settings/secrets/actions/new"
    fi
    echo "Opening GitHub. Pick 'Only select repositories' > ${RELEASE_REPO#*/},"
    echo "Repository permissions > Contents: Read and write, then Generate token."
    open "https://github.com/settings/personal-access-tokens/new" 2>/dev/null || true
    [ -t 0 ] && gh secret set RELEASES_TOKEN --repo "$SOURCE_REPO"   # prompts for the token
  fi
fi

echo
echo "Done. Back up $KEY and $PASSWORD_FILE (e.g. in 1Password): without them,"
echo "installed copies of Argus can never update again."
