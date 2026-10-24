# Auto-edit research: shoot style

2026-10-23. Greg asked for an open-ended research push on the best auto-edit system ("act like an AI researcher"), then added the product frame that became the main track: "as a product it's a game of picking a style for the shoot and then the AI applying that style to all the photos in the shoot in just the right way, leveraging human taste, but then giving them leverage in implementations."

So the benchmark here is: given k of Greg's own edits from a Shoot, predict the rest of that Shoot. It sits beside the earlier question in `edit-head.md` (predict an edit from the Photo alone), where everything since the first scene head has stalled around dE 10.

## Data and how it's scored

The RAW and edit pairs (`bench/pair_features.py`'s `fit.jsonl`, copied from the Mac: 5,152 pairs, 4,860 fitted, 147 capture days, median 14 Photos a day). A capture day stands in for a Shoot. Targets are the offsets from plain Auto to the best Basic sliders for his finished JPEG (`fit`), as in the head's own training.

For every Photo on a held-out day, k other Photos of that day are the anchors (their finished edits are known) and the rest of the day is predicted. Anchors are drawn at random three times. Global parts of each model train on other days only, using the shared day folds (`bench/splits.py`); hyperparameters are picked on inner folds; fold 0 was scored once at the end.

Slider distance turned out to be a misleading screen. The fitted sliders are degenerate (Blacks +100 against Whites -100 can render like zero and zero), so z-scored slider error punished harmless moves and made plain sync look worse than the head. The screen therefore scores a linearised render error, sqrt(fit dE² + dᵀ M d), where M is the bench's mean slider-to-render matrix from `edit-fit`'s `sensitivity` (139 Mac bench Photos). The deciding check renders every prediction: 67 cloud bench RAWs whose day has at least three other fitted pairs, anchors drawn from those pairs (never the Photo itself), scored against Greg's edit by `edit-fit` with the same dE as `score.py`.

## Methods

- **head**: today's recipe (ridge on cheap and scene facts plus shooting EXIF), no anchors.
- **sync**: Lightroom's Sync, the anchors' sliders copied as they are (their mean). This is also what "apply to the rest" starts from today, before the model's Corrections.
- **sync-auto**: the anchors' offset from Auto, added to this Photo's own Auto.
- **head+shoot**: the head plus the anchors' mean miss from the head, shrunk k / (k + 2).
- **within**: a mixed model. Each Shoot has its own intercept (its style), and a ridge learnt only from differences inside a Shoot says how Greg's edit follows the scene from frame to frame. The Shoot's intercept comes from the anchors, shrunk per slider by how much Shoots really differ against frame-to-frame noise.
- **within-sim**: the same, with anchors weighted by how close their scene facts are to this Photo (Gaussian on the z-scored facts, width 0.5).
- **within-gbm**, **within-emb**: gradient-boosted trees for the within-Shoot part, or the MobileNet embedding added to it.

## Numbers

Rendered, 67 cloud bench Photos (mean dE to Greg's edit; "vs head" is the mean paired difference ± 2 SE):

| | k = 1 | k = 3 |
|---|---|---|
| plain Auto | 11.15 | |
| head (no anchors) | 9.16 | |
| sync | 10.03 (+0.88 ± 0.91) | 8.48 (-0.67 ± 0.77) |
| sync-auto | 10.17 | 8.60 |
| head+shoot | 8.81 (-0.35 ± 0.31) | 8.23 (-0.92 ± 0.47) |
| within | 8.71 (-0.45 ± 0.38) | 8.12 (-1.04 ± 0.56) |
| **within-sim** | 8.71 | **7.90 (-1.25 ± 0.60)**, closer on 44 of 67 |
| within-gbm | 8.78 | 8.25 |
| best sliders (ceiling) | 4.70 | |

The 22 of those Photos on test-fold days tell the same story (head 9.33, sync k=3 8.18, within-sim k=3 7.64). The slider screen on the test fold, scored once (linearised dE, 4,860 pairs): head 9.37; at k = 1 sync 11.01, within 9.02; at k = 3 sync 9.54, head+shoot 8.62, within-sim 8.44.

More anchors keep helping (slider screen, folds 1-4, within-sim against sync): k = 2 8.84 vs 9.94, k = 3 8.58 vs 9.53, k = 5 8.30 vs 9.20, k = 8 8.07 vs 9.00. The head stays at 9.9.

Which anchors the person edits matters at k = 1. The most typical frame of the day (nearest the day's average facts) as the one anchor gives 8.85, about what three random anchors give (8.79). At k = 3 the choice matters less: with similarity weighting, random and spread-out anchors both give 8.58.

## What it says

Greg's edits are mostly a per-Shoot style plus small per-frame adjustments. In his Lightroom settings, 62% of his Photos have Basic sliders identical to another Photo's on the same day (he syncs), and only 30-40% of the spread of each slider is inside a day. Exposure is the exception: 68% of its spread is within the day, because he sets it frame by frame.

Plain sync from one hero Photo is worse than not using the hero at all (10.03 against the head's 9.16), because frames shot in other light need other sliders. A model that keeps the hero's style but carries over only the part that should carry (its intercept), and lets the scene move the rest, gets 8.71. With three heroes it gets 7.90: a third of the way from the head to the best-slider ceiling, the first big step since the scene head.

Weighting the anchors by similarity is the best single trick once there are three or more. Trees, kernel ridge and the embedding added nothing in this regime: the gain comes from the structure of the problem (Shoot effect plus within-Shoot response), not from model capacity.

## Also tried (no-anchor head, slider screen, folds 1-4, linearised dE)

| | dE |
|---|---|
| head (ridge, today's recipe) | 9.86 |
| head plus the whole Shoot's average facts (edited or not) | 10.08 |
| gradient-boosted trees (LightGBM, per slider) | 10.23 |
| kernel ridge (RBF) on top of the linear head | 9.95 |

Shoot context without any edits hurts: a day's average facts only add ways to overfit. Trees and kernels fit most sliders a little better in slider space but blow up Saturation, and lose overall. The no-anchor head looks done at this data size; the anchors are where the gain is.

## Caveats

- 67 rendered Photos on about 40 days; the paired SE is about 0.3 to 0.6 dE. The k = 3 gains clear 2 SE; the k = 1 gain of within over the head (-0.45 ± 0.38) does too, narrowly.
- Anchors are Greg's Lightroom edits seen through the best-fit Argus sliders, which are noisier than the exact Recipe a person makes in Argus. In the app the anchors would be cleaner, so this probably understates the gain.
- A capture day is a stand-in for a Shoot; days with two shoots in different light count as one.
- dE is a screen. Greg's taps on a review page are the judge.

## In the engine (2026-10-23)

`engine::shoot_match` holds the shipped shoot model (`engine/src/shoot_match.json`, `bench/shoot_style.py export`: within-sim on all 4,860 fitted pairs, the facts the Edit head reads, no EXIF, which scored the same). "Apply to the rest" on a RAW now leads with it: the first matched Suggestion, titled Shoot match, takes the hero plus up to seven other Photos of the Shoot the person edited by hand as anchors and sets the Photo's own Basic sliders under the hero's Look; the model proposes one matched take after it instead of two, and if the model fails the Shoot match still lands. A variant's `shoot_match: false` turns it off. The engine's formula matches the bench model to 0.001 of a slider unit on 50 random anchor sets; `engine/tests/jobs.rs` covers the job on two sample RAWs.

## The render gap (2026-10-23 evening)

The best Basic sliders still land 5.2 dE from Greg's edit on the cloud bench (4.7 to 5.0 on the pairs), and every model sits on top of that floor. The question was how much of it is Argus rendering differently from Lightroom, and whether one learned transform could close it (`bench/render_gap.py`, fit renders against his JPEG as score.py's 8 x 8 Lab blocks, 100 Photos).

| On top of the best Basic sliders | dE |
|---|---|
| as rendered | 5.23 |
| one shared colour transform (quadratic in Lab), fitted on other days | 5.33 |
| a colour transform fitted per Photo (the ceiling for any global colour change) | 3.63 |
| per Photo, plus radial terms (vignette, lens shading) | 3.15 |
| per Photo, plus radial, plus large-scale local tone (blurred luminance terms) | 3.01 |

Refitting the sliders with Clarity 15, 30 or 50 underneath gains 0.06, or 0.10 picking per Photo. Lens corrections "off" match the default ("camera"); "on" is 1.2 worse. A post-crop Vignette prior hurts the head (−10: +0.15, −20: +0.56, worse on 42 of 67) and the Shoot match, though it helps plain Auto a little (−10 to −20: 11.15 to 10.91).

What it says: there is no shippable "Lightroom look". Once the sliders are fitted, Argus's rendering isn't off from Lightroom's in any shared way. What's left is per Photo: about 1.6 dE of colour and tone shape (larger on Photos where Greg used HSL or colour grading, 2.0, than on Basic-only edits, 1.1), 0.5 of radial shading, a little large-scale local tone, and about 3 dE that no global or radial transform explains (masks and brushes, local tone at scales Clarity doesn't reach, and how a re-rendered JPEG lines up with his). Edits that use only Basic sliders still sit 4.4 dE from their best fit on the pairs, so most of the floor is not his extra tools.

A note from the lab thread agrees from the other side: a model that knows Greg's exact Lightroom sliders (plus the facts) only reaches about 8.5 dE on the pairs, close to the Shoot match with three heroes. The intent is mostly predicted; the per-photo translation from Lightroom sliders to Argus ones is the rest (the same Lightroom Exposure fits to Argus Exposures 0.4 EV apart within a day).

`argus edit-fit` gained what this needed: Recipes take non-number settings (`lensCorrections`), a `base` of settings every render starts from (fit with some Clarity underneath), and scored Recipes get post-crop Vignette and Grain as `render_photo` does.

## Reproduce

```
python3 bench/shoot_style.py screen --k 1,3 [--anchors random|typical|diverse] [--methods ...]
python3 bench/shoot_style.py screen --test --k 1,3      # fold 0, once per decision
python3 bench/shoot_style.py screen --own-sens --siglip siglip.npz --methods head,sync,within-sim,within-sig,within-sigf
python3 bench/shoot_style.py render --k 1,3            # ~10 min, 67 cloud bench RAWs
python3 bench/shoot_style.py export                    # -> engine/src/shoot_match.json
python3 bench/shoot_style.py page                      # Shoot match vs Sync at 2048 px for the review page
python3 bench/render_gap.py out/render-gap/img         # needs edit-fit save_dir renders of each `fit`
```

Inputs: `out/pair-features/fit.jsonl` and `out/whole-edit/bench-features.json` from the Mac checkout (device folder tools). Outputs and logs: `/mnt/project-files/argus-bench/shoot-style/`.

## On the bigger pair set (2026-10-24)

The pairs grew to 6,865 (6,456 fitted under 12 dE, 203 days) with each Photo's own edit-fit `sensitivity`, so the screen now scores every miss with that Photo's slider-to-render matrix (`--own-sens`) rather than the bench average. The story holds. Linearised dE, development folds then the test fold scored once:

| | k = 1 dev | k = 1 test | k = 3 dev | k = 3 test |
|---|---|---|---|---|
| head | 10.31 | 10.08 | 10.32 | 10.10 |
| sync | 11.32 | 11.62 | 9.82 | 10.14 |
| within | 9.41 | 9.66 | 8.93 | 9.33 |
| within-sim | 9.42 | 9.59 | **8.63** | **9.07** |

SigLIP, the best single-Photo input for the Edit head (10.02 against MobileNet's 10.16 on the 99-RAW bench), adds nothing here. Weighting anchors by SigLIP cosine instead of the facts gives 8.72 at k = 3 (8.63 with facts), and its top 16 directions as within-Shoot facts give 9.01. Which anchors look alike matters less than how their scene facts differ.

The shipped `shoot_match.json` is retrained on the full set (same recipe: within-sim, alpha 1000, width 0.5, no EXIF). The per-slider shrink ratios came down a little (Highlights 3.6 to 2.7, Tint 1.0 to 0.8), so one hero's style carries over slightly more than before.

## Greg's taps (review page, 2026-10-23 5:38-5:55 PM ET)

Blind pairs from the held-out run (61 Photos, no hidden-window days), latest answer per Photo and question, Shoot match wins-losses-same: against Sync with 1 hero 16-7-4, against Sync with 3 heroes 4-3-6, against the Edit head 8-5-2. All three together 28-15 on decisive taps (sign test p = 0.07). The one-hero case is where dE said the gain was biggest over Sync, and it is where his eye agrees most (p = 0.09 alone). With three heroes the two look alike to him, as the 0.6 dE gap predicts. Left-side picks 25 of 44, so no strong side bias. Leaning the right way, not yet decisive: about 20 more taps per question would settle it.
