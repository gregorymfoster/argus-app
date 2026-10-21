# Argus

AI-native batch photo editing for the Mac. This glossary fixes the words we use for the photos, their edits and how they're judged.

## Language

**Library**:
The set of photos Argus manages together, with all their edits and judgments. On disk it is one folder the person chooses, which can move to another disk or Mac (ADR 0011).
_Avoid_: Catalog, collection, project

**Photo**:
One original image file (RAW or JPEG/HEIC) imported into a Library. Argus never modifies it.
_Avoid_: Image, asset, picture

**Shoot**:
The Photos brought in by one Import, usually one outing. A Library holds many Shoots.
_Avoid_: Session, event, folder

**Shoot name**:
A Shoot's date and title with no spaces, `2026-10-18_Boat-Trip`, which is also its folder name. See docs/naming.md.
_Avoid_: Slug, folder name

**Keyword**:
A hierarchical tag on a Photo under one of five Facets (People, Places, Events, Activities, Subjects), such as `Places|Portugal|Lisbon`, written to XMP the way Lightroom does. The AI suggests them and the person confirms.
_Avoid_: Tag, label

**Import**:
Bringing photos from a folder or memory card into a Library.
_Avoid_: Ingest, upload, sync

**RAW+JPEG pair**:
The two files a camera writes for one frame when it shoots RAW+JPEG. Import makes them one Photo, as Lightroom does: the RAW is the Photo and the camera's JPEG is kept beside it. "RAW only" skips the JPEG.
_Avoid_: Duplicate, sidecar

**Card**:
A camera's memory card as Argus sees it: a mounted volume with a `DCIM` folder. Import finds Cards by itself and shows which of their photos the Library already has.
_Avoid_: Disk, drive, SD

**Recipe**:
The complete, versioned list of edit settings for one Photo, readable as one JSON document.
_Avoid_: Preset, develop settings, edit stack

**Version**:
One saved state of a Recipe. Every change creates a new Version, and undo returns to an earlier one.
_Avoid_: Snapshot, history entry

**Batch**:
The changes made together by one request or action (an Agent request, a cull, a Look change), undone together in one step.
_Avoid_: Transaction, changeset

**Sidecar**:
A copy of a Recipe written next to the original file. It is never the source of truth.
_Avoid_: XMP

**Override**:
A person changing something the AI decided, such as flipping a Verdict or moving a slider after an AI edit.
_Avoid_: Correction, feedback

**Analyzer**:
One specialist that measures something about a Photo (faces, eyes open, an embedding, sharpness), usually with a small offline model. Analyzers write Facts (ADR 0013).
_Avoid_: Plugin, detector, model (for the whole thing)

**Fact**:
What one Analyzer found about one Photo, stored in the Library and recomputed when the Analyzer or the preview changes. The cull and the Agent read Facts.
_Avoid_: Feature, metadata, annotation

**Usage log**:
The Library's append-only record of what happened to each Photo: where it started, every AI take, what the person kept, flagged and exported. Never uploaded; benches and learning read it (docs/usage-log.md).
_Avoid_: Telemetry, analytics, history

**Debug log**:
The Mac's record of how Argus ran, for tuning and fixing it: every command, Queue job and model request with its timing, and what the AI used. JSON lines in `~/Library/Logs/Argus/debug.jsonl`, rotated, never uploaded. Unlike the Usage log it is about the app, not the Photos.
_Avoid_: Telemetry, trace, analytics

**Resource meter**:
What the AI is taking from the Mac right now and since launch: CPU, memory, power and GPU for the Local model (Ollama), Claude and Argus itself. A line under Background work, with a panel.
_Avoid_: Activity monitor, stats, usage widget

**Backup**:
A whole copy of a Library's database (Verdicts, Recipes, Looks), kept in the Library's `backups/` folder. Argus makes one a day and before every upgrade. Originals and previews are never in it.
_Avoid_: Snapshot, archive

**Remove**:
Taking Photos or a whole Shoot out of the Library, as Lightroom's Remove does. Everything that belongs to them goes to the Library's `trash.db` for 30 days, so Undo or Restore puts them back exactly. The originals stay where they are unless the person also chooses to move them to the Trash.
_Avoid_: Delete (that word is for originals only)

