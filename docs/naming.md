# Naming and organizing

How Argus names Shoots, folders, files and Keywords. The rules are borrowed from how working photographers already do it: Peter Krogh's *The DAM Book* and the ASMP's dpBestflow guidance, Lightroom's import and export templates, ISO 8601 dates, and the IPTC Photo Metadata Standard. Where those sources disagree, Lightroom wins, because that's what Greg already knows. ADR 0010 records why.

All of it is implemented once, in `engine/src/naming/mod.rs`. Features call that module and never build a name by hand.

## At a glance

| Thing | Default | Function |
|---|---|---|
| Shoot name (and its folder) | `2026-10-18_Boat-Trip` | `shoot_name(date, title)` |
| Copied originals | `<Library>/originals/2026-10-18_Boat-Trip/DSC08005.ARW` | `originals_folder`, `safe_file_name`, `unique_name` |
| Exported file | `2026-10-18_Boat-Trip_DSC08005.jpg` | `export_file_name` |
| Export folder | `<export location>/2026-10-18_Boat-Trip/` | `export_folder_name` |
| Sidecar | `DSC08005.xmp` beside the original | `sidecar_file_name` |
| Keyword | `Places\|Portugal\|Lisbon` | `Keyword::parse`, `hierarchical_subject`, `flat_keywords` |

## Characters

Every name Argus puts on disk uses ASCII letters, digits, hyphens and underscores, plus one dot before the extension. No spaces, ever (Greg's rule, and dpBestflow's too). Words join with hyphens. The underscore is reserved for separating fields, so a name splits cleanly back into date, Shoot and original. Accents fold to plain letters (`São Paulo` becomes `Sao-Paulo`), apostrophes vanish (`Maya's` becomes `Mayas`) and `&` becomes `and`. Case is kept as typed, since macOS doesn't care and `Boat-Trip` reads better than `boat-trip`. Slugs stop at 48 characters on a word break.

## Dates

Dates are ISO 8601 calendar dates with hyphens, `YYYY-MM-DD`. They sort correctly as plain text in Finder, in any app and on any OS, which is the whole reason pros put the date first. The date is the capture date of the Shoot's first frame, in the camera's local time (EXIF DateTimeOriginal), not the import date and not UTC. A shoot that runs past midnight or over a weekend keeps its first day. Only when no frame has a capture time does the import day stand in.

## Shoots and folders

A Shoot's name is its date, an underscore, then a short title: `2026-10-18_Boat-Trip`. The same string is the Shoot's folder name, so what you see in Argus matches what you see in Finder. The local model suggests the title from a few thumbnails ("Boat Trip"); the person can change it, and `shoot_name` turns whatever they type into the stored form. With no title it's the bare date, `2026-10-18`. A title that already starts with the date isn't dated twice.

Shoot folders sit flat under `originals/`, one per Shoot. Lightroom's "By date" option adds a year folder above them. Argus skips that because the date prefix already sorts them and a flat list is easier to relink and back up. Krogh's own layout is the same idea: one folder per shoot, named date first.

