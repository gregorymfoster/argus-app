# Edit heuristics and an edit head from Greg's own edits

2026-10-21. Greg: "heuristics for culling seem to be helping, can we try a similar technique for edits?" The cull's Taste head (`taste-head.md`) is cheap signals plus a small head fitted to his own choices, tested on frames it never saw. This applies the same recipe to the Basic sliders.

## Setup

`bench/edit_head.py` over the 104 bench edits whose RAW reaches the cloud (`fixtures/bench/edit`, mostly his favorites from 2017 and 2022 to 2026, 70 capture days). Targets are his finished Lightroom edits (640 px previews); nothing about them is a feature, so they can't leak.

`argus edit-fit` (`cli/src/edit_fit.rs`, a dev tool) decodes each RAW once at the edit's size and renders hundreds of Recipes in a few milliseconds each. It reports cheap facts about the unedited render (brightness percentiles, clipping, colour cast, saturation, skin-coloured and blue-sky share, as-shot white balance), Auto's sliders, and the **fit**: the ten Basic sliders (Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Temp, Tint, Vibrance, Saturation) that bring the render closest to his edit, by coordinate descent from Auto on the bench's dE. Its dE matches `score.py` to about 0.05. The embedding and faces come from a scratch Library over the same RAWs (`analyze_photo`).

Every method starts from Auto, as the app does, and predicts the offset his edit needs. Folds are whole capture days (a day shares light and mood, so a random split leaks); every choice (rule, neighbours, ridge strength, how far to trust the head) is made on inner folds of the training days only, then the held-out predictions are rendered and scored. Three different splits of the days.

## Numbers (dE to Greg's edit, 104 Photos, lower is closer)

| | split 0 | split 1 | split 2 | closer than Auto (split 0) |
|---|---|---|---|---|
| no edit | 12.08 | | | 35 |
| Auto (today) | 11.66 | 11.66 | 11.66 | |
| Style offset (median offset of his other days) | 10.41 | 10.37 | 10.42 | 68 |
| per-camera offset | 10.38 | | | 69 |
| one-feature rules per slider | 10.48 | 10.45 | 10.52 | 70 |
| ridge on cheap facts | 10.12 | 10.27 | 10.15 | 73 |
| nearest past edits, embedding | 10.32 | 10.73 | 10.46 | 67 |
| nearest past edits, cheap facts | 10.27 | 10.19 | 10.26 | 72 |
| **blend** (ridge + embedding neighbours) | **10.05** | 10.28 | 10.12 | 71 |
| fit (best possible sliders, ceiling) | 5.15 | | | 104 |

Claude's Natural take on 92 of the same Photos (Favorites runs, cea6425, scored at 2048 px) sits level with Auto there (11.56 vs 11.57), while the head is about 1.5 dE closer than Auto on the same Photos.

