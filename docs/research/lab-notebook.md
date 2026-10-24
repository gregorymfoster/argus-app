# Cull and edit research lab: notebook

A running log of hypotheses, setups, results and verdicts. It started 2026-10-23, when Greg asked for a long research loop on AI culling and editing ("go deep, research online best modern cutting edge techniques... testing hypotheses, invalidating, and trying more, taking notes the whole time"). Newest entries go at the bottom of each day. Scripts: `bench/lab_edit.py` and `bench/lab_cull.py`; one-off analyses live in `out/lab/*.py` and are quoted here with their numbers.

The literature survey behind the hypotheses is `/mnt/project-files/argus-research-lab/reports/Personal cull and edit models.md`, with its notes beside it in `research_notes/`. Its short version: for one photographer with little data, gains come from Shoot context and from choosing what to label, not from bigger backbones or fine-tuning. No published method uses within-Shoot heroes the way the Shoot match does; the closest is Bychkovsky et al. 2011 (the FiveK paper), a Gaussian process whose covariance is learnt from other people's edits, plus active choice of which photos to edit.

## How things are scored

Edits use the Shoot match bench from `docs/research/auto-edit-research.md`: Greg's RAW and edit pairs (a snapshot of the Mac's `out/pair-features/fit.jsonl` on 2026-10-23, 4,860 fitted pairs on 147 days), whole capture days held out, k anchors drawn three times per Photo from the same day. The screen is the linearised render error with the bench's mean slider-to-render matrix ("lin dE", folds 1-4). A lead that survives goes to fold 0 once and to the rendered check on the 67 cloud bench RAWs (`bench/lab_edit.py render`). Paired differences are quoted ± 2 SE. Within-day frames are correlated, so the screen's SEs are optimistic; the rendered SE is the honest one.

Culls use Greg's blind taps from the review page (`cull_blind`, pulled with ArtifactData into `out/lab/votes`) and the clean 2026-10-10 card at 1600 px.

## 2026-10-23

### E1. Full-covariance shrink of the Shoot's style (BLUP)

**Hypothesis.** Sliders move together across Shoots (more Exposure tends to come with less Highlights), so one hero's miss on one slider says something about the others. Replacing the per-slider shrink with the multivariate one, u = Su (Su + Se / k)⁻¹ r, should help most at k = 1.

**Result.** Lin dE k = 1: 9.214 → 9.225. k = 3: 8.619 → 8.636. **Killed.** The cross-slider covariance adds estimation noise without new information.

### E2. Recent style as the prior

**Hypothesis.** Greg's style drifts over time, so the last few edited days should beat the global average as the starting point, with no heroes at all (k = 0) and as the prior for the hero update.

**Result.** k = 0, best window (last 20 days, shrunk 20 / 22): 9.813 → 9.772. With heroes: no change (8.619 → 8.613 at k = 3). The previous day alone, unshrunk, is far worse (11.13). **Killed as a lever.** His style doesn't carry from one shoot to the next; what carries is the long-run average, which the model already has.

### E3. Trust heroes whose fit is good

**Hypothesis.** A hero whose best-fit sliders still miss Greg's JPEG by a lot is a noisy anchor; down-weight it.

**Result.** 8.619 → 8.616 at k = 3. **Killed.**

### E4. A Gaussian-process Shoot model (literature hypothesis 4)

**Hypothesis.** The shipped model keeps one intercept per Shoot and weights heroes by a fixed similarity kernel. A GP that splits the within-Shoot residual into a part that varies smoothly with the scene facts (share rho, RBF length ell on the z-scored facts) and pure noise, with the Shoot intercept as a shared random effect, should use several heroes better, and more so as k grows.

**Setup.** Per slider, posterior mean of intercept plus smooth part at the target given the heroes' residuals, variances from the training days (`gp_update` in `bench/lab_edit.py`). Swept rho in {0, 0.3, 0.5, 0.7, 0.8, 0.9} and ell in {0.3, 0.4, 0.5, 0.7, 1.0}; chose rho 0.8, ell 0.45.

**Result.**

| lin dE, folds 1-4 | k = 1 | k = 3 | k = 5 | k = 8 |
|---|---|---|---|---|
| within-sim (shipped) | 9.214 | 8.619 | 8.294 | 8.060 |
| GP | 9.162 | 8.547 | 8.189 | 7.907 |

Test fold, scored once: k = 1 -0.03 ± 0.03, k = 3 -0.06 ± 0.05, k = 5 -0.09 ± 0.06. Rendered on 67 bench RAWs: k = 1 8.71 → 8.65 (-0.06 ± 0.12), k = 3 7.90 → **7.77** (-0.13 ± 0.11, better on 43 of 67).

