# Lightroom parity: what's still missing

Written 2026-10-21 against main at the time, by reading the engine's tool layer, the Recipe, the prototype's screens and hotkeys, and the schema. It compares Argus with modern Lightroom (cloud and Classic) on the flows Greg actually uses, and ranks the gaps by his four goals: import a batch of about 100, cull it, fix the light and make people look good, and go deep on one Photo. Bugs and UX problems are out of scope here; the "Bug hunt and UX audit" thread owns those.

Constraints that shape the list: Mac only, no generative fill, no face or body reshaping, model licences Apache, MIT or BSD only (so no lensfun, which is CC-BY-SA, and no InsightFace or Depth Anything Base/Large weights, which are non-commercial).

## Where Argus already matches or beats Lightroom

Import from a card with New only, RAW+JPEG pairing, dated Shoot names and verified copies is on par. Culling is ahead: P/X/U flags that advance, 0-5 stars where the person always wins, Stacks for bursts, Compare (Lightroom's Survey), a Library-wide Picks view, and an AI that rates the Shoot and rejects broken frames. The Develop panel now covers Lightroom's Basic, Tone curve (parametric and point), Color mixer, Point color, Color grading, Effects, Detail (sharpening and manual noise reduction), Optics (CA, defringe, manual distortion and vignetting) and Calibration, plus WB presets and an eyedropper. Masks cover subject, background, sky, people with parts (face skin, body skin, eyes, teeth, hair), brush with erase, linear, radial and gradient, each invertible. Version history, batch undo, before/after on `\`, crop with aspect ratios and straighten, Looks shared across a Shoot, and export to JPEG, TIFF or original plus XMP are all there. Lightroom has nothing like Suggestions, the Agent or MCP.

## The gaps, ranked

Size is a rough guess: S is a day or less for one thread, M a few days, L a week or more or a new model.

| # | Gap | Goal it serves | Size | Notes |
|---|---|---|---|---|
| 1 | Copy, paste and sync settings | batch fix | S-M | ⌘⇧C / ⌘⇧V with a checklist of groups, ⌥⌘V paste previous, Sync across a selection |
| 2 | Lens corrections from the camera's own data | fix light, every Photo | M | Sony ARWs carry distortion, vignetting and CA coefficients; no lensfun needed |
| 3 | AI Denoise | fix light (indoor, night, high ISO) | L | NAFNet (MIT) or SCUNet (Apache-2.0) through the Analyzer seam |
| 4 | Spot heal and clone | make people look good, dust | M | Manual, non-generative; needs a scope call (see below) |
| 5 | Mask combine, Amount and range masks | deep edit | M | Add, subtract, intersect; luminance, color and depth range; per-Mask Amount |
| 6 | Saved develop presets across Shoots | batch fix | S | Looks belong to one Shoot today |
| 7 | Keywords, title, caption, Collections | find photos later | M | Naming rules for Keywords exist, storage does not |
| 8 | Search by content and metadata | find photos later | M | ⌘K only jumps; Lightroom cloud searches "dog on beach" |
| 9 | Export options | delivery | S each | HEIC (the spec promises it), output sharpening, color space, short edge or megapixels, watermark |
| 10 | Rotate 90° and flip | edge case | S-M | Only EXIF orientation today; masks are in upright coordinates, so it touches them |
| 11 | Virtual copies | deep edit | M | Color and B&W of one frame side by side; needs a schema step |
| 12 | Snapshots | deep edit | S | Named points in the History list Argus already keeps |
| 13 | Develop odds and ends | deep edit | S each | W eyedropper, J clipping overlay, crop resize handles, more Profiles |
| 14 | People view | find photos later, cull | M | Face recognition with SFace (Apache-2.0) on the YuNet faces we already detect |
| 15 | Lens Blur | deep edit (portraits) | L | Depth Anything V2 Small is Apache-2.0; the larger ones are not |
| 16 | Import extras | import | S each | Second copy to a backup drive, eject the card when done, Keywords on import |
| 17 | Color labels | cull | S | 6 to 9 in Classic; no sign Greg uses them, so last |

### 1. Copy, paste and sync settings

This is the one Lightroom move every batch editor uses and Argus doesn't have. Today a person can "Sync to Look" (fold the open Photo's settings into the Shoot's Look for every Pick), and the AI can "apply to the rest" from a Hero Photo. What's missing is the plain version: copy this Photo's settings, pick which groups come along (Light, Color, Crop, Masks, Retouch), and paste onto a selection, or ⌥⌘V to paste the previous Photo's settings. Lightroom cloud does exactly this from the Edit menu and the filmstrip. In the engine it's one tool, `paste_settings {from, photo_ids, groups}`, written as one batch so `undo_batch` reverts it; Masks need care because a person Mask with `person: 2` may not exist on the target, so AI Masks should carry their kind and hand-drawn ones their shapes. The UI is a small dialog and two hotkeys.

### 2. Lens corrections from the camera's own data

Lightroom turns on a lens profile for almost every Photo, and Argus can't use lensfun. There's a licence-free way around it. Sony writes its in-camera distortion, vignetting and chromatic aberration coefficients into every ARW (tags 0x7032, 0x7035 and 0x7037 in the SR2 data; rawler 0.8 already names the tags but its ARW decoder ignores them). Applying those gives the same correction the camera's own JPEG has, which is also what Greg's A7R V JPEGs look like today. This needs a careful read of the public tag documentation (exiftool's Sony tag tables) rather than darktable's GPL code, an Optics "Use camera corrections" switch on by default, and a golden test. Vignetting correction alone should brighten corners on most wide shots, which is "fix the light" for free. The Leica Q3's DNGs may carry the same data as DNG warp and vignette opcodes; that's unchecked.

### 3. AI Denoise

Lightroom's Denoise is the feature photographers cite most since 2023, and indoor and evening shots of people are where Greg's photos are noisiest. Argus has the classic luminance and color noise sliders but no learned denoiser. Candidates with acceptable licences are NAFNet (MIT) and SCUNet (Apache-2.0); both are image-domain, while Lightroom denoises the RAW mosaic before demosaic, which is better. It belongs behind the Analyzer seam (ADR 0013) as a specialist, cached like the demosaiced image so sliders stay fast, and it would need a speed check on the Mac. This is the biggest item on the list and the one most likely to show up in a side-by-side with Lightroom.

### 4. Spot heal and clone (needs a scope call)

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

## Suggested order

Copy, paste and sync settings (#1) and saved presets (#6) are cheap and land on the batch goal directly. Camera lens corrections (#2) improve every Photo without a model and without a licence question. AI Denoise (#3) is the biggest visible gap against Lightroom and the right next specialist model after faces. Spot heal (#4) waits on Greg's call about scope.
