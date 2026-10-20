# Argus v1 spec

Status: draft for Greg's review (2026-10-17). Vocabulary follows [GLOSSARY.md](../GLOSSARY.md), and the big calls are recorded in [docs/adr](./adr).

## Problem Statement

After a shoot, Greg comes home with a hundred or so Sony RAW files. Getting from that card to a handful of great photos takes hours in Lightroom. He has to cull by eye, fix the light frame by frame, and make the people look good without making them look fake, and some photos deserve real time on their own.

AI could do most of this work, but Lightroom wasn't built to let it. Its edits are locked inside Adobe's catalog and cloud. Driving it from Claude or ChatGPT means clicking through the interface with accessibility hacks, and an edit can't be read, diffed or reasoned about as data. We proved that this is possible, and also how brittle it is.

## Solution

Argus is a Mac app built so that AI is a first-class editor. Point it at a folder or a memory card and it imports the batch. The AI looks at the whole Shoot before judging any one frame. It rates every Photo with stars, rejects only the plainly broken frames (missed focus, motion blur, closed eyes, ruined exposure), gives each a reason, and groups bursts into Stacks. Greg still does the cull himself, sorted by those stars, and picks the keepers (2026-10-20: no model picked his keepers well enough to trust with Picks).

It then applies one Look across the keepers, chosen with the whole Shoot in view so the set stays consistent, with per-photo Corrections for exposure, white balance and light on faces, and natural retouching so people look their best. For a special shot, he opens it and talks to it: "brighten her face, cool the sky." Argus builds Masks and local adjustments, and every change appears on ordinary sliders he can nudge by hand.

The AI can be a local open-source model through Ollama, Claude or OpenAI on Greg's own API key, or Claude or ChatGPT driving Argus headlessly through its MCP server. Every edit is a readable, versioned Recipe in one SQLite Library that spans all of Greg's shoots, and the originals are never touched. Finished photos export as JPEG or HEIC to a folder.

## User Stories

### Import

1. As a photographer, I want to import a folder of photos, so that Argus can work on a shoot I've already copied off the card.
2. As a photographer, I want to import straight from a memory card, so that I can start culling before copying anything by hand.
3. As a Sony shooter, I want ARW RAW files to import and render with accurate color, so that I don't need another app to decode them.
4. As a photographer, I want JPEG and HEIC files to import alongside RAWs, so that phone shots and camera shots live in one Library.
5. As a photographer, I want Argus to leave my original files untouched, so that I can never lose a photo to an edit.
6. As a photographer, I want thumbnails to appear within seconds of starting an import, so that I can begin looking before the batch finishes.
7. As a photographer, I want to see import progress for a large batch, so that I know when Argus is ready.
8. As a photographer, I want importing the same file twice to be harmless, so that I don't end up with duplicates.
8a. As a photographer, I want every shoot in one Library, so that I can search and browse across all of them.
8b. As a photographer, I want each import grouped as a Shoot, so that I can still work one outing at a time.
8c. As a photographer, I want Argus to keep working when a card or drive with originals is unplugged, so that I can browse and cull from previews.

### Cull

9. As a photographer, I want the AI to rate every Photo and set aside the broken ones, so that culling 100 photos starts from a sorted Shoot instead of a slog.
10. As a photographer, I want a short reason with each Verdict ("eyes closed", "missed focus", "best of 6"), so that I can trust or overrule it quickly.
11. As a photographer, I want near-identical shots grouped into Stacks, so that I can keep the best frame of a burst without comparing them all by hand.
12. As a photographer, I want the AI to rank the frames inside a Stack, so that the strongest one is on top.
12a. As a photographer, I want the AI to suggest a Lightroom star Rating for every Photo, judged against the whole Shoot, so that I can sort and filter by quality and not only by Pick or Reject.
12b. As a photographer, I want the AI to choose the Shoot's Best Photos (about one in ten, one per Stack, ranked), so that I see the handful worth editing first.
12c. As a photographer, I want to rate with the 0-5 keys and have my stars always win over the AI's, so that ratings work as they do in Lightroom.
13. As a photographer, I want to flip any Verdict with a single keystroke, so that my choice always wins.
14. As a photographer, I want to step through Photos full screen with the keyboard, so that a cull pass is fast.
15. As a photographer, I want to filter by Verdict, so that I can see only the Picks or double-check the Rejects.
16. As a photographer, I want Rejects kept but hidden rather than deleted, so that I can change my mind later.
17. As a photographer, I want culling to judge focus, blinks, expressions, exposure and composition, so that it catches what I would catch.
17a. As a photographer, I want the AI to compare each frame against the rest of the Shoot (not just its own Stack) before choosing the best of similar shots, so that it doesn't keep three near-identical portraits from different moments of the same scene.
17b. As a photographer, I want Pick and Reject flags that work like Lightroom's (P to pick, X to reject, U to unflag), so that my muscle memory carries over.
17c. As a photographer, I want to click a flag on any thumbnail to change it, so that I can cull with the mouse as easily as with the keyboard.
17d. As a photographer, I want flagging to jump to the next Photo, so that a cull pass never stops for a keypress.
17e. As a photographer, I want to select several Photos and flag them in one go, so that I can clear out a bad sequence quickly.

