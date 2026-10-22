# Technical check and Taste: the split cull (2026-10-20)

Greg asked for this after the cull numbers came in: split the technical checks from taste, ask Qwen only the technical part, lean on cheap signals that already work (faces), and spend his taps on the pairs where the AI judges disagree.

## Why split

Qwen is good at one half of a cull and no better than a coin at the other. On `cull_degrade` it rejected all 24 damaged twins (blur, shake, underexposure, a bad crop) and starred the original above its twin 23 times out of 24. On the clean 2026-10-10 card, its stars ranked Greg's 124 keepers at AUC .54 with the old prompt and .48 with the cautious one. Claude Opus does a little better on taste (.66 to .67 on the 1600 px photos), but Greg chose to keep culls on Qwen.

So the cull now runs in two stages. The model's **Technical check** answers one question per Photo, broken or not, with a two or three word reason. Its reply is a verdict and a reason, so it is much shorter than a full cull's (no stars, Stacks or Best Photos to write), and a local cull should finish sooner. **Taste** (`engine/src/taste.rs`) then fills in what the model left empty: stars, the order of each burst's Stack, and about one Best Photo in ten. It uses Facts the Analyzers already computed and no model at all.

## What Taste measures

The main faces (`eyes::main_faces`): their mean smile minus the share of them that look off (`eyes::awkward`: a half-blink without a laugh, mouth mid-word, looking down, turned away, frowning, a big face that is soft). A frame without a readable face scores 0, which sits between "someone looks off" and "everyone smiles". Stars come from where a frame falls among the Shoot's kept frames: top tenth 5, next quarter 4, then 3, bottom 30% 2. Rejects get 1. A Shoot where everything scores the same (a landscape day) gets 3 stars throughout and no Best Photos, because there is nothing to tell them apart and inventing an order would mislead.

Sharpness and exposure were tried and left out. Within the sound frames they don't rank keepers (sharpness AUC .49 on 2026-10-10, exposure .44 to .46), which makes sense: they are technical, and the Technical check already acts on them.

## Numbers

Star AUC against Greg's picks, 640 px fixtures (`bench/cull_features.py`, then the scorer replicated offline):

| Shoot | kept | Taste stars | Qwen stars (overnight) |
|---|---|---|---|
| 2026-10-10 (clean) | 48% | .65 | .54 |
| 2023-05-20 | 46% | .52 | .73 |
| 2024-08-10 | 87% | .44 | .54 |
| 2024-10-19 | 35% | .55 | .59 |
| 2025-03-22 | 49% | .59 | .47 |

Only the first row is a fair test. The four older Shoots are Lightroom previews that carry Greg's edits, and he edits his picks, so any model that looks at the image can read the answer off the tone (tone features alone reach .77 there, see `cull-scorer.md`). 2024-08-10 is also a Shoot where he kept 87%, which leaves little to rank. Even so the mixed old rows are a reason to keep measuring on clean cards before trusting Taste further.

Inside bursts, neither Taste nor sharpness picks Greg's frame better than chance (51% and 48% of 104 keep-versus-drop pairs on 2026-10-10). Bursts still need a better signal.

## How to compare

The split is the default. `bench/variants/whole-cull.json` (`"cull_taste": "model"`) is the built-in prompt with the model rating taste, as before. A variant with its own `cull` prompt (`stars-first.json`, `cautious-rejects.json`) keeps rating taste by the model unless it sets `cull_taste`. Taste needs no model, so its stars can also be rescored offline from any run's rejects and `cull_features.py` output.

## Taps where the judges split

`bench/model_judges/judges.py order` sorts the review page's experiment set so the pairs Opus, Sonnet and Codex disagree on come first: 49 of the 100 wide pairs split the three judges, 46 have all three agreeing. The page asks experiment pairs in file order, so the reordered `runs/experiment.json` (page v38) is all it took. A tap on a pair every judge agrees on mostly confirms them; a tap on a split pair tells us which judge to trust.

## Qwen on Greg's Mac (night of 2026-10-20)

Same 1600 px photos of 2026-10-10, Qwen 3.8 27B at Full, one run each:

| harness | star AUC | top-k | Best Photos he kept | wrong rejects | time |
|---|---|---|---|---|---|
| pre-faces whole cull | .56 | .52 | 17 of 25 | 2 | |
| faces whole cull (`whole-cull.json`) | .61 | .56 | 17 of 25 | 0 | |
| faces, stars-first prompt | .59 | .55 | 19 of 26 | 2 | |
| split (Technical check + Taste) | .62 | .57 | 12 of 26 | 3 | 43 min |

The split ranks keepers best and is the quickest, but two things lost ground. Its Best Photos are weak (12 of 26 is under the 48% base rate), so the faces score is fine for sorting a Shoot and poor for naming its very best frames. And the Technical check rejected three of Greg's keepers, all for half-shut eyes. Since Taste already marks a half-blink down, the prompt now rejects only eyes that are fully closed without a laugh.

The twin check on the split (30 damaged twins over 5 Shoots) rejected 9 of 10 blurred, all 10 shaken and all 5 badly cropped twins, but none of the 5 darkened ones, and Taste starred those at or above their originals. Leaving a 2 EV-under RAW unrejected is defensible, since it can be rescued, but it must not outrank the frame it copies. Taste now takes a full point off a frame more than a third darker than the median of the frames shot within 30 seconds of it; on Greg's five bench cards only 4 of 661 frames fall under that line, none of them keepers.

Rerun on fdd211a the next morning: same AUC (.62, top-k .57, Best Photos still 12 of 26), now 0 wrong rejects, 40 min. Its twin check ranked the original above its twin 29 of 30 times (24 before, 27 for the faces whole cull), including 4 of the 5 dark twins. It rejected 21 of 30 (blur 9, shake 10, crop 2), against 21 for the whole cull too; the badly cropped twins it stopped rejecting still rank below their originals, which is what a conservative cull wants. Files: `/mnt/project-files/argus-bench/cull-qwen/`.

The next night (2026-10-22, 97a27a9) the stars-first prompt ran with Qwen's own Technical check unioned in (`ARGUS_CULL_CHECK=model`), 57 min: it rejected 23 of 30 twins (blur 9, shake 10, dark 1, crop 3) and ranked the original above its twin 27 of 30 times, dark 4 of 5. That is more rejects than the split (21) but a worse ordering than its 29 of 30, so the split stays the default.

Since 2026-10-21 the default Technical check is rules on stored Facts, with Qwen only as an opt-in second opinion. The night of 2026-10-22 (97a27a9, all five bench Shoots, Full) compared the two: rules-only rejected none of Greg's picks, while adding Qwen's rejects cost 16 wrong rejects (10 on 2024-08-10, 4 on 2025-03-22, 2 on 2026-10-10). Star AUC was the same or better with rules (.51/.51, .44/.43, .57/.57, .61/.58, .65/.65 rules/model) and Best Photos identical, because Taste sets both. Rules took 10 s for the five Shoots, Qwen 1 h 36 min. Qwen's second opinion only adds wrong rejects here, so rules-only stays the default.

## Next

A Qwen run of the split on the 1600 px 2026-10-10 photos and on `cull_degrade`, to confirm the Technical check holds up without the rest of the prompt. Then more clean, people-heavy Shoots, and the learned taste head on embeddings (`/mnt/project-files/research/local-decision-models.md`) once Greg wants it, which would give no-face frames and bursts a signal too.
