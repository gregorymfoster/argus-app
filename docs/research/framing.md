# Crop & straighten: the AI's straighten and crop for a Shoot

Greg's ask (2026-11-01): "a way to propose and auto-crop/straighten all the photos based on AI, and then accept that as well." It ships as Crop & straighten in Edit (⌥⌘R, `prototype/src/ui/Framing.tsx`), laid out like Apply look: the Shoot's frames that have something to fix, drawn as they'd look, untick to leave one out, Apply as one Undo that changes only the crop. Frames already cropped, by hand or by an earlier pass, are never touched. Engine: `engine/src/framing/` (tools `propose_framing`, `apply_framing`; HTTP `framing_preview`).

How Greg frames (from `recompose.md` and the interim pairs): he straightens 10 of 3,083 frames with Lightroom settings (0.3%) and crops 108 (3.5%), nearly always a 3:2 punch-in. So the useful version proposes little, and is right when it does.

## Straighten (2026-11-01)

The October line detector (`lightroom-parity.md`, results log 2026-10-22) was scored against Greg's finished edits as if they were level, and "failed". Looking at its turns by eye says otherwise: his edits keep real tilts (window frames and door jambs visibly lean, and the turned version is plumb). Scored instead against its own answer on the original (a copy tilted by ±3° should come back 3° off what it says about the original), on the 152 edit-bench JPEGs (`bench/straighten_bench.py`):

| Gates (objects, balance, reach, horizon, max tilt, misfit) | spoke on | tilted copies right | wrong |
|---|---|---|---|
| October's strict 4, 0.25, 0.8, 0.6, 5, 0.6 | 6 of 152 | 9 | 3 |
| shipped 2, 0.1, 0.4, 0.4, 6, 1.0 | 24 | 40 | 1 |
| looser 1, 0, 0.3, 0.3, 6, 1.5 | 56 | 82 | 12 |

The scene model (DeepLab on ADE20K, already in the app for the sky Mask) now decides which edges vote: verticals only on architecture, never on people; a horizon only where sky meets water. On this set it changed little against the plain detector at the same gates, but it keeps receding table edges and people out by design. Turns on the originals with the shipped gates, judged by eye on a grid: of 16, about 10 put a crooked room or horizon right, 2 were under 0.7° (too small to show), and the 3 over 3.5° were misreads, so proposals are kept to 0.7° to 3.5°. That leaves about 11 proposals on 152 frames, 10 of them right. Most of his frames (people outdoors, close portraits) have nothing to level by, and get nothing.

## Crop (2026-11-01)

A punch-in at the frame's own shape around the main faces and the whole bodies under them (`compose.rs`), only when it trims at least 12% of each side. `bench/crop_bench.py` runs it on the edit bench's full frames: it cropped 36 of the 136 frames Greg left whole and 1 of the 3 he cropped. The proposals look reasonable (a person small in a wide street or hangar), but Greg leaves those wide. So crops are behind "Straighten + crop" in the popover and `crop: true` in the tools, off by default.

## Next

A bigger crop bench needs the Mac: the 8,517 pairs with Lightroom crop rectangles and the 384 px plain renders there (`recompose_bench.py`'s data), scored for whether he cropped at all and IoU when he did. Greg's taps in the popover (applied and left-out frames, `framing_applied` in the Usage log) are the real judge of the straightens.