### Batch fix

18. As a photographer, I want to apply one Look across all my Picks, so that the set reads as one shoot.
19. As a photographer, I want the AI to propose a Look from a description ("warm, soft, film-like"), so that I don't have to build it slider by slider.
20. As a photographer, I want per-photo Corrections for exposure and white balance under the Look, so that frames shot in different light still match.
21. As a photographer, I want faces to get better light automatically, so that people aren't left in shadow.
22. As a photographer, I want natural retouching (even skin, blemish removal, brighter eyes), so that people look their best and still look like themselves.
23. As a photographer, I want retouching to never reshape faces or bodies, so that edits stay honest.
23a. As a photographer, I want the AI to keep color, tone and contrast consistent across a Shoot (matching skin tones and white balance between frames shot in different light), so that the set looks like one coherent story.
24. As a photographer, I want to tune the Look once and have it update across the whole set, so that iterating on style is cheap.
25. As a photographer, I want to see before and after for any Photo, so that I can judge what the AI did.
26. As a photographer, I want a batch to run in the background while I keep working, so that I'm never stuck waiting.

### Deep edit

27. As a photographer, I want to open one Photo and describe changes in plain language, so that complex edits take a sentence instead of twenty slider moves.
28. As a photographer, I want the AI to create Masks for subject, faces, sky and background, so that local edits land where I mean them.
29. As a photographer, I want to draw or refine a Mask by hand, so that I can fix the AI's selection when it misses.
30. As a photographer, I want every AI edit to appear as real slider values, so that I can see exactly what changed and fine-tune it.
31. As a photographer, I want slider changes to preview instantly at full screen, so that editing feels fluid.
32. As a photographer, I want to undo any change, mine or the AI's, so that experimenting is safe.
32a. As a photographer, I want the AI to apply batch edits directly without asking first, so that work moves fast.
32b. As a photographer, I want one undo to revert a whole AI batch, so that a bad batch costs one keystroke.
33. As a photographer, I want to browse a Photo's Version history, so that I can return to an earlier state.
34. As a photographer, I want the chat to remember earlier turns for this Photo, so that "a bit more" makes sense.

### Learning my taste

34a. As a photographer, I want Argus to record every time I overrule the AI (Verdict flips, slider nudges after an AI edit), so that it can learn my taste later.
34b. As a photographer, I want those records to stay on my Mac, so that my taste data is private.

### AI providers

35. As a user, I want to run Argus on a local open-source vision model through Ollama, so that I can edit without sending anything to the cloud.
36. As a user, I want Argus to pull a recommended model through Ollama for me, so that setup is one click once Ollama is installed.
37. As a user, I want to use Claude or OpenAI models with my own API key, so that I get the strongest judgment when I want it.
38. As a user, I want to switch Providers in settings, so that I can trade speed, quality and privacy.
39. As a user, I want cloud Providers to see only downsized previews, so that my original files never leave my Mac.
40. As a user, I want my API keys stored in the macOS Keychain, so that they aren't sitting in plain files.

### Headless (MCP)

41. As a Claude Desktop or Claude Code user, I want to connect Argus as an MCP server, so that I can cull and edit from a conversation using my subscription.
42. As a ChatGPT user, I want to connect Argus to ChatGPT, so that ChatGPT can drive it too.
43. As a headless user, I want the same tools the in-app Agent has (list, preview, read and write a Recipe, score, cull, batch apply, export), so that nothing is app-only.
44. As a headless user, I want changes made over MCP to show live in the app if it's open, so that I can watch and step in.
45. As a headless user, I want MCP to work only on my own Mac by default, so that nobody else can reach my Library.