Ridge trained on all 104 says what his offset from Auto is, on average: Contrast +37, Temp +19 (warmer), Blacks +18, Whites +16, Highlights +10, Shadows -9, Vibrance -10, Tint +7, Exposure -0.3. The features add a little on top (more contrast where there's less sky, less vibrance on blue-sky scenes, Whites follow how much Auto pulled Highlights). Tone carries most of the gain (tone-only 10.61), white balance next (11.24), colour alone nothing (11.71).

## What it means

- Most of the gain is a constant: Greg's edits are consistently punchier, warmer and a touch darker than Argus's develop plus Auto. That is the Style profile idea (`style.rs`), learned here from finished edits instead of corrections to takes, and it matches the profile's stand-in direction (contrast up).
- Per-Photo heads add a further 0.2 to 0.4 dE on average. Nearest-neighbour on the embedding alone is unstable across splits; blending it with ridge is the steadiest.
- These edits are not timid: the head moves well away from Auto, toward him. By eye (contact sheets of eight random Photos) the head versions read closer to his edits, sometimes a bit dark on bright scenes.
- The ceiling (5.2) is far below every method: per-Photo sliders could get much closer, and nothing cheap predicts them well yet. The earlier finding that brightness stats don't predict his exposure (`argus-edit-tuning`) still holds: the exposure rule stays a constant.

## Caveats

- dE alone isn't the judge. The held-out renders (no edit, Auto, Style offset, Edit head) at 2048 px are a review-page run (`/mnt/project-files/argus-bench/edit-head/`), for Greg's taps.
- The fit is a proxy for his Lightroom sliders, fitted at 640 px; it absorbs crops, clarity and masks he used with global sliders.
- 104 Photos is small, and many are favorites (heavily edited).

## Reproduce

```
cargo build --profile fast -p argus-cli
python3 bench/edit_head.py features        # ~4 min: out/edit-head/fit.json + emb.npy
python3 bench/edit_head.py eval --seed 0   # ~4 min per split
python3 bench/edit_head.py page --seed 0   # held-out renders at 2048 px as a score.py result
```

## In the engine (Greg chose "Build it now", 2026-10-21)

`engine::edit_head` holds the features (one copy, which `argus edit-fit` also uses, so bench and app read the same numbers) and the shipped head, `engine/src/edit_head.json`: ridge-lite, the ridge without the camera's white balance or faces (Auto can't see those), fitted on all 104 Photos at alpha 300 by `python3 bench/edit_head.py export`. Held out it scores like the full ridge (10.16 and 10.28 on two splits, against Auto 11.66). Auto adds its offset on a RAW when the variant turns it on: `bench/variants/edit-head.json` (`{"auto": {"edit_head": true}}`), so `score.py --auto --variant bench/variants/edit-head.json` and `ARGUS_VARIANT` reach it. Engine and bench predictions agree to the rounding of the sliders (checked on six Photos with `edit-fit`'s `head` output). It stayed off by default until Greg's taps on "edit-head-vs-auto" agreed (they did on 2026-10-22, see below).

Next: learn from the person's own Library (their finished Recipes give the exact sliders, no fit needed) with the bench head as the prior, as the Taste head learns from flags.

## Mac, 148 Photos, 8 splits (2026-10-22)

Overnight on Greg's Mac, where 148 bench RAWs are reachable (97 days), seeds 0-7: Auto 12.13, offset 11.04, ridge-lite 10.76 (10.57-10.95), ridge 10.74, knn-feat 10.70, blend 10.68 (10.63-10.77), fit 5.58. Every head beats Auto by ~1.4 dE on every split. That run's divide-by-zero warnings came from Auto's always-zero RAW sliders as features; they now z-score to 0 (`spread()`), with identical scores.

## Scene-aware head and the Library's own head (2026-10-22)

Greg wanted the head to read the scene instead of adding one warm, contrasty shift to everything, to keep learning from his edits in the app, and to try local face and sky tweaks with the Masks we have. `bench/edit_scene.py` (same 104 cloud Photos, same day folds, three splits, choices on inner folds).

**Scene facts.** `argus edit-fit` with `"scene": true` runs the Mask models on the unedited render (`engine::edit_head::scene_features`): people and sky share, face count and area, and how bright the faces, the people and the sky sit against the whole frame, plus `backlit` (background minus people). Greg's favorites are mostly people: 96 of 104 have a face, 23 have real sky.

**Local fit.** With `"fit_local": true` it also finds the AI-Mask exposures (faces, subject, background, sky) and sky saturation that bring the best global sliders closer, scored on the whole frame plus the faces and the sky on their own (a face is too small a share of the frame for the whole-frame dE to see). `face_scores` and `sky_scores` report dE over only those blocks. His typical local move is small: subject +0.07 EV (median), background -0.03, faces 0, sky -0.03. The Adaptive layer's people lift (+0.2 subject, -0.12 background) points the same way, a bit stronger.

| dE to Greg's edit (mean of 3 splits) | whole frame | faces (96) | sky (47) |
|---|---|---|---|
| Auto | 11.66 | 12.12 | 10.82 |
| shipped head (ridge-lite, before today) | 10.20 | | |
| ridge-lite, features clipped at 3 SD | 10.04 | | |
| **scene head** (ridge-scene, clipped) | **9.93** | 10.24 | 9.74 |
| scene head with interactions | 9.93 | 10.25 | 9.85 |
| scene head + predicted face/sky tweaks | 9.95 | 10.07 | 9.69 |
| nearest past edits by scene facts | 10.22 | | |
| soft retrieval, MobileNet embedding (top 5, softmax) | 10.49 | | |
| best global sliders (ceiling) | 5.15 | 6.01 | 6.01 |
| best global sliders + Mask tweaks | 5.20 | 4.95 | 5.48 |

What it says:

- Reading the scene helps, a little: 0.11 dE on top of the clipped head, on every split and every scene type (faces, backlit people, sky, dark). Clipping each feature at 3 SD helped as much (0.16): one red-lit room had pushed the old head to -1.4 EV and -77 Vibrance. Together 10.20 to 9.93. What the head learned reads sensibly: brighter faces get a touch less exposure, more faces get warmer, skies get less contrast and less warming, more vibrance.
- Local tweaks don't move the whole-frame number but bring faces about 0.15 closer. The ceiling says Greg does work faces locally (6.0 to 5.0 on faces with the best tweaks), but a regression from these facts recovers only a sliver of it.
- Soft retrieval of nearest past edits (the InstantRetouch idea) loses on the MobileNet embedding we have; a stronger embedding (SigLIP 2, PE-Core) needs the Mac, since the cloud can't reach Hugging Face.
- The gap to the ceiling (about 10 against 5) is still mostly per-Photo choices that cheap facts can't see.

**In the engine.** `engine/src/edit_head.json` is now the scene head (ridge-scene, alpha 300, all 104 Photos, `bench/edit_scene.py export`); engine and bench predictions agree to slider rounding. Auto reads the Photo's Analysis from its Facts when the head is on (`auto::head_input`); without one the scene facts read as average. On by default since 2026-10-22 (below).

**The Library's own head.** Every export queues `learn_edits`, which learns a head on top of the shipped one from the RAWs the person has exported (`edit_head::learn_library`, docs/usage-log.md): their average miss shifts the intercept by n / (n + 4), and a ridge at alpha 300 nudges the weights. Auto uses it when the head is on. The `learn_edit_head` tool runs it now. Untested on real use yet: Greg's Mac Library has few exports.

**Review page.** `bench/edit_scene.py page --seed 0` renders split 0's held-out predictions at 2048 px: no edit, Auto, Edit head (clipped ridge-lite), Scene head, Scene head + faces (on the page since v46). Scene head vs Edit head and faces vs no faces are too close to see on every photo (median dE 1.6 and 1.4 at 192 px, none over 5, nor in any face-sized patch), so they're decided on the numbers: the scene head ships as the head. Greg's taps decide only Edit head vs Auto, which this run's Edit head column now feeds.

Reproduce: `python3 bench/edit_scene.py features` (5 min), `eval --seed N` (about 15 min each on 4 cores), `export`, `page`.

## 2026-10-22 · On by default

Greg's taps on the review page, Scene head (held out) against plain Auto: the head 9, Auto 2, on 11 clear pairs (dE 5 or more). Against the Natural Suggestion, a model take, it tied: head 29, Natural 28, can't tell 4. Small sample, but it agrees with the held-out dE (9.93 against 11.66), so `auto::Params.edit_head` now defaults to true. Plain Auto stays one variant away (`bench/variants/no-edit-head.json`), and the review page keeps asking edit-head-vs-auto, so a turn in his taps would show.


## 2026-10-22 · Toward the whole edit: learned scene types and Greg's XMPs

Greg asked for a "god tier" edit model: sort the photo into kinds first (learned, not a fixed list), then edit per kind, and cover crop, vignette, noise and the rest.

**Learned scene types on today's bench.** `bench/edit_scene.py eval --only ridge-scene,moe-emb,moe-scene` adds a mixture of experts: k-means on an embedding (MobileNet, `moe-emb`) or on the scene facts (`moe-scene`) finds the kinds, a soft gate mixes one expert per kind, and each expert is a ridge on the global head's residual with its intercept shrunk n/(n+4). Held out by day on the 100 bench Photos (9 since removed), mean of 3 splits: scene head 10.09, moe-emb 10.07, moe-scene 10.11, Auto 11.79. A tie: 100 Photos can't feed a gate. The pipeline is ready for the pairs.

**What 2,704 of Greg's XMPs say** (first pairs batch, `bench/crs.py` maps crs to Recipe units). His signature is nearly constant: Highlights -63 (sd 17), Shadows +47 (sd 15), Vibrance +14, a magenta Tint +12, sharpening at the default 40, Adobe Standard on 93%. Beyond Basic he uses HSL on 60% of photos, a vignette on 53% (about -2 to -9), luminance noise reduction on 32% (up to 18), Clarity 24%, Texture 16%, colour grading 10% (midtones) and 7% (shadows), Dehaze 5%, crop only 3%, masks 1%. The point curve on 89% is Lightroom's default Medium Contrast, not an edit. Discrete edit styles explain little: k-means on his sliders explains 22% of their variance at 3 styles and 40% at 8, and capture year alone 15% (2023 has its own look: brighter Whites, Blacks lifted, more Clarity and vignette). So his edits are one style plus continuous per-photo tweaks, and the model should be a continuous predictor from the image (gated or not), weighting recent years, rather than a classifier into named kinds.

**Next.** Pairs need their RAWs to train on (510 2017 RAWs on the drive now; 2022-2026 wait on Greg's Lightroom export). Tone and colour train on Argus-space targets fitted to his finished JPEG (`edit-fit`), since Lightroom's and Argus's renders differ; vignette, noise, sharpening, grain and crop train on the crs values directly.
