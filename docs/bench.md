# The Argus benchmark

Greg wants the best benchmark there is for AI culling and editing of one photographer's work (2026-10-22: "Just keep intaking these data into our benches as much as possible"). This page lists what we hold, what each label really means, how we split it so a score can't cheat, and how new data gets in. The "Benches and review page" thread owns it. Update it in the same change that adds data.

## What we hold

**Cull, Lightroom Shoots** (`fixtures/bench/cull/<day>/`, `bench/extract.py`, `bench/argus_shoot.py`). Five Shoots: four of 100 consecutive frames (2023-05-20, 2024-08-10, 2024-10-19, 2025-03-22) plus the 261-frame A7R V card 2026-10-10. The label is pick or not picked, because cloud Lightroom deletes rejects. The four older Shoots are Lightroom previews with Greg's edit applied. Only 2026-10-10 is unedited (rendered as shot), so it is the held-out test Shoot.

**Cull, the drive** (`bench/drive_bench.py` on the Mac, features in the Mac checkout's `out/drive-bench/<day>.json|.npy`). Greg's backups of the frames he rejected (4,082) and left unflagged (3,174) on gf-storage, plus his favorites from iCloud. Days with at least 10 frames of two kinds: 62 days and 4,712 frames (3,487 rejects, 668 unflagged, 559 favorites). Every frame carries its Facts and embedding, as the cull sees them at 768 px. Unlike the Lightroom Shoots, this is the only data with **real rejects**.

**Cull, Greg's blind choices** (the review page's `cull_blind` rows, imported by `bench/cull_pairs.py import` into `fixtures/bench/cull/<shoot>/pairwise.json` and `fixtures/bench/drive/<shoot>.pairwise.json`). "Which would you keep?" between two same-scene frames. This is the cleanest cull label we have: same moment, both unedited, chosen today.

**Edit, bench cases** (`fixtures/bench/edit/cases.json`). 140 RAWs with Greg's finished edit as a 640 px Lightroom preview. dE to his edit rewards timid edits, so read it beside his taps.

**Edit, RAW and favorite pairs** (`bench/pairs.py`, thread "RAW and edit pairs"). Every favorite in `core/core-photos` (8,249) joined to its catalog asset and, where Greg exported the original, its RAW, checked by sha256. The rolling set is the Mac checkout's `out/pairs-raw/pairs.json`, rewritten as the month-by-month export continues, with Greg's sliders as `edit.crs`, his XMP and `fold` from `bench/splits.py`. Hidden days are dropped from it for training too. On 2026-10-22 it held 3,548 pairs, 2,667 with a RAW, and 505 RAW pairs in the test fold (mostly 2025 and 2017; bodies A7R V, A7R II, Q3). RAWs stay on gf-storage.

**Edit, Greg's blind choices** (the review page's `blind` rows, read with ArtifactData). Unlabeled A/B between two edits of one photo, sides shuffled, about one in eight a mirrored repeat. Questions with a hypothesis each live in `bench/questions.py`. The page stops asking once a question settles.

## What the labels mean

A Lightroom pick means kept and chosen. Not picked means kept but not chosen, never rejected. A drive reject is a frame Greg flagged X before Lightroom deleted it. Unflagged means he never flagged the frame either way. It is not a keep. A favorite means he edited it and exported it to core-photos.

His choices are noisy and get better with repeats. On mirrored repeats he agreed with himself 66% of the time (small n) by 2026-10-22. The left photo wins 52% of 632 picks, so the page's side shuffle is enough.

## Splits: nothing scored by a model that saw it

Every learned score is held out **by capture day**. `bench/splits.py` is the one split every bench uses: `fold(day)` hashes the capture day into 5 folds, so the split never moves as data arrives, and fold 0 is the test fold. Tune and choose on folds 1-4, score fold 0 once per decision. A burst's neighbours are almost the same picture, so a random split leaks. `bench/drive_eval.py` and `bench/taste_head.py` hold out one day at a time. `bench/pairs.py match` writes capture-day folds for edit training. Per-day AUC is averaged weighted by each day's pairs. Pooling scores across days measures how a whole day's scores sit, which no cull ever compares (on the drive's first 26 days, pooling moved one AUC from 0.36 to 0.07).

## Leaks we know about

The big one is the **edited look**. Favorites and the four old Lightroom Shoots carry Greg's edit, while rejects and unflagged frames don't. Any keeper vs reject score partly measures "is this edited". Only reject vs unflagged, Greg's blind cull choices and 2026-10-10 are free of it. `drive_eval.py` labels the leaky comparisons, and `drive_pairs.py` never pairs a favorite on the review page.

A model judging its own work favours it, so the default judge is not the model that made the takes. Claude agrees with Greg on 61 of 98 judged pairs. Greg's taps decide.

## Privacy

Greg doesn't want to see photos of his ex while he votes (2026-10-22). Training, benches and scores may use them. So capture days from 2022-04-01 to 2024-04-30 (`splits.HIDDEN`) never go on the review page or anything else he votes on, and photos in `fixtures/bench/removed.json` stay out of the shown cases. The review page's Remove button feeds `bench/remove_photos.py`. Originals and the Lightroom catalog are only ever read.

## Coverage

To fill in once the drive run and the favorites read finish: frames per year, per camera body (A7R V, A7R II, Leica Q3, phone) and per scene type (people, landscape, indoor, night), for each dataset above. Gaps there decide what to ask Greg for next.

## Adding data

1. Read only from the source, on the Mac, at background priority (`taskpolicy -b nice -n 20`, through `scripts/progress`). Skip cloud-only iCloud files instead of downloading them. The Mac's disk is tight.
2. Write features per capture day so a run resumes, and keep the source path so a pair can be exported later.
3. Score it held out by day with the leak called out. Add a line to `docs/blog/results-log.md` when the number is notable.
4. Put the most useful pairs on the review page (`bench/drive_pairs.py`, `bench/cull_pairs.py`, `bench/questions.py`), at least 1400 px and unedited on both sides for cull pairs. The page holds at most 256 MB and 511 files per version, so retire packs nothing points at.

## Writing a bench script

Import `ROOT`, `BENCH`, `argus()` (the CLI wrapper, honours `ARGUS_BIN`) and `library_facts()` from `bench/common.py` rather than copying them, and call the engine through `argus` instead of re-implementing it in Python. dE to Greg's edits lives in `score.py` (`distance`); reuse it so every number means the same thing.
