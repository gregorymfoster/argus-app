# Picking the burst keeper by faces (2026-10-24)

Greg, tapping through cull pairs on the review page: "when they're very similar ... the more faces visible, the more smiles, the more clear eyes, less cut off, and the more in focus. These different factors are more likely to keep. When calling it, just give head-to-head." So this tried exactly that, as a pairwise call between two frames rather than a score per photo, and asked whether it picks his keeper more often than the burst order Argus ships today (upright heads first, then the faces' taste, then the sharper frame; `engine::taste::rate`).

## Data

Three label sets, all scored by `bench/burst_keeper.py score`:

- **Taps.** His blind "if you could keep only one" answers from the review page, decisive ones only: 279 pairs, 159 from the 2026-10-10 card and 120 from 2023-05-20 and the drive's Backup Shoots. Faces are read on the 1600 px card frames and on the 1280 px pair images the page shows (`facts` subcommand, cloud CPU, about 2.5 minutes).
- **Card bursts.** 2026-10-10 burst pairs, his pick against a frame he didn't pick: 114 pairs at 1600 px. Heads upright was chosen on this card, so the shipped order is flattered here.
- **Drive bursts.** The drive's burst pairs, unflagged against rejected: 76 pairs, faces read at 768 px.

The five factors per frame: faces the face model can read at least half the size of the largest (background people and backs of heads don't count), smiling faces (smile at least 0.3), faces with open eyes, faces whose box sits on a side or the bottom edge or whose head top (half a face above the box) leaves the frame, and frame then eye sharpness (one frame 25% sharper than the other). Each pair is called by a vote, one per factor.

## Results

Share of pairs ordered as Greg did (a call the rule can't make counts half):

| Rule | Taps (279) | Card bursts (114) | Drive bursts (76) |
|---|---|---|---|
| Shipped order | .559 | .684 | .645 |
| Greg's five, one vote each, then shipped order | .595 | .588 | .539 |
| Same, without the face count | .606 | .588 | .553 |
| Pairwise logistic on all factors, held out by day | .636 | .605 | .592 |
| Fewer cut-off heads, then shipped order | **.581** | **.684** | **.645** |

The full five-factor vote helps a little on the taps and loses clearly in both burst sets; pooled over all 469 pairs it is 0.019 below the shipped order. The fitted model, trained only on other capture days, is 0.018 above it pooled, with a 95% interval of -0.033 to +0.074: not a win yet.

Taken one at a time, counting faces runs backwards (.48 of the 71 tap pairs where the counts differ, and negative in a fit on every set). When the count changes inside a near-duplicate pair it usually means the framing changed, a step closer or wider, and Greg tends to keep the tighter frame. Smiles (.54 of 90) and open eyes (.56 of 97) lean his way on the taps but sit at chance in the bursts, as they did in the earlier burst rounds (docs/research/taste-head.md). Sharpness agrees on the taps away from the card (.58) but not on it.

Cut-off heads are the exception. In all 12 pairs where the two frames cut a different number of heads (10 taps, 2 drive bursts, none on the card) Greg kept the one that cut fewer. It is a small sample, but the effect is the same on every set it appears in, and putting it first never changes a pair where the counts are equal. Pooled gain 0.013 with a 95% interval of +0.004 to +0.023.

## What shipped

`engine::taste::cut_off` counts the main faces a frame cuts, and inside a burst `taste::rate` now puts the frame that cuts fewer heads first, then the most upright heads, then taste. The `SheetPhoto` carries the count beside `tilt`. Tests: `cull::inside_a_burst_a_frame_that_cuts_off_a_head_goes_after_one_that_doesnt` and `cull::a_head_at_the_frames_edge_or_above_its_top_counts_as_cut_off`.

Not shipped: the face count, smiles, open eyes and sharpness votes. Greg's stated rule is in `docs/agents/preferences.md` so the next round starts from it.

## Next

More taps decide this. The fitted head-to-head model is the most promising thing here (+.08 on the taps) and needs a few hundred more decisive taps across many days before its interval clears zero. Rerun `burst_keeper.py facts` then `score` after each batch of taps; ship the fitted model only when it beats the shipped order pooled, held out by day. A face count that tracks the same people across the burst (rather than counting faces per frame) would test what Greg meant by "faces visible" more directly.