**Verdict. Small real win**, the same sign on every check and growing with the number of heroes. Smaller than the survey's guess (7.4 to 7.7).

### E5. Where the remaining error lives

At k = 3 with the GP, the miss above the fit floor splits by slider (share of dᵀMd): Exposure 55%, Temp 19%, Tint 6%, Whites 6%, Vibrance 5%, the rest under 4% each. So it is frame-to-frame exposure and white balance.

But how much of that is Greg's intent? When two frames of one day carry identical Lightroom Exposure, their fitted Argus Exposure still differs with a 0.40 EV spread. And an oracle that is told Greg's actual Lightroom settings for the Photo (13 crs values: Basic, Texture, Clarity, Dehaze, Vignette, Noise) plus the facts reaches only 8.48 lin dE with LightGBM (ridge 8.82; his sliders alone, 10.0), against a fit floor of 4.75. Given the GP's change against his settings relative to the heroes, a linear correction gets 8.55 → 8.05.

**What it says.** On this bench, Shoot match at k = 3 (8.55) is already about as good as knowing his settings outright. Most of what's left is per-photo translation between Lightroom's render and Argus's, which no model of Greg's intent can recover. Two consequences: the Lightroom-vs-Argus render gap is now the biggest edit lever (the "Auto-edit model research" thread owns it, told 2026-10-23), and in the app, where heroes are Argus Recipes and there's no translation, Shoot match should do better than this bench says.

### E6. Which photo the person edits first

**Hypothesis.** Heroes chosen on purpose beat random ones. Literature: Bychkovsky's mutual-information choice saves about one edited photo.

**Result** (lin dE, folds 1-4):

| heroes | k = 1 within-sim | k = 1 GP | k = 3 within-sim | k = 3 GP |
|---|---|---|---|---|
| random | 9.214 | 9.162 | 8.619 | 8.547 |
| most typical frame of the day | 8.862 | **8.773** | 8.657 | **8.485** |
| spread out (farthest point) | 8.862 | 8.773 | 8.575 | 8.574 |
| greedy information (GP posterior variance) | 9.057 | 8.974 | 8.387 | **8.323** |

**Verdict. Confirmed, and the biggest cheap lever found today.** With three heroes, choosing them greedily to leave the least GP variance over the rest of the day beats random by 0.22 dE (8.55 → 8.32), three times the GP upgrade itself. With one hero, editing the Shoot's most typical frame is worth 0.4 dE over a random one: as much as the whole GP upgrade at k = 3. Product idea: Argus suggests "start with this one" when a Shoot opens in Edit.

### E7. Flag the photos the heroes don't cover

**Hypothesis.** The model's own uncertainty says which Photos it will get wrong, so Argus could ask for one more hero there instead of guessing.

**Result** (k = 3, GP). Spearman of error with the GP's Exposure variance 0.24, with distance to the nearest hero 0.33. Sorting by that distance, the closest 80% score 8.15 and the farthest 20% score **10.15**, worse than the no-hero head (9.85). **Confirmed** as a signal, and it led to E8.

### E8. Lean on the head when no hero looks alike

**Hypothesis.** Far from every hero, blend toward the no-anchor head: P = w GP + (1 - w) head, w = exp(-nearest distance / tau).

**Result.** Lin dE, folds 1-4: k = 1 9.162 → 9.053 (tau 4), k = 3 8.547 → 8.494. A constant 0.85 blend gets most of it, so it is partly plain ensembling. Test fold once: k = 1 9.057 → **8.890**, k = 3 8.377 → **8.303**. Rendered: k = 1 8.71 → **8.58** (-0.13 ± 0.15 against the shipped model, better on 42 of 67), k = 3 7.90 → 7.79.

**Verdict. Small win**, mostly at k = 1. Together with E4, the Shoot match would move from 8.71 to 8.58 with one hero and from 7.90 to about 7.78 with three. Each step is inside 2 SE on its own rendered check, but every screen, the test fold and the renders agree on the sign.

### E9. Camera white balance as a light-source fact

**Hypothesis.** Temp is the second-largest miss; reviewers say white balance is what they fix most in AI edits. The camera's as-shot white balance is a cheap light-source proxy the Shoot model doesn't read.

**Result.** Adding log(R/B) and log(G/√RB) of the as-shot multipliers: 8.61 → 8.59 at k = 3, Temp's share 11.8 → 11.7. **Killed.** A real light-source classifier is still open but this cheap version carries nothing.

