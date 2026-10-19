# 11. The Library is one portable folder, and card imports copy into it

Status: accepted (2026-10-18). Amends ADR 0002, which had Import always read originals in place.

## Context

Greg wants to pick exactly where on his Mac the Library lives, to see and manage the disk space it takes, and to be able to carry it to another disk or Mac. Lightroom's model is the one he knows: a catalog he places himself, a Copy or Add choice on Import, previews and caches he can size and clear, and a "find missing photo" flow when files move. Until now Argus stored absolute paths to originals and read everything in place, which works until the card is formatted or a folder is renamed.

## Decision

A Library is one folder the person chooses. It holds `library.db`, `previews/`, `originals/` (photos copied in by Import), `cache/` and `backups/`. Every path to something inside the folder is stored relative to it, so moving or copying the whole folder to another disk or Mac keeps it working. Paths to originals left outside it stay absolute, and those Photos show as missing (with their previews still usable) when that disk is not connected.

Import from a memory Card copies the originals into `originals/<Shoot name>/` (`originals/2026-10-12_Boat-Trip/`, named per docs/naming.md), because a card gets formatted. Each copy is read back and checked against the original's hash before the Photo is recorded. Import from a folder reads in place by default and offers the copy. This is Lightroom's Copy and Add.

Preview size and quality are Library settings, stored in the Library (they travel with it). Changing them applies to new imports, and `rebuild_previews` remakes the existing ones.

The storage view reports originals inside the Library, originals elsewhere, previews, cache, database and backups, plus free space on the disk. Only `cache/` and previews no Photo uses count as cache, and clearing never touches originals, edits, previews in use or backups.

Missing originals relink by pointing Argus at a folder. It matches by file name and size, then confirms by content hash, so a different file with the same name is never linked.

Moving a Library is a rename on the same disk. Across disks it copies every file, verifies each by hash, and only then removes the old folder. Any mismatch deletes the copy and leaves the original Library where it was.

The desktop app remembers the current Library and recent ones in its own config, outside any Library. First launch asks where to put the Library. An existing `~/Pictures/Argus/library` is adopted silently, and `ARGUS_LIBRARY` still overrides everything for tests and the CLI.

Renaming a Shoot renames its originals folder too (2026-10-18), so Finder keeps matching Argus. The folder is renamed first (a single `rename` on the same disk), then the Shoot's name and its Photos' relative paths change in one transaction; if that write fails the folder is renamed back. A crash between the two leaves the Photos pointing at the old folder, which `find_missing_originals` and relink repair. Only a folder that holds this Shoot alone moves: originals read in place, or a folder another Shoot shares, stay where they are and only the name changes. A taken folder name gets `-2`, as on Import. Restoring a Backup made before a rename points at the old folder name, and relink fixes that the same way.

## Why

Relative paths are the cheapest way to make the folder portable, and they cost nothing at read time. Copying from cards protects against the most common way to lose photos, formatting a card that was imported in place. Hash verification makes a copy as trustworthy as the original, which matters because the copy may become the only one.

## Considered and rejected

Keeping a Shoot's folder name fixed after a rename. It is simpler and can't strand a Photo, but the Library would slowly stop matching Finder, which breaks the naming rule that what you see in Argus is what you see on disk (docs/naming.md).

A macOS package (`Argus.library` shown as a single file, like Photos). It looks tidy, but hides the originals from Finder and from other apps, and Greg wants to see where things are.

Always copying, including folders already on the Mac. That doubles disk use for photos that are already safe, which is the opposite of what Greg asked for.

Deleting the old Library before verifying the new copy, or never deleting it. The first risks data, the second leaves Greg to clean up a full second copy by hand.
