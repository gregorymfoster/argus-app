# Cull taste experiments: edited means kept, burst-relative, Greg's taps

Greg picked these on 2026-10-23 ("lets try 3, 4, 5, 7, 8, 9"; 3 to 5 are edit-model work in the "Scene-aware edit model" thread). All three try to teach the cull's Taste from data we already hold, with no new taps from Greg. Script: `bench/cull_taste.py`, which prints every number below.

## How it was scored

Only on clean labels, never on a frame its model trained on:

- **The 2026-10-10 card**: 261 unedited frames at 1600 px (the Mac's `out/cull-features-hd`), 124 picks. AUC of pick vs not picked across the card, and inside bursts (114 pairs, `engine::bursts` rule).
- **Greg's blind taps**: 159 decisive "which would you keep?" answers on that card from the review page's `cull_blind` rows, plus 22 on 2023-05-20 and 3 on the new Backup Shoots.
- **Drive reject vs unflagged**: held out by `bench/splits.py` fold. It turned out to be a poor taste label (see below), so it is reported but not used to choose.

The comparison is what ships today: `engine::taste` blends z(faces) with a centroid direction learnt from the history Shoots' picks (the four old Lightroom Shoots here). On the card that scores AUC 0.66 with 20 of its top 26 frames being picks.

## 7. Edited means kept

Every favorite is a keep, and the rest of that capture day on the drive (unflagged and rejects) are drops: 88 days, 731 favorites. Favorites are Greg's edits, so a head trained on them can learn "edited look". To remove that, a second run swaps each favorite's embedding for its RAW as shot (from `bench/pair_features.py`); 390 favorites had one, the rest were dropped from that run.

| Taste score on the card | AUC | top 26 picks | taps (159) |
|---|---|---|---|
| faces alone | 0.62 | 13 | 0.59 |
| today: faces + old Shoots | 0.66 | 20 | 0.55 |
| faces + edited=kept (RAW) | 0.69 | 18 | 0.60 |
| faces + old Shoots + edited=kept (RAW) | **0.70** | **21** | 0.55 |

Adding the edited=kept direction to today's blend gains 0.035 AUC, paired bootstrap 95% interval 0.011 to 0.058, on this one card. The JPEG version scores a little lower (0.68 with faces) and shows the leak plainly: it ranks the drive's rejects above its unflagged frames (AUC 0.18), while the RAW version doesn't (0.57). Logistic heads did worse than centroids everywhere, as before. Inside bursts no head beats chance (0.44 to 0.56), and on the taps every difference is inside the ±0.08 noise.

So edited=kept helps the whole-card ranking a little and nothing inside bursts. The in-app version is cheap and has no leak: Argus computes embeddings from the unedited preview, so a Photo the person edited (or exported) can count as an implicit Pick for the history direction.

## 8. Burst-relative scoring

Each frame's features (sharpness, eye focus, eyes open, smile, awkward face, heads upright, face size, brightness, shake streak, how typical it looks for its burst, faces score) compared with its burst mates, with a pairwise logistic model on the differences trained on the drive's burst pairs.

The drive has only 76 clean burst pairs (reject vs unflagged), and single features disagree between datasets: sharpness wins 0.74 of the drive's burst pairs but 0.41 of the card's; eyes open wins 0.64 of the taps but 0.42 of card bursts. Only heads upright holds in both (0.65 drive, 0.68 card), and it already orders bursts in `taste::rate`. Pairwise models trained on drive bursts score 0.47 to 0.56 on card bursts and 0.43 to 0.52 on taps. No gain, and nothing to ship.

## 9. Training on Greg's taps

The same pairwise model trained on the 159 card taps, five folds blocked by scene so a fold never scores a scene it trained on: 0.57 (±0.08), against faces alone at 0.59 on the same taps. Trained on all 159, it gets 0.68 of the 22 taps on 2023-05-20 (±0.21). Adding embedding components (0.55) or the drive's bursts (0.54) didn't help. Too few taps to learn from yet: a few hundred more, spread over many Shoots, are needed before this can beat faces. The 577 Backup pairs published on 2026-10-23 are the place to get them (3 answered so far).

## The drive's reject vs unflagged is mostly content

A direction learnt from it scores 0.60 to 0.73 on held-out drive days but runs backwards on the card (0.36) and on the taps (0.47). The faces score, too, ranks the drive's rejects above its unflagged frames. Rejects and unflagged frames come from different kinds of moments, so this pair says little about which frame Greg keeps. Don't tune taste on it.

## What would move these

- Edited=kept with many more days: the unedited neighbours of the other ~5,000 RAW pairs live in the backup Lightroom library on gf-storage (unplugged on 2026-10-23).
- More clean cards like 2026-10-10. Every number here rests on one card.
- More blind taps, for 8 and 9.

## Shipped

Greg chose to ship 7 (2026-10-23). `engine::taste::flags` now also reads the person's edits: on any Shoot where they edited some frames, edited frames count as kept and unedited frames they didn't pick as dropped. `Head::learn` adds that direction, at unit length, beside the other Shoots' flags and the card's own. Only Recipe Versions by the person count. Tests: `engine/tests/taste.rs`.