### E11. The whole guided protocol, as the engine would run it

**Question.** E4, E6 and E8 were each scored on their own and with EXIF facts. Stacked together, on the 32 facts the engine has live, what does the full protocol (Argus suggests which photos to edit first, GP Shoot model, lean on the engine's Edit head when no hero looks alike) buy over today's Shoot match?

**Setup.** `out/lab/guided.py` (not in git), folds 1-4, linearised dE, no EXIF. "Random" heroes stand for whatever Greg happens to edit first; "MI" is E6's information-based choice.

**Result.**

| heroes | k | shipped Shoot match | GP + fallback |
|---|---|---|---|
| random | 1 | 9.235 | 9.044 |
| random | 2 | 8.852 | 8.721 |
| random | 3 | 8.629 | 8.495 |
| random | 5 | 8.302 | 8.170 |
| MI | 1 | 9.233 | 9.080 |
| MI | 2 | 8.760 | 8.622 |
| MI | 3 | 8.435 | 8.319 |
| MI | 5 | 8.133 | 8.008 |

**Verdict: holds.** The model upgrade is worth 0.13 to 0.19 at every k, whichever heroes are edited. Suggesting heroes adds another 0.09 to 0.18 from k=2 up, so at three heroes the stack goes from 8.63 to 8.32 (-0.31). At one hero the information pick no longer helps without EXIF (9.08 vs 9.04); E6's k=1 win leaned on camera facts, so for a single hero "most typical" (E6) is the better rule. Still parked with the E10 port, per Greg's hold.

### C1. Greg's blind taps against his own Lightroom picks

The 159 decisive taps on the 2026-10-10 card all pair a frame he picked in Lightroom with one he didn't. His blind tap agrees with his own earlier pick **66%** of the time (95% interval 59-73%), and only **57%** on the 82 pairs inside one burst.

**What it says.** Within a burst, Greg today agrees with Greg at capture time little better than a coin. The survey arrives at a similar ceiling from his 66% repeat agreement on edit pairs (if each tap matches his real preference with probability q, then q² + (1 - q)² = 0.66 gives q ≈ 0.78). So any within-burst score above about 0.6 against one set of his labels is near what can be measured. Lightroom picks mined from bursts (survey hypothesis 3) would teach a label that his taps only half agree with. The cull's lever is whole-Shoot ranking (AUC 0.66 to 0.70 today) and showing near-duplicates as a Stack the person picks from, not a sharper burst ranker.

### C2. Signals on the taps, with intervals and by margin

| signal | all taps | big margin | small margin | card bursts (pick vs not) |
|---|---|---|---|---|
| eyes open | **0.64** [0.57-0.71] | **0.70** [0.60-0.81] | 0.59 | 0.42 |
| faces score | 0.59 [0.51-0.66] | 0.60 | 0.58 | 0.60 |
| smile | 0.58 [0.51-0.66] | 0.63 | 0.54 | 0.57 |
| sharpness | 0.57 [0.49-0.64] | 0.53 | 0.60 | 0.41 |
| heads upright | 0.50 [0.42-0.58] | 0.51 | 0.49 | 0.68 |

Across signals, agreement on the taps and on the card's burst picks correlates at 0.04. Eyes open is the one signal that gets stronger when the frames differ more on it. The later frame wins 0.49 [0.41-0.57] (survey hypothesis 16: **killed**).

### C3. "Both" answers

On 2023-05-20, 23 of 45 answers were "both" and one "neither". Half the near-duplicate pairs on that Shoot have no winner for Greg. On 2026-10-10 he never used it. This is the strongest argument yet for Stacks over forced burst picks.

### C4. Choosing scenes versus choosing frames

**Question.** How much of the cull is picking the right scenes (bursts) and how much is picking the frame inside one?

**Setup.** On each of the five cull Shoots, the AUC Greg's picks would get from a "scene oracle" that knows which bursts (`engine::bursts` rule) hold a pick and ranks every frame of those bursts equally. Plus burst size (how many frames he shot of the moment) as a free signal.

**Result.**

| Shoot | frames | picks | scene oracle | burst size | faces |
|---|---|---|---|---|---|
| 2023-05-20 | 100 | 46 | 0.57 | 0.57 | 0.52 |
| 2024-08-10 | 100 | 87 | 1.00 | 0.72 | 0.42 |
| 2024-10-19 | 100 | 35 | 0.91 | 0.51 | 0.57 |
| 2025-03-22 | 100 | 49 | 0.78 | 0.35 | 0.62 |
| 2026-10-10 | 261 | 124 | 0.85 | 0.61 | 0.62 |

On 2026-10-10, 39 of the 49 multi-frame bursts hold a pick, and in 14 of them Greg picked every frame.

**What it says.** Knowing only which scenes Greg keeps already scores 0.78 to 1.0 on four of five Shoots, well above today's whole-card 0.66 to 0.70. The open problem is ranking scenes, and the frame inside a scene matters much less (and, per C1, Greg himself barely agrees with himself there). Burst size flips sign between Shoots: **killed**. Next: a scene-level taste score (the Taste head's direction averaged over a burst, against the Shoot's other bursts) and a scene-level benchmark (keep any frame of a kept scene = right).

### C5. A scene-level taste score

**Question.** C4 says the cull is mostly about which scenes Greg keeps. Does scoring whole bursts (instead of single frames) rank his kept scenes better?

**Setup.** Leave one Shoot out over the five cull Shoots (`out/lab/scene/c5.py`, not in git). Bursts from `taste.stacks`. A scene counts as kept when any of its frames is a pick. Scene scores: the shipped frame score (Taste head from the other four Shoots plus faces) averaged or maxed over the burst; a separate "scene direction" trained on kept-scene minus dropped-scene burst means; that direction plus the best faces score. Frame AUC then gives every frame its scene's score, with the frame score breaking ties.

**Result.** Mean over the five Shoots:

| score | scene AUC | frame AUC |
|---|---|---|
| shipped frame score | | 0.587 |
| frame score, mean over burst | 0.626 | |
| frame score, max over burst | 0.638 | 0.590 |
| scene direction | 0.582 | 0.567 |
| scene direction + faces | 0.633 | 0.585 |

**Verdict: killed.** The best frame of a burst already ranks scenes as well as anything trained on scenes, and spreading it to the whole burst moves frame AUC by 0.003. A direction trained on scene means is worse (0.58), likely because averaging a burst smears out the very frame that made Greg keep it. The bottleneck is the taste signal itself, not the unit it is applied to. Burst size scores 0.67 on scenes, but that is mostly arithmetic (more frames, more chances to hold a pick), so it is no lead.

### N0. Neural nets: what Greg asked for, and the plan

Greg, 2026-10-23: "have we thought about creating a moderately significant neural network on this computer, even if it takes hours to train, to see if it can cull/edit better than the other regression things we've made?" Then, from the coordinator's list, "do all of them": this thread owns a pixel net that turns Argus's render into Lightroom's look, and learned local tone maps. Torch runs in the cloud on CPU (no pretrained weights reach it); the real runs go to the Mac on MPS, overnight at full power per Greg's rule.

- **Net A** (`bench/lab_net.py`): a CNN on the unedited render adds g(image) to the Shoot match's within-Shoot part. It trains only on differences inside a day (day means removed from targets and outputs) with the linearised render loss, so it can't learn a day's look, only frame-to-frame moves. Backbones: a small CNN from scratch, or torchvision MobileNetV3 / ConvNeXt-Tiny with ImageNet weights on the Mac.
- **Nets B and C** (`bench/lab_look.py`): from Argus's render at Greg's best sliders to his JPEG, on score.py's 8 x 8 block grid with a differentiable Lab dE76 loss. Shared curves (an image-independent "Lightroom look" that could ship as a Camera Profile), a shared 3D LUT, CNN-predicted per-photo curves (global, so expressible as Basic and Curve settings), a CNN-predicted low-res bilateral grid (HDRNet-lite, the local upper bound), and 18 soft regions (3 brightness bands x 3 columns x 2 rows, standing in for luminance-range and gradient Masks, so the result stays editable).

### N1. How much of the ~5 dE floor is curves, regions and local tone

**Setup.** Per-photo oracle on the 100 cloud bench RAWs: each model's parameters fitted to that photo's own JPEG (`out/lab/look_oracle.py`). Not a prediction, a ceiling.

**Result.** Block dE: Argus at best Basic sliders 5.18; plus per-photo RGB curves 3.90; plus 18 soft regions 3.29; plus a free 4 x 6 x 8 bilateral grid 2.37.

**What it says.** About a quarter of the floor is global tone and colour beyond Basic (curves), and most of the rest is regional. A Mask-shaped local layer could reach about two thirds of what an unconstrained local grid reaches, while staying editable. This answers the survey's hypothesis 6 roughly as it guessed (floor 2 to 3 with a richer basis).

### N2. First CPU runs (prototype, too little data)

Held out by day on the 100 bench Photos, the only learnt model that helped is the shared curves (5.18 → 5.12, better on 67 of 100). Every image-adaptive one overfits with 80 training photos (per-photo curves 5.60, local grid 5.51, regions 5.36) and the shared LUT more so (6.15). Net A from scratch on the 591 pair renders that existed so far (31 days) hurts too: k = 3 8.56 → 8.85. Both need the full ~6k pairs and, for net A, a pretrained backbone. **Inconclusive by design**: these runs check the code, the Mac runs answer the question.

### N3. The heavy runs: plan (Greg, 2026-10-23 7:41 PM ET: "start the heavy ai work now")

Greg asked for these to start now, planned out, checkpointed and inspected along the way. The CPU prototypes (N2) only showed that 100 to 600 photos are too few. The Mac has every fitted pair (several thousand, across ~290 capture days), which is the real test.

**Protocol (the same for every run).** Held out by capture day. Folds 1-4 are for development, and every number below is "trained on three folds, scored on the fourth". Fold 0 is the test fold, scored once, only for a run that already won on 1-4. Inside each training part, 15% of days are validation: the kept weights are the best validation epoch, a run stops early after `--patience` epochs with no gain, and every (model, fold) is checkpointed with its learning curve (`out/lab/{look,net}/ckpt/`, `curves.jsonl`), so a stopped job resumes. Each net runs at least once at 25% and 50% of the training days, to show whether more data would still help.

**Hypotheses and kill rules.**

| run | hypothesis | baseline | a win is | killed if |
|---|---|---|---|---|
| A, `lab_net` small CNN | the unedited image says how Greg's edit moves from frame to frame inside a Shoot, beyond the cheap facts | GP Shoot model (E4) at k = 1 and 3 | at least 0.1 lin dE better on folds 1-4, and the shuffle control shows no gain | under 0.05, or the shuffle control gains as much (then it's leakage, not the image) |
| A, `--control shuffle` | images swapped among frames of each day carry no signal | same | n/a: this is the leak check | |
| A, mobilenet / convnext | pretrained features beat a small net trained from scratch | run A small | better than small by 0.05 | not better, or no weights on the Mac |
| B, curves / LUT | one shared transform closes part of the render gap | identity (Argus at the best sliders) | 0.2 dE better | as `render_gap.py` found, under 0.1 (expected: kill) |
| C, adapt-global | an image-aware global curve per Photo | identity, curves | 0.3 better than curves | under 0.1 better |
| C, adapt-masks | 18 soft regions the engine can express as Masks close a real share of the gap (the oracle reached 3.29 from 5.18) | adapt-global | 0.2 better than adapt-global | not better: local tone isn't learnable from 64 px |
| C, adapt-local | the bilateral-grid ceiling for learnt local tone | adapt-masks | shows how much a richer Mask set would add | |