### Export

46. As a photographer, I want to export Picks as JPEG or HEIC to a folder, so that I can share or post them anywhere.
47. As a photographer, I want to choose the size and quality on export, so that files fit where they're going.
48. As a photographer, I want exports to keep useful metadata and drop location if I choose, so that I control what I share.
49. As a photographer, I want to write Sidecar copies of Recipes on request, so that I can back up or diff edits outside Argus.

### Install and update

50. As a new user, I want to download Argus as a .dmg from a web page, so that installing takes a minute.
51. As a new user, I want clear steps for the one-time "Open Anyway" approval, so that the macOS warning doesn't scare me off.
52. As a user, I want Argus to update itself when a new version ships, so that I'm always on the latest without thinking about it.

## Implementation Decisions

The **Engine** is written in Rust. It owns RAW decode, the GPU edit pipeline, previews, export, the Library and on-device ML. It renders through wgpu, which uses Metal on the Mac. See ADR 0001.

The **edit pipeline** is a fixed chain of stages written once as WGSL shaders: white balance, exposure and tone, the Look's color grade, and then local adjustments through Masks. Native wgpu runs them for batches and export. WebGPU in the webview runs the same shaders for live slider previews, so dragging a slider never waits on the Engine.

The **UI** is TypeScript and React in a Tauri 2 shell. The visual direction is dark and image-first: neutral dark surfaces so color judgment isn't skewed, minimal chrome, and a calm side panel. The layout is three parts: a grid or filmstrip, the image, and a side panel that combines chat with sliders. The patterns Greg chose in the prototype are listed in docs/design.md. The UI is built from existing libraries (shadcn/ui on Radix primitives, Tailwind, cmdk for the ⌘K palette, TanStack Virtual for the grid) rather than hand-rolled components, and screens are kept thin over the tool layer so the UX can change quickly without touching the Engine. Previews cross from the Engine to the UI as display-sized images over a custom protocol, never as full-resolution buffers over JSON.

The **Library** is one folder Greg places wherever he likes, holding one SQLite file for all of his photos, like a Lightroom catalog. Card imports copy originals into it; folder imports can read originals where they live. It keeps previews of its own, so it still works when a drive is offline, and stores paths relative to itself so it can move to another disk or Mac (ADR 0011). Each import is recorded as a Shoot. Photos, Verdicts, Stacks, scores, face boxes and image embeddings are tables. A Verdict is Pick, Reject or Unflagged, as in Lightroom. Each Photo's Recipe is a versioned JSON document. Every change appends a Version tagged with who made it (person or AI) and which batch it belonged to. That gives us undo, batch undo and history. Overrides (a person changing an AI Verdict or AI slider value) are logged in their own table from day one, for tuning to Greg's taste later. Sidecars are optional copies. See ADR 0002.

The **Recipe** is a JSON document holding the Photo's global settings, a reference to its Look, its Corrections, and a list of local adjustments, each tied to a Mask. Masks are stored as descriptions of how they were made (a kind such as sky or one face, or hand-drawn dots and shapes), never as pixels. The models' raw Analysis of each Photo is cached next to its preview, and every render refines it against the image's edges (ADR 0012). The schema carries a version number from day one.

The **tool layer** is the single API everything goes through: list_photos, get_preview, get_contact_sheet, read_recipe, write_recipe, undo, score, set_verdict, build_stacks, apply_look, add_mask, export. set_verdict takes many Photos at once. The UI, the in-app Agent and the MCP server all call these same tools. See ADR 0003.

The **Agent** runs a tool-use loop against a pluggable Provider. Claude on Greg's API key is the default, and it's the model prompts and culling are tuned against first. Ollama is the private option. One option is a local vision model through Ollama's HTTP API, which can also pull models. The other is a cloud model (Claude or OpenAI) on the user's API key, with keys kept in the Keychain. Cloud Providers receive previews of about 1500px, never originals.

The Agent always works with **Shoot context**. Before it judges any Photo it reads the Shoot's Contact sheet (small previews of every Photo in time order, with Stacks, scores and face signals as text), so it knows the scenes, the people and the light across the whole outing. Choosing the best of similar shots happens at two levels. Inside a Stack it ranks frames on local signals. Across the Shoot it compares Stack winners that show the same scene or people (found through image embeddings), so it can keep the strongest moment and not three versions of it. Styling works the same way. A Look belongs to the Shoot, and Corrections aim every Pick at the same targets (skin tone, white balance, exposure on faces), measured against a reference Photo the Agent picks and names. Shoot context is cached per Shoot and refreshed when Verdicts or Recipes change, so per-Photo calls stay cheap.

