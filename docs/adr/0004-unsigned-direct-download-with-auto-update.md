# 4. Direct download without an Apple Developer account, with auto-update

Status: accepted (2026-10-17)

## Context

Early Argus is Mac only. Greg wants to ship without paying for an Apple Developer account for now, but wants installs to update themselves whenever he ships a new version.

## Decision

Builds are ad-hoc code signed (the minimum Apple Silicon needs to run at all), not notarized, and published as a .dmg on GitHub Releases. The Tauri updater checks a `latest.json` on GitHub Releases and installs new versions in the background. It verifies each update with Argus's own update-signing key, which is separate from Apple signing and costs nothing.

## Why

Auto-update works without Apple's involvement, because Tauri's updater trusts its own key. The cost falls only on the first install. macOS blocks an un-notarized app the first time it opens, and since macOS 15 the person has to approve it in System Settings > Privacy & Security > Open Anyway. Updates fetched by the app itself aren't quarantined, so they shouldn't prompt again (to verify on a real update).

## Consequences

The install page needs a two-step "Open Anyway" note. The update-signing private key must be kept safe: losing it means existing installs can never update again. Moving to a Developer ID and notarization later is a build-config change, and it removes the first-launch warning.

## Amendment (2026-10-18)

The argus repo is private, and the updater can't sign in to read a private repo's releases. Releases are therefore published to a separate public repo, gregorymfoster/argus-releases, that holds only binaries. Builds are Apple Silicon only for now; a universal build is a one-flag change if an Intel Mac ever needs it.

Releases are built on Greg's Mac (`scripts/release/local.sh`), not on GitHub's macOS runners: those cost paid Actions minutes on a private repo and Greg wants no billing (2026-10-18). The Actions workflow stays as a manual backup.