**Preview**:
The JPEG Argus shows for a Photo in the grid and loupe, kept in the Library's `previews/` folder. Import first uses the camera's embedded JPEG so the grid fills fast, then Argus replaces it with its own render. A Preview can always be rebuilt from the original. An edited Photo also gets an **edited preview** (its Recipe rendered, under `previews/edited/`), kept on disk until the Recipe changes; like any cache it can be cleared.
_Avoid_: Thumbnail (except the Import dialog's quick look at files not imported yet), proxy, cache

**Export**:
Writing Photos out of Argus as new files (JPEG, TIFF, or the original with an XMP sidecar), named by the Shoot name and never overwriting anything. The originals stay untouched.
_Avoid_: Save, publish, render (render is what the pipeline does, at any size, for any purpose)

**Agent**:
The AI inside Argus that plans and carries out edits and culls through the same tools the UI uses.
_Avoid_: Assistant, bot, copilot

**Provider**:
The model that powers the Agent, either a local open-source model or a cloud model on the user's key.
_Avoid_: Backend, LLM, engine

**Compute level**:
How much of the Mac a local Provider may take while it works: Gentle (the default, for when the person is at the Mac), Balanced or Full (overnight).
_Avoid_: Power mode, priority, performance

## Culling

**Cull**:
Sorting a batch of Photos into the ones worth keeping and the ones to drop. The AI suggests and the person confirms.
_Avoid_: Filter, triage, select

**Verdict**:
A Photo's cull outcome, as a Lightroom-style flag: Pick, Reject or Unflagged. The AI cull is cautious (Greg, 2026-10-20): it only proposes Reject, with a reason, for a plainly broken frame it rated 1 star, and leaves picking to the person, whose choice always overrides it.
_Avoid_: Maybe, status

**Rating**:
A Photo's Lightroom-style stars, 0 (unrated) to 5, set with the 0-5 keys. The AI cull suggests 1 to 5 stars for every Photo, judged against the rest of its Shoot, and the person's Rating overrides it. A Rating grades; a Verdict decides.
_Avoid_: Score, grade, stars (in code)

**Best Photos**:
The AI's ranked short list of a Shoot's strongest Photos, about one in ten and never two from one Stack, named after Lightroom's Best Photos view. The cull chooses them; Photos the person rejects drop out.
_Avoid_: Highlights, favorites, top picks

**Technical check**:
The cull's first stage: the model looks at every Photo of the Shoot and says only whether a frame is technically broken (missed focus, shake, eyes closed, exposure beyond rescue). Local models call this well, so it is all they are asked (Greg, 2026-10-20).
_Avoid_: QC, quality check

**Taste**:
The cull's second stage, how likely the person is to keep a frame that passed the Technical check. It comes from Facts, not the model (today the main faces: smiles, and anyone who looks off), and sets the stars, the best frame of each Stack and the Best Photos (`engine::taste`).
_Avoid_: Aesthetics, quality score

**Signals**:
Cheap measurements of each Photo (sharpness within its Shoot, brightness, clipping) that the AI reads beside the image. Hints for its judgment, never a Verdict on their own.
_Avoid_: Scores, metrics

**Stack**:
A group of near-identical Photos (a burst or repeated shot) that get culled together, so only the best ones survive. Argus stacks bursts by itself (look-alike frames shot seconds apart, `engine::bursts`), and the AI cull joins its own Stacks to them. The Stacks view (N) shows one Stack at a time to keep its best frame or two.
_Avoid_: Group, burst, cluster

**Contact sheet**:
Small previews of every Photo in a Shoot, in time order, with their Stacks and signals. The Agent reads it so it judges each Photo against the whole Shoot.
_Avoid_: Overview, summary, context dump

## Editing

**Look**:
A shared creative style (color grade, contrast, mood) applied across a set of Photos so they read as one shoot.
_Avoid_: Preset, filter, style

**Correction**:
A per-Photo adjustment (exposure, white balance, light on faces) that brings each frame to the same baseline under a Look.
_Avoid_: Auto fix, adjustment

**Suggestion**:
A complete edit (sliders, Masks, crop and straighten) the AI proposes for one Photo, usually two or three to tab through, each with its own character (Natural, Punchy, Moody, then Black and white and Airy) under that name. It changes nothing until the person accepts it, at its Amount, which saves it as a new Version.
_Avoid_: Proposal, auto edit, preset

**Adaptive layer**:
What every fresh Suggestion starts from on top of Auto, like Lightroom's Adaptive presets: a little presence (Clarity, Texture, Dehaze) and, through the AI Masks for what the Photo has, the people lifted, the background let down a touch and a bright sky held.
_Avoid_: Base edit, auto masks

**Amount**:
How strong a Suggestion is applied, 0-150%, as on a Lightroom preset: 0 is the Photo as it is, 100 the Suggestion, and every setting moves in proportion.
_Avoid_: Strength, intensity, opacity

**Style profile**:
How the person usually finishes a take they kept, learned from the Usage log: the median change to each global slider on the Photos they exported from a kept take, shrunk while there are few. Fresh Suggestions add it on top of every take, so the AI drifts toward the person's own edits; it is empty until they have exported at least three.
_Avoid_: Preset, taste model, personal Look

**Review round**:
One pass in which the AI looks at its own Suggestions rendered, with their light measured in numbers, and fixes each one's biggest remaining problem. Suggestions get one at the Full Compute level, none otherwise.
_Avoid_: Iteration, self-critique, refine step

**Hero Photo**:
The Photo the person edited by hand and wants the rest of the Shoot to match ("apply to the rest"). Its style becomes a Look, and every other Photo gets matched Suggestions under it.
_Avoid_: Reference image, master, source

**Queue**:
The AI work waiting to run in the background (Suggestions, "apply to the rest", culls), kept in the Library so it survives a quit, and worked at the Compute level.
_Avoid_: Tasks, jobs list, pipeline

**Mask**:
A region of a Photo (subject, face, skin, hair, sky, background, chosen parts of one person, or one drawn by the person as a brush, linear or radial shape) that a local adjustment applies to. A Recipe stores how a Mask is made, never its pixels. A People Mask is Lightroom's Select People: any of face skin, body skin, eyes, teeth, hair or the whole person, for one person or everyone.
_Avoid_: Selection, layer

**Local adjustment**:
Slider changes that apply only inside one Mask, on top of the global sliders.
_Avoid_: Layer, local edit

**Spot**:
One circle of Lightroom's Heal or Clone tool: pixels copied over a small blemish, stray hair or dust speck from a source circle elsewhere in the same Photo, which Argus picks and the person can drag. Heal blends the copy into the light and colour around the spot; Clone copies it exactly. Nothing is generated, and a Spot is small (up to 15% of the long edge), so it never removes whole objects.
_Avoid_: Healing brush, patch, object removal

**Retouch**:
Natural people fixes that never move a pixel: skin smoothing, brighter eyes, whiter teeth and light on faces.
_Avoid_: Beautify, filter, face tune

**Analysis**:
What the local Mask models found in a Photo (people, skin, hair, sky, faces with their eyes and mouths), made once from its preview and cached.
_Avoid_: Detection, segmentation result

**Denoise**:
Lightroom's AI Denoise: a learned model (SCUNet) runs once over a RAW's full-size image in the background and the result is kept as the Photo's denoised base in the Library's cache. The Recipe's Denoise Amount (0-100, 0 is off) blends that base over the plain image, so the slider moves instantly once the base exists. RAW only. Separate from the manual Noise reduction sliders.
_Avoid_: Noise reduction (that's the manual sliders), enhance, clean up

**Auto**:
Lightroom's Auto button: Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Temp, Tint and Vibrance set from a Photo's own light by a fast, deterministic measurement (no model). It replaces only those sliders and keeps the rest of the Recipe.
_Avoid_: Auto-enhance, magic fix

**Settings group**:
One of the groups Copy, Paste and Sync settings move between Photos, as in Lightroom's Copy Settings dialog: White balance, Light, Color, Effects, Detail, Optics, Calibration, Retouch, Masks, Spot removal and Crop (`engine/src/paste.rs`). Pasting a group replaces those settings on the target and leaves the rest of its Recipe alone.
_Avoid_: Category, section

**Lens corrections**:
Distortion, vignetting and chromatic aberration fixes taken from the lens data the camera writes into each RAW (Sony ARWs today; `engine/src/lens.rs`), in place of Lightroom's lens profiles. The Recipe's `lensCorrections` is As camera (the default: what the camera did to its own JPEG), On or Off.
_Avoid_: Lens profile (we have no profile database)

**Preset**:
A one-click starting point from the Presets tool, as in Lightroom: a built-in set of settings in a group (Essentials, Portrait, Film, Black and white...; `engine/presets/builtin.json`), or an adaptive one that adds a Mask the engine finds by itself (Subject, Sky, People). Applied at an Amount (0 to 200%), its numbers add to the Photo's Recipe, so it stacks on Auto, and the Recipe remembers what it changed so it can come off again. Unlike a Look, it belongs to no Shoot and is copied into the Photo's Recipe. The AI suggests an order of Presets per Shoot ("Suggested for this Shoot"), a suggestion, never applied by itself.
_Avoid_: Filter, style
