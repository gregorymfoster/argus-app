# Edit as you cull: Keepers first and pre-edits (2026-10-23)

Greg asked for "edit as you cull" (2026-10-23, item 6 of the coordinator's list): as he flags a Shoot, Argus learns that Shoot's taste and look live, re-ranks the frames ahead and pre-edits the rest before he gets to them. Mockup: the design canvas boards "Edit as you cull · Loupe" and "· grid" (https://claude.ai/artifact/DL71SAHY8XDUA6FgK2XxcY).

## What shipped

Keepers first is a Sort choice (⇧K, also a toggle in the Loupe header). `rerate_shoot`, which already refreshes the AI's stars from the Taste head 1.2 s after flagging pauses, now also returns `order`: every Photo but the Rejects, likeliest keeper first (`taste::keepers_first`, the same ranking the stars come from). The prototype holds one order for the open Shoot (`data/keepers.ts`) and merges each new one in only after the Photo the person is on, so what they passed and the frame under the cursor never move, flagged frames ahead keep their place behind the unflagged ones, and Stacks stay together (`sortPhotos`). The filmstrip marks where Up next starts and rings frames that just moved up; the footer says how many moved.

Pre-edits: a hand edit of a RAW (the `edit` command, Author Person) queues `pre_edit_shoot`, as does every `rerate_shoot`. The job gives every Pick and likely keeper (4★ and up, not a Reject) the person hasn't edited a Shoot match Suggestion (`engine::pre_edit`): the hero is the RAW they edited last, its Recipe minus Crop, Masks, Spots, one-click fixes and Lens Blur is the base (Lightroom's Sync defaults), and only the Basic sliders are matched to each frame through `shoot_match` with up to 8 hand-edited anchors. No model call; anchors are measured once per run. A match older than the newest hand edit is stale and worked out again; an accepted take, a dismissed Shoot match or other matched takes ("apply to the rest") are left alone. The grid shows "Shoot match ready for N Photos" with Accept all (one Undo) and Show them; the Loupe shows a chip, \ swaps between the match and the frame as it is, A accepts.

## Replay on the clean card

`bench/keepers_first.py` on bench/cull_features.py's dumps (768 px). Greg reviews the 2026-10-10 card one frame at a time, flagging each as he did (keepers Pick, the rest Reject).

| order | 25% | 50% | 75% | 90% | keepers in first 26 | first 52 | mean position |
|---|---|---|---|---|---|---|---|
| capture time | 79 | 151 | 196 | 234 | 10 | 22 | 0.534 |
| sorted once (faces + history head) | 44 | 96 | 155 | 207 | 20 | 35 | 0.402 |
| Keepers first (re-scored every flag) | 37 | 83 | 155 | 203 | 20 | 40 | 0.380 |
| only Picks pressed, no Rejects | 44 | 96 | 155 | 207 | 20 | 35 | 0.402 |
| ceiling | 31 | 62 | 93 | 112 | 26 | 52 | 0.237 |

Columns 25% to 90%: frames reviewed before that share of his 124 keepers had come up.

Most of the gain is the first sort; the live re-ranking adds 13 frames at the halfway mark and 5 keepers in the first 52. The tail (75% and on) doesn't move: the last keepers look like the frames he drops. One card, no Stacks in the replay (the app keeps Stacks together, which costs a little). The head learns only against Rejects, so a person who presses P and skips the rest gets the sorted-once order; counting frames passed over in Keepers first as soft Rejects is the obvious next try, once a real cull log shows how he flags.

Not measured here: how good the pre-edits are. That is the Shoot match bench (docs/research/auto-edit-research.md: one hero 8.7 dE from his own edit against 10.0 for Sync, held out by day).
