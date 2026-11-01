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

### E10. E4 + E8 in the engine

`engine::shoot_match` reads optional `gp` (rho, length) and `fallback` (tau) from `shoot_match.json`; without them it is the shipped weighted mean. The fallback is the engine's own Edit head, and with it and no EXIF (as the engine runs) the screen gives k = 1 9.235 → 9.044 and k = 3 8.629 → 8.495 against the shipped rule; test fold once: k = 1 -0.16 ± 0.06, k = 3 -0.15 ± 0.06 (`out/lab/fallback_enginehead.py`). Length and tau are rescaled for the 32 live facts the engine averages over (the bench averages over all 35): length 0.45 x √(35/32), tau 4 x 35/32. A Python port of the engine formula matches lab_edit's GP plus fallback to 0.0008 of a slider unit on 40 random anchor sets, and the Rust code matches the port to 1e-13 (`out/lab/parity.py` plus a throwaway test). Tests: `engine/src/shoot_match.rs` (closed form for look-alike anchors, far anchors lean on the head), `engine/tests/jobs.rs` apply_to_the_rest.

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

**Verdict: holds.** The model upgrade is worth 0.13 to 0.19 at every k, whichever heroes are edited. Suggesting heroes adds another 0.09 to 0.18 from k=2 up, so at three heroes the stack goes from 8.63 to 8.32 (-0.31). At one hero the information pick no longer helps without EXIF (9.08 vs 9.04); E6's k=1 win leaned on camera facts, so for a single hero "most typical" (E6) is the better rule. The GP and fallback went on by default on 2026-10-24 (Greg's check-in, "turn it on, with an off switch", 17595cf, switch in Settings › AI and Compute); suggesting heroes still needs a screen.

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

### N6. The Shoot look fitted on real pixels (2026-10-24, Mac; Greg: "do it")

**Why.** N5e showed the block-space proxy doesn't carry over to real renders on the development folds (-0.05). So the look is fitted on what the engine actually draws: each hero rendered at its best sliders (vibrance and saturation at 0), then `bench/lab_shoot_fit.py fit` runs a torch copy of the engine's HSL, vibrance, saturation and Color Grading on those pixels (16 of 64 per block, Adam, about 30 s per Shoot) toward Greg's JPEGs. The copy sits 0.49 dE from the engine on a cloud RAW against a 4.4 dE effect, and it recovers a known edit from 4.45 to 0.61. The plan is 117 days with three heroes each; the 4,525 other photos are scored by real `argus edit-fit` renders at their best sliders, without and with their Shoot's look.

**Result** (rendered dE against the best sliders alone, 4.860; test fold 1,045 photos, development folds 3,480):

| look | all | development | test |
|---|---|---|---|
| hue and sat only | -0.273 (62%) | -0.322 (63%) | -0.113 (58%) |
| hue and sat, half strength | -0.270 (77%) | -0.301 (77%) | -0.167 (77%) |
| with lum | -0.322 (67%) | -0.363 (68%) | -0.185 (62%) |
| with lum x 0.3 | -0.245 (90%) | -0.261 (89%) | -0.193 (90%) |
| with lum x 0.4 | -0.299 (87%) | -0.320 (86%) | -0.231 (89%) |
| with lum x 0.5 | -0.339 (84%) | -0.364 (83%) | -0.257 (86%) |
| with lum x 0.6 | -0.365 (80%) | -0.394 (79%) | -0.270 (82%) |
| **with lum x 0.75** | **-0.377 (75%)** | **-0.411 (75%)** | **-0.263 (76%)** |

Strength was chosen on the development folds only (peak at 0.75). Two renders shared an edit-fit job file and ran side by side; both were rerun alone on 592e13b and matched to the last digit.