The **ML models** run locally through ONNX Runtime with the CoreML backend. They cover scoring signals (sharpness, eyes open, exposure), face detection and landmarks, image embeddings for Stacks, and segmentation for Masks (for now three small embedded models run on the CPU through tract, ADR 0012). The vision LLM adds judgment on top ("best expression", the reason text). Both are needed. The local models are fast and run on every Photo, while the LLM is slower and sees fewer, smaller images.

**Retouching** is natural only. It works through face and skin Masks with frequency-separation smoothing, blemish healing, eye and teeth brightening, and face-weighted exposure. There is no reshaping and no warping.

The **MCP server** is local, runs over stdio for Claude Desktop and Claude Code, and also over HTTP on localhost. ChatGPT access goes through whichever route ChatGPT supports for local servers at the time, and that needs its own spike.

**Distribution** is an ad-hoc signed, un-notarized .dmg on GitHub Releases, with Tauri's updater verifying updates with Argus's own key. There is no Apple Developer account for now. See ADR 0004.

## Testing Decisions

A good test exercises behavior through the tool layer, the same seam the UI, Agent and MCP use, and checks outcomes rather than internals. For example: import this folder, call score, and expect the blink frame to be Rejected. Or write a Recipe, render it, and expect the export's mean luminance to rise.

The Engine gets golden-image tests. A small fixed set of sample photos is rendered through known Recipes and compared against reference outputs within a tolerance. This needs real Sony ARW files from Greg's camera and some JPEGs. The shared WGSL shaders get one test that renders the same Recipe through native wgpu and through WebGPU and checks they match.

Culling gets a labeled set: about 100 of Greg's photos, which he culls by hand once. Agreement between the AI's Verdicts and his becomes the culling metric we track. For edits, the reference is his existing Lightroom edits of the same shoots, compared side by side.

The MCP server gets a test that connects a real MCP client over stdio and runs one cull-and-export flow end to end.

The UI gets a few Playwright smoke tests against the Tauri dev build (open a Library, flip a Verdict, move a slider). Visual polish is judged by eye, not by tests.

There is no existing codebase, so there is no prior art yet. These tests become the prior art.

## Out of Scope

Out of scope for v1:

- iPhone, iPad and Windows
- the Mac App Store and notarization
- Apple Photos import and export
- importing Lightroom catalogs or edits
- generative fill and object removal
- face or body reshaping
- a model runtime built into Argus (Ollama does that job)
- cloud sync between machines
- accounts, payments and team features
- video

## Milestones

v1 ships in slices (Greg, 2026-10-17), and each one is usable on real shoots before the next starts.

M0, proof of the risky parts: Sony ARW decodes in the Rust Engine with color close to the camera's own JPEG look. The engine-to-UI preview path stays smooth at full screen. A Claude vision call gives useful Verdicts on a real shoot. The Claude Design mockups of the MVP screens are agreed before any UI code is written.

M1, import and cull: Import, Library, previews, Verdicts with reasons, Stacks, keyboard culling, and the MCP server with the cull tools.

M2, batch fix: Looks, Corrections, face-weighted light, batch undo, and export.

M3, people and deep edits: natural retouching, Masks, conversational editing on one Photo, and Version history.

## Definition of Done

v1 is done when Argus's edits on Greg's real shoots look as good as his own Lightroom edits of the same photos, judged side by side by Greg. Time taken doesn't count toward this bar. We still track how often he overrules the AI's Verdicts, as a quality signal.

## Further Notes

The riskiest unknowns get proven first, before any real UI work. There are three. Sony ARW decoding in Rust (rawler or LibRaw) must land close to the camera's own JPEG look in color. The engine-to-webview preview path must stay smooth at full screen. A local Ollama vision model must give culling judgments that are actually useful on an M4 MacBook.

One factual claim is unverified. As far as we know, Claude and ChatGPT subscriptions can't be used as API credentials inside third-party apps, so subscriptions only come in through the MCP route. That should be checked before the settings screen is designed.

Update-signing keys must be backed up somewhere safe. Losing them strands every installed copy on its current version.
