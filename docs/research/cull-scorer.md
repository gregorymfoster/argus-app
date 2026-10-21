# A taste scorer for the cull: first try (2026-10-19)

Question: can a small scorer on frozen image embeddings, plus cheap technical checks (sharpness, eyes, bursts), rank Greg's picks better than Qwen's stars, so the VLM isn't doing everything?

How it was tested. `bench/cull_features.py` imports each cull bench Shoot into a scratch Library and dumps every Analyzer Fact (signals, faces, eyes, segment, sky, the 1280-number embedding). `bench/taste.py` trains L2 logistic regressions with one Shoot held out and scores pick AUC on it, for technical features, PCA'd embeddings, and both.

## The bench leaks Greg's edits

The cull bench photos are Lightroom previews with Greg's develop applied, and Greg edits the photos he picks. On 2023-05-20, the one Shoot whose develop settings the catalog stores locally, all 46 picks carry an edit and none of the 54 others do. On the other Shoots the pixels show it: picks average brightness 0.64 against 0.48 on 2024-08-10, and "fewest crushed shadows" alone ranks picks at AUC 0.90 on 2025-03-22.

So the first result (technical features held-out AUC 0.77 against Qwen 0.58 and Adobe aesthetics 0.62) is the scorer learning "looks edited". Dropping the tone features (brightness, clipping, crushed) kills it: nothing beats chance across Shoots.

| held-out AUC | 05-20 | 08-10 | 10-19 | 03-22 | mean |
|---|---|---|---|---|---|
| technical + tone (leaky) | .55 | .82 | .92 | .79 | .77 |
| technical, no tone | ~.50 | ~.54 | ~.51 | ~.48 | ~.51 |
| embedding PCA 8 | .46 | .67 | .58 | .52 | .56 |
| Adobe aesthetics | .49 | .89 | .62 | .49 | .62 |
| Qwen stars (overnight) | .73 | .54 | .59 | .47 | .58 |

Any model that looks at these pixels, Qwen and Claude included, gets the same hint, so cull bench numbers flatter tone-sensitive judges until the bench has unedited frames.

## Within a burst

The cleaner task is "which frame of this burst did Greg keep". The four Shoots hold only 109 pick-vs-unpicked pairs inside one burst (90 of them on 2023-05-20), too few to train on. Where there are enough, the sharpest frame of its burst is Greg's 78% of the time (2023-05-20, iPhone, unedited unpicked frames, so tone can't explain it).

## A clean Shoot: 2026-10-10

Lightroom keeps Smart Previews only for what Greg kept, so it can't give unedited rejects (`bench/asshot.py` found none for the old Shoots). The 266-RAW A7R V card of 2026-10-10 is in Greg's Argus Library whole, and he culled it in Lightroom before Argus existed. `bench/argus_shoot.py` rendered all 261 frames of that day as shot with `sips` and labelled the 124 he kept as picks (b4f2b63). Here the not-picked frames are real rejects, and nothing is edited.

| on 2026-10-10 | AUC | keepers rejected |
|---|---|---|
| Qwen 3.8 27B, overnight app cull | .54 | 75 of 124 |
| Qwen 3.8 27B, cull bench at Full, built-in prompt (6f9e756) | .49 | recall 20% |
| Qwen 3.8 27B, cull bench at Full, named-flaw prompt (e2ba0b8) | .48 | 14 of 124 (30 rejects) |
| Claude (claude-code), built-in prompt | .58 | 66 of 124 (154 rejects) |
| Claude, "cautious rejects" prompt | .61 | 8 of 124 (16 rejects) |
| Claude Sonnet, named-flaw prompt | .48 | 78 of 124 (148 rejects) |
| Claude Haiku, named-flaw prompt | .47 | 100 of 124 (204 rejects) |
| technical checks (sharpness, eyes, bursts) | ~.50 | |
| scorer trained on the 4 old Shoots, no tone | .61 | |
| embeddings, trained on other frames of this Shoot | .60-.65 | |

