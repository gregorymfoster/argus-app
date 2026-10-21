# Releasing Argus

Releases are built on Greg's Mac, which costs nothing. GitHub's macOS runners would use paid Actions minutes on a private repo, and Greg wants no billing (2026-10-18), so Claude always releases locally.

`scripts/release/local.sh` builds the Tauri app for Apple Silicon at gentle priority (utility QoS + nice: performance cores, yields to Greg; `ARGUS_BUILD_PRIORITY=full` when he's away, `background` for the old efficiency-cores-only mode) and gives it an ad-hoc signature. It signs the update bundle with Argus's update-signing key and publishes a release to the public [argus-releases](https://github.com/gregorymfoster/argus-releases) repo with the `.dmg`, the update bundle and `latest.json`. Installed copies read `latest.json` from the newest release on launch and update themselves. From 0.1.2 on they check soon after launch and every 6 hours, download and install in the background, then show **Restart to update** in the sidebar footer (and a toast once), like Cursor and Claude. Argus › Check for Updates…, ⌘K and Settings › Updates check by hand (`app/src/updates.rs`, `prototype/src/ui/Updates.tsx`). In a browser, `?update=ready` fakes a ready update for screenshots. Updates use the signed `.app.tar.gz` from `latest.json`, never the `.dmg`, which is only for first installs. The build runs with `CI=true` so Tauri skips its Finder layout step, which otherwise opens the mounted "Argus" disk window on the Mac during every release.

Why a second repo: argus is private, so its release files need a GitHub login, and the updater can't sign in. argus-releases holds binaries only. The source never goes there.

## Ship a version

On the Mac, on an up-to-date main with nothing uncommitted:

```sh
just release-local 0.2.0      # or scripts/release/local.sh 0.2.0
just ship 0.2.0                         # all of it on Greg's one "ship it": release-candidate, publish, installer dry run
just release-candidate 0.2.0            # check, then mac-e2e beside --build-only, as one unattended job
just release-local 0.2.0 --build-only    # build and check it first (mac-e2e), no publish
just release-local 0.2.0 --publish-only  # then publish that exact build once Greg says go
```

The script refuses to publish a version that isn't newer than the current release, because the updater only installs a higher semver than the one running. It takes the version from its argument, so nobody bumps `tauri.conf.json`, and it tags the source through the GitHub API (no git push credentials needed). It ends by fetching `latest.json` the way installed copies do and checking that the version matches. Build files land in `out/release/vX.Y.Z/`.

A release build keeps the Mac busy for several minutes, so Claude runs it when Greg says go or overnight, following the Remote Control rules in CLAUDE.md (time limits and progress notes).

## The public release page

argus-releases is the page friends land on, so it reads like a download page, not a build log (Greg, 2026-10-20: "way less sketchy"). Before a release, write `docs/release-notes/X.Y.Z.md`: a few bullets on what's new, in words for people who use the app (no commit subjects, bench work or anything from this private repo). `local.sh` stops before building without it. `scripts/release/notes.sh` wraps those bullets in the install steps, the not-notarized explanation, a link to the public source in argus-app and the `.dmg`'s SHA-256. After publishing, `scripts/release/dress.sh vX.Y.Z` sets the notes, gives the assets plain labels (".dmg", "Update bundle (the app downloads this itself)") and sets the repo's description; it only changes text, so re-run it on any release after editing the template. The repo's README (icon, install, what Argus is) lives in argus-releases itself. Its screenshots (`screenshots/` there, also shown in every release's notes) are the real app on a sample card of public-domain photos, never Greg's: the card is in `/mnt/project-files/argus-release-shots/` with its licences. To refresh them after a UI change, in a cloud session: `argus import` that card into a fresh Library, `argus cull --provider claude-code`, `argus tool suggest_edits '{"photo_ids":[28]}'` then `argus work --drain`, and `ARGUS_PROVIDER=claude-code node prototype/scripts/release-shots.mjs <library> 28 34` (Edit shot, Agent shot); look at the JPEGs in `prototype/out/release-shots/`, resize to 1800 px wide and commit them to argus-releases.

## The one-command installer

Each release also carries `install.sh`, so friends can install with `curl -fsSL https://github.com/gregorymfoster/argus-releases/releases/latest/download/install.sh | bash` (Greg, 2026-10-20). It installs Argus.app from that release's `.dmg` (checksum verified, quarantine cleared on Argus.app only), installs Ollama from ollama.com if it's missing, starts it and pulls the default model, after checking Apple Silicon, macOS 13+, memory (it skips the default model under 32 GB) and free disk. It asks one Y/n before the model download (`--yes` skips it, `--dry-run` changes nothing), never uses sudo, and falls back to `~/Applications` when `/Applications` isn't writable. The source in `scripts/release/install.sh` has placeholders; `local.sh` fills them through `stamp-install.sh` (version, `.dmg` name and SHA-256, and `DEFAULT_OLLAMA_MODEL` from `engine/src/cull.rs` at the built commit) and uploads the result. `scripts/release/install.test.sh` runs it against fake macOS commands; it's part of `just check` in the cloud and skips on the Mac. It's written for macOS's own bash 3.2.

## One-time setup (done 2026-10-18)

```sh
scripts/release/setup.sh
```

It creates the update-signing key in `~/.tauri/argus-updater.key` (with a random password beside it), stores both as Actions secrets on argus for the backup workflow, creates argus-releases, and asks for a fine-grained token that can write to argus-releases only (`RELEASES_TOKEN`, also only for the backup workflow). Re-running it is safe.

Back up the key and its password somewhere safe. If they're lost, every installed copy is stuck on its version for good, because the updater rejects bundles signed by any other key.

## Backup: GitHub Actions

`.github/workflows/release.yml` does the same build on a GitHub macOS runner. It only runs when started by hand from the Actions tab, and it needs Actions billing for macOS minutes. It fails early with a clear message if a secret is missing.

## What's checked where

`node --test scripts/release/latest-json.test.mjs` covers the `latest.json` format, and `scripts/release/install.test.sh` the installer. The build itself can only be checked on macOS, so the release script's own final check (the published `latest.json` version) and installing the `.dmg` are the test.

Release-only config (version, public key, update URL, ad-hoc signing, updater bundles) is merged over `app/tauri.conf.json` at build time, so dev builds don't need the key and never update themselves.
