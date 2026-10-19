---
name: release
description: Ship a new Argus version to the auto-updating .dmg download, or fix the release scripts. Use when asked to release, publish a build, cut a version, or debug scripts/release/ or .github/workflows/release.yml.
---

# Release

Greg wants every release built locally on his Mac, never on paid GitHub runners (2026-10-18). Read `docs/release.md` first. In short:

1. Pick the next version (semver, higher than the newest release: `gh release view --repo gregorymfoster/argus-releases --json tagName -q .tagName`).
2. It's a release build, so it needs Greg's go (or run it overnight). Run it on his Mac through Remote Control in his checkout: `git checkout main && git pull`, then `just release-local X.Y.Z` (or `scripts/release/local.sh X.Y.Z`). To build ahead of his go, run `--build-only`, check it, ask, then `--publish-only`, which ships exactly that build even if main moved on. The build runs at gentle priority (`ARGUS_BUILD_PRIORITY=full` when Greg is away or says go). Since 2026-10-18 the release profile has no LTO and is incremental, so a release after small changes recompiles in seconds; a cold one (fresh `app/target`) is still many minutes. Follow CLAUDE.md's Remote Control rules: a time limit around the command (macOS has no `timeout`; use `perl -e 'alarm shift; exec @ARGV' 1800 ...`) and a progress note every 5 minutes. If another thread is using the checkout, coordinate first rather than switching branches under it.
3. The script ends by checking that `https://github.com/gregorymfoster/argus-releases/releases/latest/download/latest.json` shows the new version. Tell Greg with the release link.

If it stops with "no signing key in ~/.tauri", the key was moved or lost. Never generate a new signing key to get past it: installed copies only trust the original. Ask Greg to restore it from his backup.

The Actions workflow is a manual backup that needs paid macOS minutes; don't trigger it unless Greg asks.