Nothing predicts Greg's keeps well, and sharpness and eyes say nothing here (a sharp camera, a careful shooter). The worst failure was the rejects. Both models rejected about 60% of the card, barely better than chance (57% of Claude's rejects were frames Greg dropped, against a 52% base rate), so a cull hid half of Greg's keepers. The built-in prompt now rejects only for a named flaw (`bench/variants/cautious-rejects.json` is the same text as a variant). That cut wrong rejects from 66 to 8 with Claude, and pick precision rose from 57% to 62%. On Qwen the same rule cut rejects from 75 to 30, but 14 of those 30 were still Greg's keepers, and its ranking stays at chance (AUC .48). Qwen 27B can't tell Greg's keepers from his rejects on this card; Claude does a little better.

A taste scorer trained on Greg's flags inside one Shoot reaches about .60 after he flags the first 20-30% of it. That's better than both VLMs, but too weak to pick for him yet.

Only Opus beats chance. Sonnet and Haiku ignore the named-flaw rule and reject most of the card, so a reject from a smaller model needs a check outside the prompt.

Obvious damage is a different story. In the cull_degrade run (5 Shoots, neutral file names since 461c52f, Qwen at Full), Qwen rejected all 24 damaged twins (blur, shake, dark, bad crop) and starred the original above its twin 23 times of 24. So a local pass is trustworthy for clear technical rejects even though its taste is at chance. That suggests a split: Qwen or the technical checks name the frames with an obvious flaw on the Mac, and Opus (or the person's own flags) decides taste among the rest.

Caveat (2026-10-20): every portrait frame of the 2026-10-10 fixtures was sideways (sips kept the sensor orientation; fixed in 414ece4), so all 2026-10-10 numbers above, for every model, need a rerun on the upright fixtures.

Faces (2026-10-20): what the face model adds and how it scores on this card is in `docs/research/cull-faces.md`.

## Decision (Greg, 2026-10-20)

Qwen only for now, and a cautious cull. The AI rates every Photo, rejects only a 1-star frame with a named technical flaw, and never picks (`cautious()` in engine/src/cull.rs enforces it whatever the model answers). Greg does the real cull by stars. So the cull bench's numbers that matter are star AUC and keepers near the top of a star sort, wrong rejects (should be about zero) and damaged twins caught, not Verdict agreement.

On the upright fixtures with the faces-first prompt and eyes v2 (2489745), Qwen at Full on 1600 px sheets (whole-cull variant, ceb3fd5, 2026-10-20) reached star AUC .61, put 56% of Greg's keepers in the top k of a star sort, chose 17 of 25 Best Photos from his picks and wrongly rejected none. That matches Opus's .61 from before the fixes. It took 75 minutes for the 261 Photos.

The two-stage cull now on main (Technical check by Qwen, Taste from faces; fdd211a) ranks as well (AUC .62, top .57) in 40 minutes, rejects none of Greg's keepers, and on the damaged twins ranks the original higher 29 of 30 times. Its Best Photos hit fewer of his picks (12 of 26) than the whole cull's (17 of 25).

The shorter stars-first variant did a little worse on the same sheets (AUC .59, top .55, Best 19/26 picks, 2 wrong rejects), so the built-in prompt stays.

## Decision models (Jev and friends)

Jev (TypeSafe) is a hosted, closed, text-only "decision model": it returns calibrated label probabilities instead of text, and its paper's pattern is to accept the verdict when confident and escalate to a reasoning judge when unsure. It needs an API key and can't see pixels, so it doesn't fit Argus. Its open counterparts (GLiNER2.5-Decide 340M and Laya, Apache; OpenJev, MIT) are text-only too and could only decide over captions and Facts, which say little about Greg's keeps. Open photo aesthetics and reward models are mostly trained on AVA, KonIQ and similar non-commercial sets, which the licence rule excludes.

The pattern does fit, built image-native: a small calibrated scorer on the embedding Analyzer, trained on the person's own flags. On 2026-10-10 (5-fold, trained on other frames of the same Shoot) its most confident 20% of calls are 81% right (61% overall), so it can flag the confident frames and send the rest to Opus or leave them unflagged.

## What it needs

- Unedited cull frames: as-shot renders (camera JPEG, or a Smart Preview DNG once Argus decodes linear DNGs), not Lightroom's edited previews. Until then, read cull bench numbers as biased toward tone.
- Many more Shoots, so leave-one-Shoot-out says something, and burst-level labels.
- Rerun: `python3 bench/cull_features.py && python3 bench/taste.py` (`--with-tone` shows the leak).