**What we inspect at each checkpoint.** Train against validation curves (a gap that widens means overfitting, so lower capacity or more decay). The best epoch (if it's always the last, train longer). Held-out against identity per fold (one bad fold means a day-type problem, so look at which days). For C, the rendered thumbnails of the best and worst held-out cases.

**Queue on the Mac** (background priority, `scripts/progress`, resumable):

1. `lab_net --features out/pair-features/fit.jsonl --plain out/pair-features/plain --device mps` (small), then `--control shuffle`, then `--frac 0.25` and `0.5`.
2. `lab_look mac-job` (fit renders of every pair, chunked), then `prep`.
3. `lab_look eval --device mps` (all five models), then `--frac 0.25,0.5` for the best two.
4. mobilenet and convnext for run A if their weights are on the Mac.
5. `--test` once for any run that won on folds 1-4.

### N4. Heavy runs, first checkpoint (2026-10-23, 7:55 PM ET)

**Net A on the renders that existed at start** (about 1,000 renders on 57 training days in folds 1-4, small CNN, MPS):

| run | k = 1 GP | k = 1 GP + net | k = 3 GP | k = 3 GP + net |
|---|---|---|---|---|
| all renders | 9.300 | **9.153** | 8.580 | **8.492** |
| shuffle control | 9.289 | 9.272 | 8.606 | 8.598 |
| 25% of days | 9.304 | 9.280 | 8.606 | 8.587 |
| 50% of days | 9.308 | 9.407 | 8.623 | 8.589 |

**What the curves say.** The net helps by -0.15 at one hero and -0.09 at three, and the shuffle control keeps only -0.02 and -0.01 of that. So the gain comes from reading the image, not from leakage. That clears the first kill rule. But the run is fragile. The best validation epoch is between 1 and 11 in every fold. Training loss keeps falling after that while validation loss swings (one fold goes 0.87 → 4.0 between epochs). The 50% run is worse than the 25% run, which with this few days is seed noise, not a trend. This is a data-starved net: about 8 validation days per fold, and one odd day moves the curve.

**Changes made from it.** `lab_net --seeds 0,1,2` trains one net per seed and also scores their average, which shows the seed spread and is usually the better model. The rerun on every pair (after the feature job writes all the plain renders) uses three seeds. Not yet a win: it has to hold on the full data with three seeds before the test fold.

**Other notes.** `lab_look` fit renders: 506 cases so far. Its LUT and bilateral-grid models need `grid_sampler_3d`, which MPS lacks, so that eval runs with `PYTORCH_ENABLE_MPS_FALLBACK=1`. The mobilenet backbone downloaded its ImageNet weights on the Mac, so the pretrained runs are on.

**Update, 8:10 PM ET.** The pretrained backbones lost to the small net, each against its own GP baseline (more renders had landed between runs). Mobilenet was +0.15 worse at one hero and +0.16 at three; convnext was +0.03 and +0.06. ImageNet features at this data size add capacity the net can't use yet. The look nets on 385 held-out cases: one shared tone curve per channel -0.10 ± 0.01 (better on 320 of 385), one shared 3D LUT **-0.22 ± 0.03 (better on 337 of 385)**. That's a surprise next to `render_gap.py`, whose shared Lightroom transform gained nothing. The difference is that this LUT is learnt end to end on the bench's block dE, with colour crosstalk. A shared LUT is image-independent, so if it holds on the test fold it could ship as a Camera Profile style "Lightroom look". The adaptive models crashed in MPS backward (a `.view` on a non-contiguous tensor), fixed in the next commit.

**Update, 8:40 PM ET: the look nets on folds 1-4** (385 held-out cases; each trains on only 180 to 340 cases):

| model | held-out block dE | vs identity | better on |
|---|---|---|---|
| identity (Argus at the best sliders) | 5.467 | | |
| shared curves | 5.370 | -0.10 ± 0.01 | 320/385 |
| shared 3D LUT | 5.242 | -0.22 ± 0.03 | 337/385 |
| per-photo curves (adapt-global) | 5.399 | -0.07 ± 0.02 | 279/385 |
| 18 soft regions (adapt-masks) | 5.232 | -0.24 ± 0.04 | 310/385 |
| bilateral grid (adapt-local) | **5.168** | **-0.30 ± 0.04** | 333/385 |

Test fold, once, for the two shared looks: LUT 4.838 → 4.704 (**-0.13 ± 0.04**, better on 94 of 121), curves -0.07 ± 0.01 (111 of 121). The shared LUT holds up out of sample, at about 60% of its development gain.

**What the curves say.** The local models train well (train dE 5.4 → 4.5) but validation barely moves (6.37 → 6.37 on fold 1). So they are capacity with no data behind it. The shared LUT's best epoch is 0 in three of five runs, which means it learns everything it can in one pass over 300 photos. Per-photo curves do worse than shared ones: at this size, image-adaptive is pure overfitting.

**The finding that matters most: these nets saw 506 of the ~3,700 fitted pairs.** `mac-job` read the old `out/pairs` set (520 RAWs) instead of the published `out/pairs-raw`. Fixed, so the rerun renders every fitted pair. Every look-net number above is a 25-day result and should get better with seven times the data. The ordering already matches the per-photo oracle (N1): local tone beats regions, which beat global curves.

**Update, 9:20 PM ET: Net A with three seeds** (1,736 renders):

| run | k = 1 GP | three seeds | ensemble | k = 3 GP | ensemble |
|---|---|---|---|---|---|
| small | 9.630 | 9.809 / 9.538 / 9.609 | **9.571** (-0.06) | 8.827 | **8.794** (-0.03) |
| shuffle control | 9.630 | 9.800 / 9.627 / 9.760 | 9.668 (+0.04) | 8.827 | 8.866 (+0.04) |

The seeds alone span 0.27 dE, about twice the gain the first run reported. So the first run's -0.15 was mostly a lucky seed. The image does carry something: the real net beats its shuffled twin by about 0.10 at one hero, seed for seed. But the gain over the GP is now under the 0.1 bar and near the 0.05 kill line. The all-pairs run decides it. Lesson for the rest of the night: never read a single-seed net result on this data.

The look nets now have **4,939 cases** (from 506), and eval3 is running.

### N5. A per-Shoot colour look from the heroes

**Question.** Eval3 (every pair, 143 days) shrank the shared looks to LUT -0.13 and curves -0.07, better on only 55-58% of photos. So there's no single "Lightroom look" to learn. Is the part of Greg's edit that his Basic sliders can't explain stable inside a Shoot instead? If so, the heroes of a Shoot should carry it to the rest.

**Setup.** `bench/lab_shoot_look.py` runs on the Mac's `data.npz` (4,939 cases, 8x8 blocks of Argus's render at each photo's best sliders against Greg's JPEG). It's plain numpy, no net. For each held-out day, it fits a quadratic colour transform (10 terms per channel, ridge toward identity) on k random hero photos and applies it to the day's other photos, three draws per day. It compares against identity, one global transform, the Shoot transform shrunk toward the global one, and a per-photo oracle. The shrink strength was chosen on folds 1-4: 10 at one hero, 0.1 at three and five. After that, the test fold was scored once.