To show a Shoot name to people, `parse_shoot_name` splits it back into the date and the title as words. Older Shoots named before these rules (`Oct 12, 2026`, a folder's own name) come back whole as the title.

## Original files keep the camera's name

Copied originals keep the name the camera gave them, `DSC08005.ARW`, exactly as Lightroom and Capture One do by default. Krogh recommends renaming on import so every file name is unique forever. That made sense when the file system was the catalog. In Argus the Library tracks every Photo by content hash, the dated Shoot folder already makes the path unique, and keeping the camera name means the RAW still matches its camera JPEG, its card, and any Sidecar another app wrote.

Two cases still need care. A file from a phone or a download may have spaces (`IMG 1234 (1).jpg`), and `safe_file_name` makes it `IMG-1234-1.jpg` when Argus copies it. Two cards in one Shoot can both hold `DSC00001.ARW` (Sony counters wrap at 9999), and `unique_name` makes the second one `DSC00001-2.ARW`, as Finder and Lightroom do. Originals that stay where they are (imported without copying) are never renamed.

## Exports

An exported file is named Shoot, underscore, original stem: `2026-10-18_Boat-Trip_DSC08005.jpg`. That one name sorts by date, says which Shoot it came from when it lands in a client folder or a phone's camera roll, and leads straight back to its original, which dpBestflow treats as the point of a derivative's name. Lightroom's default export name is just the original's (`DSC08005.jpg`), which collides as soon as two Shoots meet in one folder. The extension is lowercase (`.jpg`, `.heic`). Exports go into a folder named after the Shoot under the chosen export location, and a name already there gets `-2`, `-3` and so on.

A sequence name like `2026-10-18_Boat-Trip_001.jpg` is a fine option to offer later, for delivering a set to someone. It shouldn't be the default because it breaks the link back to the original.

## Sidecars

An XMP Sidecar is named Adobe style: the original's stem with `.xmp`, so `DSC08005.ARW` gets `DSC08005.xmp`. That's the name Lightroom, Bridge, Camera Raw and Capture One look for. (darktable's `DSC08005.ARW.xmp` is the other convention; Argus doesn't write it.)

## Keywords

Keywords are hierarchical, the way Lightroom keeps them, and every one sits under one of five Facets:

- **People**: who is in the photo, by name, and only once the person has named them. The AI never guesses who someone is.
- **Places**: country, then region or city, then neighbourhood or landmark (`Places|Portugal|Lisbon|Alfama`).
- **Events**: the occasion (`Events|Birthday`, `Events|Wedding`).
- **Activities**: what people are doing (`Activities|Sports|pickleball`).
- **Subjects**: everything else in the frame (`Subjects|Animals|dog`, `Subjects|Nature|sunset`).

Those Facets mirror IPTC's own split between Person Shown, Location Shown, Event and Keywords. Common nouns are lowercase and singular; proper nouns keep their spelling. Five to fifteen Keywords per photo, only for what's clearly there. `TAGGING_GUIDE` in the naming module says this in a form any Provider's prompt can include, so Ollama and Claude tag alike.

`Keyword::parse` accepts what people and models actually type: `|`, `>` or `/` between levels, synonyms for the Facets (`Location`, `Who`), stray quotes and commas. A path with no Facet goes under Subjects.

## Writing metadata (IPTC and XMP)

When Argus writes metadata to an export or a Sidecar, it uses these fields. They're the ones Lightroom, Photo Mechanic and stock agencies read.

| What | XMP property | IPTC name |
|---|---|---|
| Keywords, flat | `dc:subject` from `flat_keywords` (every level, no Facet names) | Keywords (Core) |
| Keywords, hierarchy | `lr:hierarchicalSubject` from `hierarchical_subject` (`Places\|Portugal\|Lisbon`) | Lightroom's field, read by most DAMs |
| Title | `dc:title` | Title (Core) |
| Description | `dc:description` | Description, the old Caption (Core) |
| People named | `Iptc4xmpExt:PersonInImage` | Person Shown (Extension) |
| Place | `Iptc4xmpExt:LocationShown` (Sublocation, City, ProvinceState, CountryName, CountryCode) | Location Shown (Extension) |
| Event | `Iptc4xmpExt:Event` | Event (Extension) |
| Creator, copyright | `dc:creator`, `dc:rights`, `xmpRights:Marked` | Creator, Copyright Notice (Core) |
| Verdict | `xmp:Rating` of -1 for a Reject, as Bridge writes it (Lightroom keeps Picks in its catalog only) | not IPTC |

Today Argus writes the Keywords and caption a Photo keeps (never AI suggestions still waiting) to the Sidecar of an Original export and as an XMP packet inside each exported JPEG, after its EXIF, the way Lightroom embeds them. The Library keeps Keywords in `photo_keywords` as full paths, each with who added it and whether it is kept, suggested or dismissed; a dismissed AI Keyword stays as a row so the AI doesn't offer it again (`engine/src/library/keywords.rs`). The AI's suggestions come from the `tag_photos` job (`engine/src/tagging.rs`), which drops anything under People.

The flat list leaves out the Facet names because "Places" and "Subjects" are filing drawers, not words anyone searches for. Lightroom treats such parent keywords the same way when they're marked not to export.
