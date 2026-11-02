# 17. Backup places are folders, and each copy is a working Library

Status: accepted (2026-11-02).

## Context

Greg asked for "powerful controls to backup the library to gdrive, local disk, external harddrive, etc", plus iCloud, and an easy view of where the storage is. Until now the only backups were daily copies of `library.db` inside the Library's own `backups/` folder (ADR 0009). They sit on the same disk as everything else and never hold an original, so a dead or stolen Mac loses every Photo that Import copied in from a card. Those originals often exist nowhere else once the card is formatted.

The project rule is no API keys and no OAuth apps. Greg already runs the Google Drive desktop app, iCloud Drive, external SSDs and a network disk (gf-storage).

## Decision

A Backup place is a folder. That can be an external or network disk, the Google Drive, Dropbox or OneDrive app folder under `~/Library/CloudStorage/`, iCloud Drive, or any folder the person picks. Argus writes files there, and the cloud app (if there is one) uploads them on its own schedule. Argus never signs in anywhere. Greg chose this on the decision card (2026-11-02) over adding rclone remotes and over a single backup disk.

Each place's copy lives in `<folder>/Argus-Backup/<Library folder name>/` and uses the Library's own layout: `library.db` (always the newest), `backups/` (at most one dated database copy a day, keeping 30), `originals/`, `previews/` when the place copies Everything, and `elsewhere/<absolute path>` for originals the Library reads in place (opt in). `argus-backup.json` records each file's size and blake3 hash, along with the Library's id. Because the layout matches, the copy opens as a Library on any Mac with Open Library. A second Library with the same folder name gets `-2` and never shares a copy.

A run never deletes and never overwrites anything except `library.db` and the manifest. Originals never change and previews are named by content, so a file the manifest already holds at the same size is skipped. Every new file is hashed as it is read, written under a temporary name, read back, compared, and only then renamed into place. A different file already sitting at a path is left alone and reported. Removing a place forgets it and leaves its copy where it is.

Backups run as the `backup` Queue job, with progress in MB and an ETA. One job copies at most 10 minutes' worth and then queues the rest, so a first backup of a terabyte never holds the AI work up for hours. Import queues a backup of the new Shoot to each place set to "After each Import". Finish Shoot calls `run_backup` with its `shoot_id`. The worker queues Daily and When-connected places once their last good run is a day old, and runs a monthly check that re-reads every file against its hash. A failed run waits an hour before it tries again.

What a place is set to do lives under the `backup_places` setting. What runs have learned lives under its own `backup_state:<id>` setting: the Photo id ranges in the copy, size, the last good run and check, and a history of 50 runs. Runs write only the state and the person writes only the settings, so neither can overwrite the other. No schema step was needed.

## Considered and rejected

rclone remotes (Backblaze, S3, Dropbox and others straight to the cloud). They are powerful, but they need rclone installed and signed in from Terminal, and every cloud Greg uses already has a desktop folder. This can come back later as another kind of place.

The Google Drive API with our own OAuth app. It needs keys and a consent screen, which the project avoids.

A mirror that deletes what the Library removed. That's simpler to reason about, but one wrong remove or a bad sync would then destroy the backup too. Pruning stays a manual step for later.

A custom archive format (zip or tar per Shoot). It would need Argus to restore it, whereas a plain folder in the Library layout can be browsed in Finder and opened as it is.
