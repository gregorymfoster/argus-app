# A personal taste judge from Greg's taps (2026-10-23)

Greg asked for a "Greg judge": a model trained on his own choices that can score edits, so the AI can try several and keep the one he'd like. Thread "Personal taste judge". Code is `bench/taste_judge.py`; the fitted weights are `fixtures/bench/taste-judge.json`.

## What it is

The judge looks at two versions of the same photo and says which one Greg prefers. Each version is reduced to 30 global statistics measured on a 256 px copy: tone percentiles and clipping, overall and shadow/highlight colour, chroma, a crude skin mask's brightness and colour, local contrast at two scales, large-scale tonal spread and centre-versus-border brightness. A pairwise logistic model (Bradley-Terry) scores the difference. Squared terms let it learn an ideal amount, and a few interaction terms let that ideal move with the scene's brightness, colour, contrast and skin share. It never sees Greg's own edit of the photo, so it works on a fresh card in the app.

It's tiny. Training takes about a second on a laptop CPU, scoring a pair takes milliseconds, and it needs no model call, no download and no network.

## Data

The review page's `blind` collection held 798 edit taps on 146 photos (669 decisive, 129 "can't tell"). Images for 525 of the decisive pairs still exist: the published packs, the model-judges composites on Greg's Mac (split back into A and B), and the Qwen Natural trio renders in the Mac's `out/bench`. The other 144 point at cloud runs whose renders are gone.

Every score below is held out by capture day (5 folds by a hash of the day), so the judge never trains on a tap from the same day as the one it predicts. Settings (which terms, how much regularisation) are chosen inside each training fold only.

## Results

| On Greg's held-out taps | agreement | n |
|---|---|---|
| Judge, all decisive pairs | 65.5% (61-70) | 525 |
| AI take vs AI take | 62% | 285 |
| Edit vs no edit | 65% | 132 |
| Involving Greg's own edit | 76% | 108 |
| Closer-to-Greg's-edit baseline (needs his edit) | 56% | 397 |
| Judge on those same 397 | 63% | 397 |

Head to head on the 319 pairs that Claude and Codex also judged (the model-judges packs, Greg's answer as recorded there):

| Judge | agreement |
|---|---|
| Taste judge (held out) | 68.0% (217 of 319) |
| Codex (gpt-6.1-sol) | 68.1% (213 of 313) |
| Opus | 64.2% (197 of 307) |
| Sonnet | 62.3% (187 of 300) |
| Majority of taste judge, Codex and Opus | 72.7% (232 of 319) |

So a one-second numpy model trained on his taps already ties the best outside model, and voting it with the two strongest LLM judges beats any one of them. It is also honest about its doubt. Split into quarters by confidence, it agrees with Greg 56%, 66%, 68% and 73% of the time, so the least-sure pairs are exactly the ones worth asking him.

What it learned (non-Greg pairs, single statistics): Greg leans toward more colour (chroma wins 58%), more local contrast and texture (61%), deeper blacks, cooler highlights, and slightly darker midtones (the brighter mid loses 56% of the time). That matches his Lightroom habit of pulling highlights hard and adding vibrance.

## Things that didn't help

Teaching it "Greg's edit beats every other version" made it worse, not better: 59% on non-Greg pairs fell to 52-54%. His edits are Lightroom renders and the candidates are Argus renders, so those pairs teach the judge what Lightroom's rendering looks like rather than what he prefers between two Argus edits. That's the render gap from `auto-edit-research.md` turning up again. Leave his finished edits out of judge training until both sides come from the same renderer.

The MobileNet embedding (differences projected on the top 4 to 32 within-photo edit directions) added nothing at 525 taps. More taps are worth more than more features right now.

## Best-of-N

`taste_judge.py bestofn` lets the judge (trained only on other days' taps) pick one candidate per photo and compares the pick's distance to Greg's edit with the run's first take. Unedited versions are not allowed as picks.

| Candidates | photos | judge pick minus first take |
|---|---|---|
| Qwen takes (Favorites runs: Auto + 2-3 takes) | 83 | -0.48 dE (95% -0.71 to -0.25), better on 22, worse on 7 |
| Scene head vs Auto | 73 | +0.17 dE (-0.09 to +0.46), better on 4, worse on 14 |
| Shoot match / Sync / Edit head (k=1 and 3) | 60 | -0.31 dE (-1.07 to +0.41), better on 22, worse on 23 |

It helps choose among a model's takes and doesn't help (or slightly hurts, by this proxy) when the candidates are already fitted to his edits. Distance to his edit only agrees with his taps 56% of the time, so these dE numbers are a weak referee. The real test is blind pairs of "judge's pick vs first take" on the review page.

## Shipped

Bench only, no app change yet: `bench/tune.py --judge taste` (or `taste,codex`) uses the judge in sweeps and super edits, free and instant. Nothing goes into the app's Suggestions until Greg's taps confirm judge picks beat the first take.

## The cull half

Greg's cull taps haven't grown since the cull taste experiments (159 decisive on 2026-10-10, about 90 elsewhere), where a pairwise model on them scored .57 against the faces score's .59 (`cull-taste-experiments.md`). Training a cull judge on the same taps would repeat that result, so it waits for a few hundred more cull taps.

## Next

1. Review page: a question "judge's pick vs first take" on new Qwen/Claude takes, and active selection that serves the pairs the judge is least sure about (owner: "Benches and review page").
2. Refit with `taste_judge.py fit` as taps grow. The curve is still rising: every few hundred taps should be worth a point or two.
3. When the lab's backbone net is trained, swap its pooled features in for the 30 statistics and see whether the judge passes 70% alone.
4. Once a Greg's-edit Argus re-render exists (his exact settings through our renderer), retry the implicit "his edit wins" pairs; the failure above should disappear when both sides share a renderer.

## Reproduce

    # taps: ArtifactData list `blind` from the review page into TAPS/blind/
    # images: TAPS-referenced renders as IMGS/<photo>__<run>:<key>.jpg (greg:greg = his edit)
    python3 bench/taste_judge.py features IMGS feats.npz
    python3 bench/taste_judge.py eval feats.npz TAPS
    python3 bench/taste_judge.py bestofn feats.npz TAPS /mnt/project-files/argus-bench/report/runs --no-none
    python3 bench/taste_judge.py fit feats.npz TAPS

The 512 px image set and features used here are in `/mnt/project-files/argus-bench/taste-judge/`.