**Result.** Block dE on the day's other photos:

| | folds 1-4 | test fold |
|---|---|---|
| identity | 6.06 | 5.33 to 5.38 |
| one global transform | +0.08 (worse) | |
| Shoot look, 1 hero | -0.13 ± 0.04 | **+0.23 (worse)** |
| Shoot look, 3 heroes | -0.55 ± 0.07 (better on 59%) | **-0.34 ± 0.04** (61%) |
| Shoot look, 5 heroes | -0.84 ± 0.07 (68%) | **-0.45 ± 0.05** (65%) |
| per-photo oracle | 3.0 | 2.9 |

**Verdict: the best lead on the render gap so far.** From three heroes up, the Shoot look holds on the test fold at about 60% of its development gain. That's a bigger effect than any net tonight, and it comes from a 30-number transform per Shoot. One hero isn't enough, because a single photo's colour quirks get copied to the whole Shoot. It also explains N1 and `render_gap.py`: Greg's look isn't global, it's per Shoot (colour grading, HSL, calibration, which our Basic sliders don't carry). The per-photo oracle at 2.9 shows how much is still left.

**Caveats and next steps.** This runs on best-fit renders, so in the app the other photos would carry Shoot match's predicted sliders, not their best ones. The look should mostly survive that, but the rendered check has to show it. To ship, it needs a home in the Recipe that a person can see and edit: either a Shoot-level Look stage (a colour matrix), or a fit into HSL and Color Grading sliders, which is better because those are editable. Greg's hold on shipping stands. Next: fit the same transform as HSL and Color Grading sliders and see how much of the 0.34 to 0.45 survives, and weight heroes by similarity as Shoot match does.

**N5b. Weighting heroes by likeness.** For each target photo, the transform is refitted with heroes weighted by how close their renders' mean Lab and spread are. Folds 1-4: at 3 heroes, -0.61 → -0.69, and at 5 heroes, -0.84 → -0.93 (width 0.5). It's a small, consistent gain, the same shape as Shoot match's within-sim weighting.

**N5c. The same look as Argus's own sliders.** `bench/lab_shoot_hsl.py` reimplements the engine's HSL (eight bands, hue/sat/lum, `pipeline::HslBands`) and Color Grading (shadows, midtones, highlights and global wheels with lum, `color::GradingOp`) in torch on the block grid. It fits those 36 slider values per Shoot on the heroes, with a ridge penalty, by gradient descent on block dE. Both operators work on encoded RGB after the tone curve, so applying them to the fitted render is close to what the engine does. The caveats are block means instead of pixels, and HSL landing after vibrance instead of before. The ridge was chosen on fold 1 (0.1), then the test fold was scored once:

| heroes | quadratic transform (test) | HSL + Color Grading sliders (test) |
|---|---|---|
| 3 | -0.34 ± 0.04 | **-0.36 ± 0.11** (better on 66%) |
| 5 | -0.45 ± 0.05 | **-0.33 ± 0.11** (66%) |

The editable sliders keep all of the gain at three heroes and about three quarters of it at five, with a wider spread, because a few days fit badly. Fold 4 is the weak fold (k = 3 -0.03, k = 5 -0.28). So "Shoot match also fills in HSL and Color Grading from the heroes" is a shippable form of N5: the person sees and can undo every value. Before shipping, it needs a real rendered check through `argus edit-fit` with these slider values on the Mac, and the switch-off at one hero. Greg's hold applies.

**N5c rendered (10:40 PM ET).** Real `argus edit-fit` renders of the 1,045 test-fold photos, at their best sliders without and then with the Shoot's fitted HSL + Color Grading: **5.012 → 4.813, -0.20 ± 0.05, better on 593 of 1,045, and on 18 of 30 days.** So about 55% of the block-space gain survives in real renders. The miss has a clear cause. The look lowered mean L by 0.9 in the renders, because the block proxy sees lightness moves through block means, while the engine applies HSL lum per pixel, scaled by that pixel's saturation. Meanwhile the yellow-blue offset dropped from -0.97 to -0.05, which is exactly the colour cast the look is meant to fix. A refit with lightness locked to the Basic sliders (`ARGUS_NO_LUM=1`) is being rendered. Its block-space test number (-0.34 ± 0.06) is a second look at the test fold, so treat it as development, not as a fresh test.

**Eval3, the look nets on every pair** (3,797 held-out cases): curves -0.07, LUT -0.13, per-photo curves +0.02, bilateral grid -0.13. Every gain halved from the 506-case run, and image-adaptive models no longer beat a shared LUT. Seven times the data made the nets *worse* relative to identity. The likely reason is that the bigger set has many more Shoots, each with its own look, so any one shared mapping fits fewer of them. That points the same way as N5: condition on the Shoot, not on the single image.

**N5d. Lightness locked to the Basic sliders (11:30 PM ET).** Refit with the HSL and grading lum values held at zero, so the look only moves hue and saturation. Real renders on the 1,045 test-fold photos: **5.012 → 4.693, -0.32 ± 0.04, better on 720 of 1,045 (69%).** That's all of the block-space gain, now in real Argus renders, as sliders a person can read and change. Because this variant was chosen after the first test render, it's being rendered again on the development folds (3,480 photos; block space there is -0.24, -0.23, -0.43, -0.20 by fold) for an honest confirmation.

**Net A on more pairs: killed.** With about 2,600 renders and three seeds, the net hurts. At one hero: GP 8.974, ensemble 9.165. At three heroes: 8.263 against 8.368. The shuffle control equals GP. The earlier gains were small-data noise. A from-scratch CNN on unedited renders learns nothing about the within-Shoot edit that the cheap facts don't already carry. The look nets tell the same story: on every pair the soft-region Masks model is best of five, at only -0.14 ± 0.03, so image-conditioned learning loses to Shoot-conditioned fitting.

**N5e. The honest check: development folds, real renders (12:30 AM ET). Weaker than it looked.** The lightness-locked Shoot look on the 3,480 development-fold photos: **5.654 → 5.600, -0.05 ± 0.03**, better on 57% of photos but on only 41 of 87 days. By fold: +0.03, -0.11, -0.24, +0.06. Some days get much worse (+1.5 to +3.3 dE on six days, mostly mid-2024) and others much better (-1.5 to -2.2). The test fold's -0.32 was the lucky end of a wide spread. Pooled over all 4,525 rendered photos it's about -0.12.

A leave-one-hero-out gate (fit on two heroes, check the third, apply the look only when that helps) doesn't separate the good days from the bad on the development folds. The correlation between the gate and the rendered gain is 0.03, so no gate.

**What it says.** The block-space proxy (the engine's HSL and grading applied to 8x8 block means) promised -0.2 to -0.4 on every fold, but real renders deliver that only on some days. On others the same sliders, applied per pixel, go the wrong way. Fitting on the proxy is the weak link, not the idea of a Shoot look: N5's quadratic transform was never rendered, and the oracle gap is still ~2.5 dE. **Verdict: not shippable as is.** The next step is to fit the Shoot's HSL and grading through real renders, by adding those sliders to `argus edit-fit`'s fit across a Shoot's heroes, then rerun this same dev and test check. That's an engine change for a later session.

### Net D (cull) is blocked on clean data

A cull fine-tune needs keeps and rejects rendered the same way. Favorites are Greg's edits (the edited-look leak), and the drive's rejects exist only as 768 px features, not images. The pairs' unedited renders give keepers as shot; rejects need `drive_bench.py` to save its renders. And per C1, the within-burst label ceiling is about 0.6, so a cull net can only help whole-Shoot ranking. Left for after the edit nets.

## Open leads

- Render-gap work (other thread): per-photo Lightroom-vs-Argus translation is most of the remaining edit error on this bench.
- Ship E4 + E8 into `engine::shoot_match` if Greg wants the small gain.
- "Start with this one": suggest the most typical frame as the first hero.
- Cull: whole-Shoot ranking with a stronger embedding on the Mac (SigLIP 2, licence check), not burst ranking.
- An Argus-native edit bench: Greg's Recipes made in Argus, so heroes and targets share one renderer.
