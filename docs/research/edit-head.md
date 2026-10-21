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

`engine::edit_head` holds the features (one copy, which `argus edit-fit` also uses, so bench and app read the same numbers) and the shipped head, `engine/src/edit_head.json`: ridge-lite, the ridge without the camera's white balance or faces (Auto can't see those), fitted on all 104 Photos at alpha 300 by `python3 bench/edit_head.py export`. Held out it scores like the full ridge (10.16 and 10.28 on two splits, against Auto 11.66). Auto adds its offset on a RAW when the variant turns it on: `bench/variants/edit-head.json` (`{"auto": {"edit_head": true}}`), so `score.py --auto --variant bench/variants/edit-head.json` and `ARGUS_VARIANT` reach it. Engine and bench predictions agree to the rounding of the sliders (checked on six Photos with `edit-fit`'s `head` output). It stays off by default until Greg's taps on "edit-head-vs-auto" agree; then the default flips in `auto::Params`.

Next: learn from the person's own Library (their finished Recipes give the exact sliders, no fit needed) with the bench head as the prior, as the Taste head learns from flags.
