# The Argus benchmark

Greg wants the best benchmark there is for AI culling and editing of one photographer's work (2026-10-22: "Just keep intaking these data into our benches as much as possible"). This page lists what we hold, what each label really means, how we split it so a score can't cheat, and how new data gets in. The "Benches and review page" thread owns it. Update it in the same change that adds data.

## What we hold

**Cull, Lightroom Shoots** (`fixtures/bench/cull/<day>/`, `bench/extract.py`, `bench/argus_shoot.py`). Five Shoots: four of 100 consecutive frames (2023-05-20, 2024-08-10, 2024-10-19, 2025-03-22) plus the 261-frame A7R V card 2026-10-10. The label is pick or not picked, because cloud Lightroom deletes rejects. The four older Shoots are Lightroom previews with Greg's edit applied. Only 2026-10-10 is unedited (rendered as shot), so it is the held-out test Shoot.

**Cull, the drive** (`bench/drive_bench.py` on the Mac, features in the Mac checkout's `out/drive-bench/<day>.json|.npy`). Greg's backups of the frames he rejected (4,082) and left unflagged (3,174) on gf-storage, plus his favorites from iCloud. Days with at least 10 frames of two kinds: 88 days and 5,800 frames (3,637 rejects, 1,432 unflagged, 731 favorites), the ex period included for scoring. Held out by day (`drive_eval.py`, 2026-10-23), reject vs unflagged comes from only 26 of those days: the Taste head's centroid scores 0.73 and sharpness 0.42 across a whole day, but inside bursts (174 pairs) sharpness wins with 0.67. Keeper vs reject inside bursts (1,499 pairs) reaches 0.70 with the logistic head, though that comparison leaks the edited look. Every frame carries its Facts and embedding, as the cull sees them at 768 px. Unlike the Lightroom Shoots, this is the only data with **real rejects**. Treat reject vs unflagged as mostly a content split, though: a Taste direction learnt from it runs backwards on 2026-10-10 (AUC 0.36), so don't tune taste on it (`bench/cull_taste.py`, docs/research/cull-taste-experiments.md, 2026-10-23).

On 2026-10-23 the page got 577 same-scene pairs from 62 of these days (`drive_pairs.py`, ex period left out), as `<day>_Backup` Shoots on the Blind cull tab.

Cull mode in the app (2026-11-01) logs every keep as a `moment` in the Usage log: the frames shown side by side, which he kept, and whether the AI's Best chip was on screen. `argus tool export_usage_bench` writes them to `cull/<Shoot>/choices.json`, and `python3 bench/cull_pairs.py moments <choices.json>...` turns each into head-to-head rows (kept beats every unkept frame in the same Stack) in `fixtures/bench/usage/<Shoot>.pairwise.json`. Rows with `blind: false` saw the AI's pick, so they count for less than the review page's blind taps. Each row also keeps `pass`: 2 and up come from a Round (Round 2 restacks the Picks), so those rows rank good frames against each other.

**Cull, Greg's blind choices** (the review page's `cull_blind` rows, imported by `bench/cull_pairs.py import` into `fixtures/bench/cull/<shoot>/pairwise.json` and `fixtures/bench/drive/<shoot>.pairwise.json`). "If you could keep only one, which?" between two same-scene frames, with "Can't tell" as the tie (`same`). Greg found the old keep both / one / neither wording confusing (2026-10-24), so the page now asks the forced choice; older `both` and `neither` rows are ties too. This is the cleanest cull label we have: same moment, both unedited, chosen today. `bench/burst_keeper.py` reads faces on every tapped frame and scores head-to-head keeper rules on these taps beside the card and drive bursts (docs/research/burst-keeper.md, 2026-10-24).

**Edit, bench cases** (`fixtures/bench/edit/cases.json`). 140 RAWs with Greg's finished edit as a 640 px Lightroom preview. dE to his edit rewards timid edits, so read it beside his taps.

**Edit, RAW and favorite pairs** (`bench/pairs.py`, thread "RAW and edit pairs"). Every favorite in `core/core-photos` (8,249) joined to its catalog asset and, where Greg exported the original, its RAW, checked by sha256. The rolling set is the Mac checkout's `out/pairs-raw/pairs.json`, rewritten as the month-by-month export continues, with Greg's sliders as `edit.crs`, his XMP and `fold` from `bench/splits.py`. Pairs from hidden days stay in it for training, marked `"hidden": true`, and never go anywhere Greg votes. On 2026-10-22 it held 3,548 pairs, 2,667 with a RAW, and 505 RAW pairs in the test fold (mostly 2025 and 2017; bodies A7R V, A7R II, Q3). RAWs stay on gf-storage.

