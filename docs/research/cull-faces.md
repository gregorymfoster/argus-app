# Faces in the cull (2026-10-20)

Greg: "one of the biggest factors is the faces. Does each person's face look reasonable, like they wouldn't hate that photo of themselves?" Blur and exposure matter too, but faces looking good is huge. This note is what the cull now does about it and what the bench says.

## What the cull sees now

The eyes Analyzer (v2) keeps all 52 MediaPipe blendshape scores per face, plus how far the head is turned, the face's size and where it sits. `eyes::awkward` names what a person might dislike about a frame of themselves: a half-blink or shut eyes without a laugh, a mouth caught mid-word, looking down, a face turned away, a frown, or a large face that is clearly soft. A laugh with closed eyes is not a flaw, because Greg keeps those.

The Contact sheet caption describes only the main faces (at least half the size of the largest), so a crowd in the background doesn't drown the people the photo is about: `6 faces all look good`, or `3 faces, 1 looks good; 2nd from left: not smiling, half-blink`. When the main faces are small (under 15% of the long edge, as in group shots), close-ups of up to six of them are cut from the full preview and set under the frame, left to right, so the model can actually see each expression. At 768 px a face in a six-person group is about 50 pixels, too small for anyone to judge. The prompt (built-in and `bench/variants/stars-first.json`) now opens its judgment of people with the question Greg asked, and a 2-star frame includes one where someone looks off.

## What the bench says

All numbers are on the clean 2026-10-10 card (261 frames, 124 kept, mostly people on a boat), using 1600 px copies of the photos (`--cull-photos hd`). The repo's 640 px fixtures are too small for the face model to read most group shots.

Face reads alone, no VLM: a frame where every main face smiles was kept 58% of the time, a frame where nobody smiles 35%. Mean smile ranks keepers at AUC .60, which is better than Qwen's stars on this card (.54). A frame with a non-smiling half-blink was kept 32% of the time against 50% for the rest. Blinks by themselves barely matter (closed eyes in a laugh are kept as often as anything). Eye focus is misleading on group shots, because it compares the eyes with the sharpest part of the frame; frames with "soft" faces were kept more often, so only a large, clearly soft face counts now.

Claude (Opus, `--provider claude-code`), same photos:

| harness | star AUC | top-k | wrong rejects |
|---|---|---|---|
| before (939090a) | .66 | .58 | 0 |
| faces (this change) | .67 | .60 | 0 |

The sharper photos did more than the prompt: the same model scored .61 on the 640 px fixtures. One run each, so the .01 is within noise.

A deterministic star nudge (+1 when every main face smiles and none looks off) was tried offline on existing stars. On 2026-10-10 it lifted Qwen's overnight stars from .54 to .61 and Claude's from .67 to .69. On the four old Lightroom Shoots (640 px, edits leak) it helped two and hurt two (2024-08-10, where Greg kept 87%, dropped .06), so it is not in the engine. It can be scored from any run's stars and `bench/cull_features.py --photos hd --edge 1600` output without rerunning a model.

## Open

- Qwen on the new harness: run on Greg's Mac overnight with `--cull-photos <checkout>/out/cull-hd2/2026-10-10 --cull-edge 1600`, against the old commit on the same photos.
- More clean, people-heavy Shoots before the nudge or a learned face score goes in the engine.
- Both Claude runs returned only 3 Best Photos for 261 frames (expected about 26); check the chunked Best Photos request.
