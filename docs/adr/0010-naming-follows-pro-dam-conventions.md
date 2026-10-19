# 10. Names follow pro DAM conventions, and originals keep camera names

Status: accepted (2026-10-18)

## Context

Greg asked for the most official default naming formats for photos, shoots and tags. Argus names things in several places (Import, the originals folder, Export, Sidecars, AI tagging), each owned by a different part of the code, and they were drifting: the engine's slug lowercased, the design said `2026-10-18_Boat-Trip`, and exports kept the bare camera name.

There is no single standard. The closest things are Peter Krogh's *The DAM Book* and the ASMP's dpBestflow (date-first names, no spaces, a fixed character set, rename every file to something unique), Lightroom's import and export templates (keep the camera name by default, folders by date), ISO 8601 for dates, and IPTC Core and Extension with Lightroom's `lr:hierarchicalSubject` for keywords.

## Decision

One engine module, `engine::naming`, holds every naming rule, written up in `docs/naming.md`. Shoots are `YYYY-MM-DD_Title-Words` from the first frame's capture date, and that string is also the folder name. Copied originals keep the camera's file name in `originals/<Shoot>/`, with `-2` on a clash. Exports are `<Shoot>_<original stem>.jpg`. Sidecars are `<stem>.xmp`. Keywords are hierarchical under five Facets (People, Places, Events, Activities, Subjects) and are written to both `dc:subject` and `lr:hierarchicalSubject`.

## Consequences

- Not renaming originals goes against Krogh's advice. We accept it because the Library identifies Photos by content hash and the dated folder keeps paths unique. Keeping names also leaves RAW+JPEG pairs, card scans and other apps' Sidecars matching. If Argus ever flattens originals into one folder, revisit this.
- Export names are longer than Lightroom's default, but they don't collide across Shoots and they lead back to the original.
- Changing any of these defaults later means old Shoots and exports keep the old names. `parse_shoot_name` already reads names made before these rules.
