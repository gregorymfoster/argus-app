# Lightroom parity: what's still missing

Refreshed 2026-10-22 (ranking below). First written 2026-10-21 against main at the time, by reading the engine's tool layer, the Recipe, the prototype's screens and hotkeys, and the schema. It compares Argus with modern Lightroom (cloud and Classic) on the flows Greg actually uses, and ranks the gaps by his four goals: import a batch of about 100, cull it, fix the light and make people look good, and go deep on one Photo. Bugs and UX problems are out of scope here; the "UX sweep and polish" thread owns those (2026-10-22).

Constraints that shape the list: Mac only, no generative fill, no face or body reshaping, model licences Apache, MIT or BSD only (so no lensfun, which is CC-BY-SA, and no InsightFace or Depth Anything Base/Large weights, which are non-commercial).

## Where Argus already matches or beats Lightroom

Import from a card with New only, RAW+JPEG pairing, dated Shoot names and verified copies is on par. Culling is ahead: P/X/U flags that advance, 0-5 stars where the person always wins, Stacks for bursts, Compare (Lightroom's Survey), a Library-wide Picks view, and an AI that rates the Shoot and rejects broken frames. The Develop panel now covers Lightroom's Basic, Tone curve (parametric and point), Color mixer, Point color, Color grading, Effects, Detail (sharpening and manual noise reduction), Optics (CA, defringe, manual distortion and vignetting) and Calibration, plus WB presets and an eyedropper. Masks cover subject, background, sky, people with parts (face skin, body skin, eyes, teeth, hair), brush with erase, linear, radial and gradient, each invertible. Version history, batch undo, before/after on `\`, crop with aspect ratios and straighten, Looks shared across a Shoot, and export to JPEG, TIFF or original plus XMP are all there. Lightroom has nothing like Suggestions, the Agent or MCP.

## The gaps, ranked (refreshed 2026-10-22)

Since the first pass, five items have landed: copy, paste and sync settings, lens corrections from the camera's own data, AI Denoise (Core ML, about 28 s per 61 MP frame on the Mac), Heal and Clone Spots, and 57 built-in Presets with an AI order per Shoot. Stacks and the shared progress row also shipped. The list below is what's left on main at fd63fca, re-ranked for Greg's flow: import about 100 RAWs, cull, fix the light and the people, go deep on one frame, then deliver to a phone or Instagram. Two gaps are new to this pass (Auto straighten and Upright, a filter bar for the grid), and one item rose (own presets) because the built-in library made its absence obvious.

Size is a rough guess: S is a day or less for one thread, M a few days, L a week or more or a new model.

| # | Gap | Goal it serves | Size | Notes |
|---|---|---|---|---|
| 1 | Your own Presets (built 2026-10-22: `save_preset`, `delete_preset`, ⇧⌘N, Yours group) | batch fix | S | "Create Preset" from the open Photo with a group checklist, a Yours group above the built-ins, rename and delete. Shares the Settings groups `paste.rs` already has |
| 2 | Export for phones and the web (built 2026-10-22: HEIC on macOS through `sips`, short edge, megapixels, screen sharpening, Instagram and Phone presets; HEIC itself still needs a Mac run of `cargo test heic`; Display P3 left out because the pipeline clips to sRGB primaries early) | delivery | S each | HEIC (promised in the spec), Display P3, output sharpening for screen, short edge and megapixels, export presets, optional text watermark |
| 3 | Auto straighten and Upright (2026-10-22: a model-free detector failed on Greg's photos, see the results log; the manual Straighten tool shipped in Crop. Auto needs a learned horizon or vanishing-point model) | fix every Photo | M | Level a horizon or straighten verticals from detected lines; Auto proposes it, the Crop tool shows it. Not on the first list |
| 4 | Mask Amount, combine and range | fix people, deep edit | M | **Built 2026-10-22.** Amount per Mask (0 to 200); add, subtract and intersect components; Luminance and Color range Masks, alone or as components. Depth range waits on a depth model (Lens Blur row) |
| 5 | Keywords, captions and search | find photos later | M-L | AI Keywords and a caption from the work the cull already does, ⌘K searches Photos by words, SigLIP text-image embeddings for "dog on beach"; Collections as saved searches |
| 6 | Develop odds and ends | deep edit | S each | J clipping overlay, W eyedropper, crop resize handles (check), Profiles beyond Color and Monochrome |
| 7 | Snapshots and virtual copies | deep edit | S, M | Named points in History; a second Recipe for one original (schema step) |
| 8 | Brushed Heal and Content-Aware | make people look good | M | Spots are circles today; Lightroom's Remove paints any stroke |
| 9 | People view | find, cull | M | SFace (Apache-2.0) on the YuNet faces we have; "everyone's eyes open" needs who is who |
| 10 | Lens Blur | deep edit (portraits) | L | Depth Anything V2 Small (Apache-2.0); also a depth range Mask for #4 |
| 11 | Rotate 90° and flip | edge case | S-M | Touches Masks and Spots, which live in upright coordinates |
| 12 | Grid filter bar | cull, find | S | Lightroom's flag, star, camera, lens and ISO filters in one row; Argus has sort and a few views |
| 13 | Import extras | import | S each | Second copy to a backup drive, eject the card when done |
| 14 | Color labels | cull | S | No sign Greg uses them |

### Why this order

Own Presets come first because they are a day of work and Greg now sees 57 built-ins and no way to save his own; the edit head learns his taste in the background, but a named look he can reach for is the Lightroom habit. Export comes next because every finished Shoot ends there, and JPEG at sRGB with no output sharpening looks flatter on an iPhone than Lightroom's export does. Auto straighten is the one new item: tilted horizons on boat and travel frames are the most common fix after light, Lightroom's Upright does it in one click, and a line detector needs no model. Masks are the biggest win for "make people look good", since Amount and subtract are how photographers stop a face brighten from spilling onto the background. Keywords and search matter more as the Library fills with a year of Shoots, and the AI angle (captions from work Argus already does) is where Argus can beat Lightroom rather than match it.

### Notes from the first pass (2026-10-21)

### 1. Copy, paste and sync settings

Built 2026-10-21 (`engine/src/paste.rs`, the `paste_settings` tool, ⇧⌘C ⇧⌘V ⌥⌘V ⇧⌘S).

This is the one Lightroom move every batch editor uses and Argus doesn't have. Today a person can "Sync to Look" (fold the open Photo's settings into the Shoot's Look for every Pick), and the AI can "apply to the rest" from a Hero Photo. What's missing is the plain version: copy this Photo's settings, pick which groups come along (Light, Color, Crop, Masks, Retouch), and paste onto a selection, or ⌥⌘V to paste the previous Photo's settings. Lightroom cloud does exactly this from the Edit menu and the filmstrip. In the engine it's one tool, `paste_settings {from, photo_ids, groups}`, written as one batch so `undo_batch` reverts it; Masks need care because a person Mask with `person: 2` may not exist on the target, so AI Masks should carry their kind and hand-drawn ones their shapes. The UI is a small dialog and two hotkeys.

### 2. Lens corrections from the camera's own data

Built 2026-10-21 (`engine/src/lens.rs`, `docs/research/lens-corrections.md`): As camera by default, On and Off in Optics.

Lightroom turns on a lens profile for almost every Photo, and Argus can't use lensfun. There's a licence-free way around it. Sony writes its in-camera distortion, vignetting and chromatic aberration coefficients into every ARW (tags 0x7032, 0x7035 and 0x7037 in the SR2 data; rawler 0.8 already names the tags but its ARW decoder ignores them). Applying those gives the same correction the camera's own JPEG has, which is also what Greg's A7R V JPEGs look like today. This needs a careful read of the public tag documentation (exiftool's Sony tag tables) rather than darktable's GPL code, an Optics "Use camera corrections" switch on by default, and a golden test. Vignetting correction alone should brighten corners on most wide shots, which is "fix the light" for free. The Leica Q3's DNGs may carry the same data as DNG warp and vignette opcodes; that's unchecked.

### 3. AI Denoise

Built 2026-10-21 (`engine/src/denoise.rs`): SCUNet's real-noise model in tract, a cached full-size base per RAW computed by a Queue job, and Lightroom's Amount slider at the top of Detail. Open: speed on the Mac (about 35 min per 61 MP frame on a 4-core cloud CPU), which may call for a GPU runtime.

Lightroom's Denoise is the feature photographers cite most since 2023, and indoor and evening shots of people are where Greg's photos are noisiest. Argus has the classic luminance and color noise sliders but no learned denoiser. Candidates with acceptable licences are NAFNet (MIT) and SCUNet (Apache-2.0); both are image-domain, while Lightroom denoises the RAW mosaic before demosaic, which is better. It belongs behind the Analyzer seam (ADR 0013) as a specialist, cached like the demosaiced image so sliders stay fast, and it would need a speed check on the Mac. This is the biggest item on the list and the one most likely to show up in a side-by-side with Lightroom.

### 4. Spot heal and clone (built 2026-10-21)

Greg said build it (2026-10-21), and the spec now lists Heal and Clone Spots as in scope, as long as they only copy pixels. Built: the Recipe's `spots`, `engine/src/heal.rs` (Heal matches the source to the spot's edge with a harmonic blend; Clone copies), an automatic source pick, the `add_spot` tools for the Agent, and the Heal tool (H) in Edit. Still to do: brushed strokes (Lightroom's Remove paints any shape; Argus has circles only) and Content-Aware.

Lightroom's Remove tool has three modes: Generative (out for us), Content-Aware and Heal/Clone. The last two copy pixels from elsewhere in the same photo, with no generation, and they're how photographers take out a stray hair, a blemish the skin smoothing misses, a sensor dust spot or a distracting sign. The spec lists "object removal" as out of scope alongside generative fill, so this needs Greg's word. A narrow version (brush a spot, Argus picks a nearby source patch, blends it; or the person drags the source) is non-generative and fits "natural retouching". It would live in the Recipe as a list of spots, like Lightroom's.

### 5. Mask combine, Amount and range masks

Each Mask is one shape today, inverted or not. Lightroom lets a Mask be built from parts: Subject minus a brushed area, a sky intersected with a linear gradient, or a Luminance or Color range inside a radial. Without that, the Agent and the person both work around it with extra Masks. Three pieces: a Mask becomes a list of components with add, subtract and intersect; range components (luminance, color, and depth once a depth model exists); and the Amount slider per Mask that Lightroom added in 2023, already noted as missing in the Edit screen notes. The Recipe stays the same JSON, with one more level.

### 6. Saved develop presets across Shoots

Looks live in the `looks` table keyed by Shoot, and Presets are the built-in list in `prototype/src/domain/presets.ts`. A person can't save "my wedding look" once and reach for it on the next Shoot, which Lightroom users do constantly (and many buy preset packs). A Library-level `presets` list (name, settings, which groups) with Create preset in the Presets panel is small, and pairs naturally with #1 since both apply a subset of settings.

### 7 and 8. Keywords, Collections and search

Argus organizes by Shoot only. There are no Keywords (though `engine::naming` already has the hierarchical rules and the XMP layout), no titles or captions, no manual or smart Collections, and ⌘K jumps to Shoots and commands but doesn't search Photos. For one Shoot at a time this doesn't hurt; it starts to once the Library has a year of Greg's work in it. The AI angle is strong here: the cull already reads every Photo, and Facts already carry a caption, so Keywords and a searchable description could come for free from work Argus already does. For "dog on beach" search without a model call per query, SigLIP (Apache-2.0) gives text and image embeddings that share a space; the MediaPipe embedder we have is image-only. Smart Collections are then saved searches.

### 9. Export options

The spec promises HEIC and only JPEG and TIFF exist. Lightroom's Export dialog also has output sharpening (screen, matte, glossy at low, standard, high), color space (sRGB, Display P3, Adobe RGB), sizing by short edge, long edge or megapixels, a text watermark, and Keywords and title in the metadata. Each is small; output sharpening for screen and the P3 color space matter most for how exports look on an iPhone or Instagram.

### 10 to 13. Smaller develop and library gaps

Rotate 90° and flip don't exist; Argus trusts the EXIF orientation, which is right for Sony but wrong for the odd frame shot straight down. It touches Masks, since their coordinates are on the upright image. Virtual copies (one original, two Recipes) need a schema step and grid support for a second tile. Snapshots are named entries in the Version history Argus already keeps. In Develop: the W hotkey for the eyedropper, J for the clipping overlay (the engine measures clipping for Auto, but nothing shows it), resize handles on the crop box, and Profiles beyond Color and Monochrome (Lightroom's Adobe Portrait, Landscape and Vivid, plus creative profiles with an Amount).

### 14 and 15. People view and Lens Blur

Lightroom groups faces into People. Argus detects faces with YuNet already; SFace (Apache-2.0, from the same OpenCV Zoo) adds the recognition embedding, and clustering them gives People. Besides finding photos of someone, it helps the cull: "best frame where everyone's eyes are open" needs to know who is who. Lens Blur needs depth; Depth Anything V2 Small is Apache-2.0 while Base and Large are non-commercial, and Apple's Depth Pro has its own licence, so Small is the only option today. It would also unlock a depth range Mask (#5).

### 16 and 17. Import extras and color labels

Lightroom Classic can make a second copy to a backup drive during import, eject the card when done, and apply Keywords and a preset on the way in. Argus applies Auto and queues the cull on import, which covers the preset case. Color labels (6 to 9) are the last thing a Lightroom user might miss; nothing so far suggests Greg uses them.

## Deliberately not on the list

Generative Remove and Generative Expand, face and body reshaping, Lightroom catalog import, mobile and Windows, and video are out of scope in the spec. HDR and panorama merge, Enhance and Super Resolution, Print, Slideshow, Book, Map, Web, tethered capture, soft proofing and publish services don't serve Greg's four goals; HDR merge is the only one that might, for bracketed interiors, and it can wait until he asks.

## First-pass order (2026-10-21)

Copy, paste and sync settings, camera lens corrections, AI Denoise and Spot heal were the first four; all four landed the same day. The section numbers above are the first pass's, kept for history; the table is the current ranking.