**Verdict: holds, the first look that survives real renders on every fold.** Fitting through the engine's own pixels turned the development gain from -0.05 (N5e) to -0.41, and the test fold keeps about two thirds of it, better on three photos in four. Shrinking toward no look trades a little mean for a lot of consistency: at 0.4 it helps nine photos in ten. It runs on best-fit sliders, so in the app it would sit on top of the Shoot match's predicted sliders; the gain should mostly carry since the look lives in HSL and grading, which the Shoot match doesn't touch, but that's untested. **Too subtle to judge by eye.** On 40 test-fold photos (`lab_shoot_hsl.py page`), the look moves the render by only dE 1.4 to 4.2 at 192 px (median 2.1), all under the review page's floor of 5, so the pairs weren't shown to Greg (he asked not to see edits he can't tell apart). It's a real but quiet correction. A Rust port into "apply to the rest" is worth it only bundled with a bigger Shoot match change, and should be checked on predicted sliders first.

### N7. Per-Shoot AI-Mask edits from the heroes

**Hypothesis.** The rest of the render gap is local: Greg lifts faces, darkens skies, separates subject from background. `lab_shoot_fit.py local` fits each hero's local exposure (subject, background, faces, sky) and sky saturation with `edit-fit fit_local`, takes the Shoot's mean shrunk by k/(k+1), and renders it on the other photos.

**Result.** +0.012 ± 0.005 overall (worse), development +0.017, test -0.007, better on 45-48%. **Killed.** A Shoot-wide mask move doesn't carry from heroes to the rest, likely because the right local lift depends on each frame's light and framing. Local edits would need a per-photo model, which the pairs can't train well (fit_local barely beats the global fit even on the heroes).

### N8. A light-source read for white balance (2026-10-24, cloud)

**Hypothesis.** Temp and Tint are about a quarter of the squared edit error with no heroes and still a fifth at k = 3. E9 showed the camera's as-shot multipliers alone carry nothing, but the picture itself should say what the light is (shade, tungsten, golden hour). So the Shoot model (GP, folds 1-4, lin dE) gets the as-shot multipliers plus either the image embedding's top principal components, fit on training folds only, or a 12-way k-means "light cluster" of it.

**Result** (`bench/lab_wb.py`, 3,719 pairs, 105 days; Temp+Tint share in brackets):

| | k = 0 | k = 1 | k = 3 |
|---|---|---|---|
| base | 9.813 (24.3) | 9.172 (19.4) | 8.610 (15.9) |
| + as-shot WB | 9.810 (24.3) | 9.160 (19.1) | 8.589 (15.5) |
| + 16 embedding PCs | 9.803 (23.9) | 9.195 (19.6) | 8.640 (16.0) |
| + WB + 16 PCs | 9.794 (23.9) | 9.181 (19.3) | 8.619 (15.6) |
| + light cluster | 9.832 (24.3) | 9.213 (19.6) | 8.667 (16.2) |

**Killed.** The embedding trims Temp+Tint a little with no heroes (24.3 to 23.9) and then gets in the way once heroes exist, because it widens every slider's model, not just white balance. The best change anywhere is 0.02. Greg's white balance is a per-Shoot choice that the heroes already carry; the picture doesn't predict it better than his own edits of the same day.

### N9. A stock VLM rating taste on the full card (2026-10-24, Mac)

**Question.** The default cull is rules plus the Taste head (no model). Does a stock vision model reading the Contact sheet (Qwen 3.8 27B, variant `bench/variants/whole-cull.json`) rank Greg's picks better? It's the only vision model on the Mac; no downloads.

**Result** (`just bench --only cull`, AUC of stars against his picks, best Photos picked):

| Shoot | Qwen rates taste | rules + Taste head |
|---|---|---|
| 2023-05-20 | 0.59, 5/8 | 0.51, 4/10 |
| 2024-08-10 | 0.57, 8/8 | 0.44, 7/10 |
| 2024-10-19 | 0.58, 1/4 | 0.57, 3/10 |
| 2025-03-22 | 0.54, 6/8 | 0.61, 8/10 |
| 2026-10-10 | 0.61, 12/25 | 0.65, 11/26 |

Mean AUC 0.578 against 0.556, worse on two of five Shoots, and 36-46 min per 100 photos (122 min for 261) against 11 s. Asking the model about doubtful frames only (`ARGUS_CULL_CHECK=model`) changed nothing: it rejected three frames and AUC moved by at most 0.01.

**Verdict: not worth it.** A 0.02 edge that flips sign by Shoot, at about 200 times the time, sits well inside the 0.66 ceiling C1 found for any label of Greg's. The default stays. A stronger or newer VLM might change this; it would be a download for Greg to approve.

### N10. AI heroes: the AI edits 3 frames, the Shoot match carries the rest (2026-11-01)

**Question** (thread "Speed up the AI import pass", Greg: "let's experiment with all this to see if we can improve efficiency/speed without compromising quality"). A model take per frame is about 95% of a card's AI time. If the AI edits only the three frames `suggest_heroes` picks and the Shoot match carries them, is the rest as good as the head alone, and as good as a take on every frame?

**Setup.** `bench/lab_ai_heroes.py`. On the Mac, for 10 test-fold days (gf-storage RAWs): a scratch Library per day, `suggest_heroes` 3, one Natural take (claude-code, as a first proxy for Qwen) on the heroes and on 3 random probe frames, 17 min for all 60 takes. In the cloud: the GP Shoot model trained on folds 1-4 predicts every non-hero frame, with the heroes' sliders taken from Greg's own edits or from the AI's takes. Linearised dE of the Basic sliders against Greg's edits.

**Result.**

| | non-hero frames (292) | probe frames (30) |
|---|---|---|
| Edit head alone | 10.16 | 9.51 |
| Shoot match, no heroes | 10.21 | 9.43 |
| AI heroes, k = 1 | 10.29 | 9.55 |
| AI heroes, k = 3 | 10.17 | 9.44 |
| head's own sliders as heroes, k = 3 | 10.13 | 9.50 |
| AI take on every frame | | 9.75 |
| Greg's edits of the same heroes, k = 1 | 8.86 | 7.99 |
| Greg's edits of the same heroes, k = 3 | **7.94** | **7.51** |

**Verdict: holds for speed, adds nothing for quality.** Carrying three AI heroes scores the same as the head alone (10.17 vs 10.16) and a little better than a take on every frame on the probes (9.44 vs 9.75, only 30 frames). So per-frame takes can go without a loss on this measure. But the AI's heroes carry none of Greg's Shoot style: they land exactly where the head's own sliders do. The head with no model call is as good, and Greg's own three edits are worth 2.2 dE. Caveat: this scores Basic sliders only. A Natural take also brings Masks, curves and colour that this doesn't see, so it can't say whether Greg would prefer per-frame takes by eye.

### Net D (cull) is blocked on clean data

A cull fine-tune needs keeps and rejects rendered the same way. Favorites are Greg's edits (the edited-look leak), and the drive's rejects exist only as 768 px features, not images. The pairs' unedited renders give keepers as shot; rejects need `drive_bench.py` to save its renders. And per C1, the within-burst label ceiling is about 0.6, so a cull net can only help whole-Shoot ranking. Left for after the edit nets.

## 2026-10-25 (overnight, Greg: "keep trying new promising angles, don't give up")

### H1. Which photos should Greg edit first? (`suggest_heroes`, landed 80c7a11)

**Question.** Shoot match learns a Shoot's look from the photos the person edits by hand. Which ones should those be?

**Setup.** `bench/lab_edit.py --anchors random|typical|mi|tmi`, Shoot match (within-gp) on 5,050 cloud pairs (155 days, folds 1-4), linearised dE. *typical* = nearest the Shoot's centre in the facts; *mi* = greedy, each next hero the one that most lowers the model's total uncertainty over the rest; *tmi* = typical first, then greedy.

**Result** (the real GP after the 7a5655d fix; lower is better; the Edit head alone, no hero, is ~10.0):

| heroes | random | typical | typical, then greedy | greedy (what ships) |
|---|---|---|---|---|
| 1 | 9.48 | **9.00** | 9.00 | |
| 2 | 9.03 | 8.81 | 8.64 | **8.61** |
| 3 | 8.75 | 8.64 | 8.50 | **8.47** |

Picking on purpose is worth about one edited photo: two chosen heroes beat three random ones. The held-out test fold (scored once) agrees: one hero random 9.59, greedy 9.16, typical **9.04**; three heroes random 9.01, typical 8.81, greedy **8.46**, which is exactly the shipped rule. The first run of this screen used the similarity-weighted model by mistake (see N15's correction) and agreed: random 9.56 / 9.12 / 8.82 / 8.46 at 1 / 2 / 3 / 5, greedy 8.71 / 8.55 / 8.20 at 2 / 3 / 5. On the Mac's retrained model (7,140 pairs, test fold) random heroes give k1 9.36, k3 8.86 against the head's 10.02.

**Verdict: shipped as a tool.** `suggest_heroes` returns the most typical frame for one hero and the greedy choice for more, counting photos already edited by hand. No UI yet (needs a mockup).

### N10. Lightroom-style local Highlights and Shadows

**Hypothesis.** Lightroom's Highlights and Shadows move an edge-aware base layer, so local contrast survives Greg's signature -63 / +47; Argus's move every pixel through one tone curve (`pipeline::tone_sliders`). If that is part of the ~5 dE floor, doing it Lightroom's way should lower the best-slider fit.

**Setup.** A throwaway engine switch (`ARGUS_LOCAL_TONE=frac`) applied the Highlights and Shadows bumps to a guided-filter base of the luma (radius 2%, 6% or 15% of the long edge) and kept the detail on top. `argus edit-fit` with `fit: true` on the 99 cloud bench RAWs, once per setting.

**Result.** Floor 5.181 with the curve; 5.178, 5.191, 5.194 local (all within ± 0.05, better on 38-44 of 99). The fit hardly uses the two sliders anyway (mean Highlights -10, Shadows -9 with the curve), because Argus's own Look already compresses highlights and lifts shadows.

**Verdict: killed.** The switch was removed. Whatever the floor is, it isn't how Highlights and Shadows are drawn.

### N11. Greg's own Lightroom settings, rendered through Argus

**Question.** E5 said (in linearised slider space) that knowing Greg's Lightroom settings is barely better than the Shoot match. Does that hold on real renders, and can a learned translation from his settings to Argus's sliders close the gap?

**Setup.** `bench/lab_translate.py`: 83 cloud bench RAWs whose XMP settings are in the pairs. A ridge (and LightGBM) from his settings plus the Photo's facts to the best Argus Basic sliders, trained on 2,765 pairs from 148 capture days with no bench Photo. Rendered with `edit-fit`, block dE76 to his JPEG.

**Result.**

| take | dE | vs head |
|---|---|---|
| Auto + Edit head (no hero) | 9.23 | |
| his Basic sliders copied straight | 12.57 | +3.34 |
| ... plus his curve, HSL, grading, Clarity, vignette | 12.33 | +3.09 |
| translation from his settings alone | 9.06 | -0.18 |
| translation from settings + facts (ridge) | 8.23 | -1.00 |
| same, LightGBM | 8.09 | -1.14 |
| best sliders (floor) | 5.10 | -4.13 |

**Verdict: E5 holds on pixels.** Copying Lightroom's numbers into Argus is worse than doing nothing clever; even a learned translation that knows his exact settings lands at 8.1, about where the Shoot match is with three heroes, three dE above the floor. The facts carry most of the translation (9.06 → 8.23), so the two renderers part ways per photo. An "import my Lightroom edits" feature can't be a slider mapping; it would have to fit sliders to each exported JPEG (`edit-fit`, 5.1).

Side check (`out/lab/align/align.py`): letting the render shift, zoom and rotate onto his JPEG lowers the floor only 5.25 → 5.00, and only 4 of 99 bench Photos move by more than 1 dE (two crops, one 2° straighten). Bench alignment is not where the floor hides, though those few outliers (14.8 → 7.2) are worth dropping from training.

### N12. Is the default Look why identical settings need different Exposure?

**Finding first** (`out/lab/expo/same_crs.py`): 26,230 pairs of same-day frames Greg edited with identical Basic settings (no HSL or curve) need Argus Exposures that differ by 0.45 EV (sd). 56% of that is explained by the unedited render: frames with more clipped highlights need *more* Argus Exposure (r 0.70 with the clipped share). Temp differs by 23 between such frames and nothing in the render explains it.

**Setup.** A research switch in `edit-fit` (`ARGUS_LOOK=ev,knee,lift,contrast,sat`), fit on the 83 bench RAWs with his settings, and the share of (fit Exposure − his Exposure) the scene explains (`bench/lab_look_default.py`).

**Result.** The highlight shoulder doesn't matter (knee 0.6-0.95: same floor, same scene share). The Look's strong shadow lift does: lift 1.118 (default) gap sd 0.458, scene R² 0.41; lift 0.75 0.397 / 0.32; 0.5 0.364 / 0.22; 0 0.348 / 0.13. The floor barely moves (4.99 default, 4.96 at 0.75, 5.00 at 0.5, 5.13 at 0), but plain Auto, tuned to the default Look, swings by ±1 dE.

**Verdict: explains it, not shippable tonight.** Argus answers the same settings more like Lightroom with a gentler built-in shadow lift, at no cost to the floor. That would make Sync and copy-the-settings habits carry over better in the app, but every model (Auto, Edit head, Shoot match) and every golden is tuned to today's Look, so it's a Look redesign for Greg to decide on, not a tweak.

### N13. Carry the hero's brightness, not its Exposure number

**Hypothesis.** If Greg edits frames to look as bright as the rest of the Shoot, copy the hero's sliders and solve this frame's Exposure so it lands at the hero's finished brightness.

**Setup.** `bench/lab_brightness.py`: 84 ordered (hero, frame) pairs of bench RAWs from the same day (26 days). Hero edit = its best sliders; Exposure solved from a ladder of renders.

**Result.** Sync 10.15, brightness-matched 14.50 (+4.35 ± 1.37, better on 17/84), halfway 11.42. His finished mean L* differs by 13.3 (sd) between frames of one day, more than the frames' own spread.

**Verdict: killed.** He doesn't hold a Shoot's brightness; he edits each scene to its own key.

### C6. A stronger image model for the Taste head (SigLIP 2)

**Setup.** `bench/cull_siglip.py` on the Mac: the five bench cull Shoots embedded by SigLIP 2 base (Apache 2.0) in place of the engine's MobileNet, `taste_head.py` unchanged, clean 2026-10-10 card held out.

**Result** (star AUC against his picks, engine rule = faces + centroid head):

| | MobileNet | SigLIP 2 |
|---|---|---|
| cold (other Shoots only) | 0.683 | 0.705 |
| first 20 flags | 0.684 | 0.729 |
| history + first 20 | 0.703 | 0.729 |
| first 40 | 0.722 | 0.724 |
| first 80 | 0.743 | 0.737 |
| first 130 | 0.666 | 0.649 |

**Verdict: small early edge, not worth a model download.** SigLIP helps the cold start and the first 20 flags by 0.02-0.045 on one Shoot (within its noise), and the edge is gone by 40 flags. Burst pairs aren't comparable (stacks come from the embedding, 504 vs 76 pairs). Revisit if another model brings more than this, or with a second clean card.

### N14. Does Argus read the camera's white balance like Lightroom?

**Question.** Same-day frames Greg edited with identical Lightroom settings still need Argus Temps that differ by 23 (sd). When the camera's as-shot white balance was identical too, the gap drops to 2.8. Either Argus follows the camera's auto white balance differently from Lightroom (an engine bug), or Greg pinned a fixed Kelvin that Argus's relative Temp has to chase.

**Setup.** `bench/lab_asshot.py` on the Mac reads `crs:WhiteBalance`, Temperature and Tint from each pair's XMP (7,235 pairs) and compares same-day frames with identical Basic settings and the same white-balance mode (and the same Kelvin, unless As Shot).

**Result.** Greg sets a Custom Kelvin on 4,877 pairs and leaves As Shot on 2,358. For Custom frames the Argus Temp gap has sd 23.0 and tracks the camera's drift, -199 Temp per unit of as-shot log(R/B) (r -0.87). For As Shot frames it is sd 6.4 with slope -0.5 (r -0.01), so Argus follows the camera exactly as Lightroom does.

**Verdict: no engine bug.** The miss is Greg's absolute Kelvin meeting Argus's relative Temp. Sync and Shoot match copy relative values, so a frame where the camera's auto white balance wandered inherits the wrong cast. Modelling every photo as absolute (`lab_edit.py --kelvin`) made Shoot match worse (k1 9.56 to 10.15, k3 8.82 to 9.16 at 107 Temp per log(R/B)), because a third of his frames are As Shot. Absolute only where he set a Custom Kelvin (`--wb-modes`, Mac, 3,841 of 5,671 pairs) was worse still: k1 9.63 to 11.24 (150 Temp per log R/B) or 12.18 (199), k3 8.87 to 9.90 or 10.57.

**Why.** He re-sets the Kelvin as the light changes. On 139 Custom days with 3+ frames he used a median of 3 Kelvins and one Kelvin all day on only 23% of days. Within a day his log Kelvin follows the camera's as-shot log(R/B) at r +0.87, while the Argus Temp he needs barely does (r -0.14). The camera's auto white balance is already a good proxy for where he puts the Kelvin, so Argus's Temp relative to as-shot is the right thing to copy. The 23-point gap only shows up between frames where he kept one Kelvin but the camera drifted, a minority.

**Verdict: killed, and the relative Temp stays.**

### C7. "Looks like photos Greg edited before" as a cull signal

**Setup.** `out/lab/keeps/knn.py` (scratch): each frame of the five cull Shoots scored by its mean cosine similarity to its 1, 10 or 50 nearest MobileNet embeddings among ~7k photos Greg edited (the pairs, same day excluded).

**Result.** Star AUC on the clean 2026-10-10 card 0.59-0.61 alone, faces + kNN 0.667 against faces 0.654 and the engine's cold 0.683. On the other Shoots it swings from 0.40 to 0.75 with no pattern (they are edited previews, so the domain differs too).

**Verdict: killed.** What he chose to edit over four years says little about which frames of a new card he keeps.


### N15. Would a gentler default Look make edits carry across a Shoot?

**Hypothesis.** N12: today's strong shadow lift is why identical Lightroom settings need different Argus Exposures. With less lift his edits should be more alike in Argus's sliders, so Shoot match and Sync should copy them better.

**Setup.** `bench/lab_look_shoot.py` on the Mac refit the same 1,601 pairs (60 days, 8+ frames a day, test fold out) under three Looks (`ARGUS_LOOK`, lift 1.118 today, 0.75, 0.5), and `lab_edit.py --override` scored each with the head and the models retrained on those fits.

**Result** (lin dE, 1,584 scored pairs):

| Look | head | GP, 1 hero | GP, 3 heroes |
|---|---|---|---|
| today (lift 1.118) | 11.06 | 9.91 | 9.04 |
| lift 0.75 | 10.94 | 9.81 | 8.98 |
| lift 0.5 | 10.89 | 9.76 | 8.94 |

**Verdict: real but small.** A gentler lift helps every row, by 0.17 with no hero and 0.10 with three, at no cost to the floor (N12). Too small to justify re-tuning Auto, the head, Shoot match and every golden on its own. Worth folding in if the Look is redesigned for another reason. Caveat: the linearised score uses sensitivities measured under today's Look.

**Correction (2026-10-25).** Until 7a5655d, `lab_edit.py` had no method called `within-gp` or `sync`: both names silently ran its similarity-weighted path (within-sim). Overnight numbers labelled within-gp from `lab_edit.py` (H1, N14's `--kelvin` runs) are that path; the Mac screen numbers from `shoot_style.py` (9.36 / 8.86) are the real GP. Rerun with the GP: base k1 9.57, k3 8.82 (within-sim 9.63, 8.87); Custom-only absolute white balance at 150 still loses (11.14, 9.95). Conclusions unchanged.

### N16 and N17. Where the Shoot match error is left, and two tries at it

**Where it is.** Splitting the GP Shoot match's squared error by slider (cloud, 5,050 pairs): with no hero, the floor is 21%, Exposure 45%, Temp 13%, everything else 2-5% each. With three heroes: floor 30%, Exposure 40%, Temp 12%. Exposure is the one lever that matters.

**N16, capture order.** Frames shot close together share the light, so part of the GP's smooth kernel followed the distance in frame numbers (same camera, `lab_edit.py --time share,frames`). Base k1 9.48, k3 8.75; every setting tried (share 0.5-0.8, scale 5-80 frames) was equal or worse (best 9.49, 8.76). The facts already say "same scene".

**N17, per-slider GP settings.** Exposure and Temp got their own rho and length scale (`--gp-slider`). Nothing beat the shared (0.8, 0.45) by more than 0.01; shorter or longer length scales lost 0.02-0.13.

**Verdict: both killed.** The Shoot model's settings are at their best; what is left in Exposure is Greg choosing a different brightness for each scene (N13), which no fact here predicts.

## Open leads

- Render-gap work (other thread): per-photo Lightroom-vs-Argus translation is most of the remaining edit error on this bench.
- Ship E4 + E8 into `engine::shoot_match` if Greg wants the small gain.
- "Start with this one": suggest the most typical frame as the first hero.
- Cull: whole-Shoot ranking with a stronger embedding on the Mac (SigLIP 2, licence check), not burst ranking.
- An Argus-native edit bench: Greg's Recipes made in Argus, so heroes and targets share one renderer.