**Edit, Greg's blind choices** (the review page's `blind` rows, read with ArtifactData). Unlabeled A/B between two edits of one photo, sides shuffled, about one in eight a mirrored repeat. Questions with a hypothesis each live in `bench/questions.py`. A black-and-white edit never meets a colour one (Greg, 2026-10-24: they are two different choices; B&W vs B&W is a fair comparison). The run's bw flag misses some, his own B&W Lightroom edits included, so `questions.py` also calls a render B&W when its 95th percentile chroma is under 8 and lists them in questions.json `mono`; the page, its tallies and its predictor drop mixed pairs and old taps on them. The page stops asking once a question settles. Next up spends each tap where it is worth most (Greg, 2026-10-24): an open question's value is its `weight` in `questions.py` (how much a live decision rides on it) times the knowledge gradient (how far one more answer is expected to push the estimate across 50%), repeats are 1 in 20 and only on open questions, settled questions are re-checked only when nothing else is left, and cull pairs come in tiers by what they test (the Taste head siding against an old reject first, two rejects or Adobe's scores last). Questions that can't be asked yet wait in `PARKED`. Runs that are random draws of one experiment (the Shoot match page, three draws of hero edits on the same 72 photos, 2026-10-24) share a `series` in their meta, and a question with `each_run` asks one pair per draw per photo, both arms from the same draw. Only images some question pair uses are packed (pairs under the floor are dropped first), at 1280 px, so a 600 MB research folder fits the page's 256 MB version. It learns while Greg votes (2026-10-23): a predictor guesses each answer, a question's estimate is the predictor's average over all its pairs plus its average miss on the answered ones (a difference estimator, unbiased however bad the predictor is), and the next pair is drawn more often where the predictor is unsure, with the draw weight saved on the row (`pi`, 1 = uniform; `pred` is the guess). The predictor is `runs/predict.json` when the taste judge publishes one (`{model, p: {"<photo>|<item>|<item>": chance the first item of the sorted key wins, a tie counting half}}`, items as `<run>:<key>`, the page's pairKey), else a logistic fit of Greg's picks on the other questions over dE to his edit, brightness and Claude's call. Each question trusts the predictor only as far as his answers track it (power tuning: the slope of answers on guesses, clipped to 0..1), so a poor predictor can't make a question noisier than counting. On 182 answers (2026-10-23) the taste judge saves about 7% of taps and the fallback about 5%; the page shows the share. `bench/taste_judge.py predict` regenerates the file each nightly run.

## What the labels mean

A Lightroom pick means kept and chosen. Not picked means kept but not chosen, never rejected. A drive reject is a frame Greg flagged X before Lightroom deleted it. Unflagged means he never flagged the frame either way. It is not a keep. A favorite means he edited it and exported it to core-photos.

His choices are noisy and get better with repeats. On mirrored repeats he agreed with himself 66% of the time (small n) by 2026-10-22. The left photo wins 52% of 632 picks, so the page's side shuffle is enough.

## Splits: nothing scored by a model that saw it

Every learned score is held out **by capture day**. `bench/splits.py` is the one split every bench uses: `fold(day)` hashes the capture day into 5 folds, so the split never moves as data arrives, and fold 0 is the test fold. Tune and choose on folds 1-4, score fold 0 once per decision. A burst's neighbours are almost the same picture, so a random split leaks. `bench/drive_eval.py` and `bench/taste_head.py` hold out one day at a time. `bench/pairs.py match` writes capture-day folds for edit training. Per-day AUC is averaged weighted by each day's pairs. Pooling scores across days measures how a whole day's scores sit, which no cull ever compares (on the drive's first 26 days, pooling moved one AUC from 0.36 to 0.07).

## Leaks we know about

The big one is the **edited look**. Favorites and the four old Lightroom Shoots carry Greg's edit, while rejects and unflagged frames don't. Any keeper vs reject score partly measures "is this edited". Only reject vs unflagged, Greg's blind cull choices and 2026-10-10 are free of it. `drive_eval.py` labels the leaky comparisons, and `drive_pairs.py` never pairs a favorite on the review page.

A model judging its own work favours it, so the default judge is not the model that made the takes. Claude agrees with Greg on 61 of 98 judged pairs. Greg's taps decide.

## Privacy

Greg doesn't want to see photos of his ex while he votes (2026-10-22). Training, benches and scores may use them. So capture days from 2022-04-01 to 2024-04-30 (`splits.HIDDEN`) never go on the review page or anything else he votes on (the page also drops any case or Shoot from those days itself, in case an old run still holds one), and photos in `fixtures/bench/removed.json` stay out of the shown cases. The review page's Remove button feeds `bench/remove_photos.py`. Originals and the Lightroom catalog are only ever read.

## Coverage

To fill in once the drive run and the favorites read finish: frames per year, per camera body (A7R V, A7R II, Leica Q3, phone) and per scene type (people, landscape, indoor, night), for each dataset above. Gaps there decide what to ask Greg for next.

## Adding data

1. Read only from the source, on the Mac, at background priority (`taskpolicy -b nice -n 20`, through `scripts/progress`). Skip cloud-only iCloud files instead of downloading them. The Mac's disk is tight.
2. Write features per capture day so a run resumes, and keep the source path so a pair can be exported later.
3. Score it held out by day with the leak called out. Add a line to `docs/blog/results-log.md` when the number is notable.
4. Put the most useful pairs on the review page (`bench/drive_pairs.py`, `bench/cull_pairs.py`, `bench/questions.py`), at least 1400 px and unedited on both sides for cull pairs. The page holds at most 256 MB and 511 files per version, so retire packs nothing points at.

## Writing a bench script

Import `ROOT`, `BENCH`, `argus()` (the CLI wrapper, honours `ARGUS_BIN`) and `library_facts()` from `bench/common.py` rather than copying them, and call the engine through `argus` instead of re-implementing it in Python. dE to Greg's edits lives in `score.py` (`distance`); reuse it so every number means the same thing.
