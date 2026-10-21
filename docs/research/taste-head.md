# The Taste head: learning keeps from the person's own flags (2026-10-21)

Greg said go on the learned head that `/mnt/project-files/research/local-decision-models.md` had recommended twice. The split cull already asked Qwen only for technical rejects and left Taste to the faces (`cull-taste.md`). Faces rank Greg's keepers at AUC .65 on the clean 2026-10-10 card, but they are poor at naming the very best frames (12 of 26 Best Photos in the Mac run were his), and they say nothing at all about a frame without people.

## What shipped

`engine::taste::Head` is a nearest-centroid head on the MobileNetV3 embedding every Photo already has. At each cull it reads every Pick and Reject the person made themselves (the AI's own flags never count), centres each embedding on its Shoot's mean, and takes the mean of the Picks minus the mean of the Rejects. Flags from other Shoots give one such direction and flags already made on the card being culled give another. Each is scaled to unit length and the two are summed, so a long history can't drown out what the person just told us about this card. Either part needs five Picks and five Rejects before it counts.

Taste is then the faces score and the head's score given equal weight (standard scores within the Shoot, summed). With no flags to learn from, Taste is exactly the faces score it was before. With flags but no faces anywhere, the head alone orders the frames, so a landscape Shoot now gets real stars instead of level 3s.

Training is an average, so it costs a few milliseconds and runs fresh on every cull. Nothing is stored, nothing new is downloaded, and no model licence changes (the embedder is Apache 2.0 and already shipped).

## How it was measured

`bench/taste_head.py` on `bench/cull_features.py` dumps (640 px fixtures, no model provider). The test card is always 2026-10-10: 261 unedited frames, 124 of them Greg's picks. No score below comes from a head that saw the frames it scores.

"Cold" trains only on the four older bench Shoots, which is what a new card gets from a Library's history. Those Shoots are Lightroom previews that carry Greg's edits, so they can teach tone that won't carry over to an unedited card; that can only hurt the test score, never inflate it. "First k" is learn-as-you-cull: trained on the first k frames of the card in capture order, scored on the rest. "Top tenth" is the share of Greg's picks among the tenth of scored frames the method ranks highest, a stand-in for Best Photos (base rate 48%). "Bursts" counts pairs inside a burst where Greg kept one frame and dropped the other.

| Taste | trained on | AUC | top tenth his | bursts |
|---|---|---|---|---|
| faces (before) | nothing | .65 | 38% | 51% of 76 |
| faces + head | 4 other Shoots | .69 | 77% | 57% of 76 |
| faces (before) | nothing, scored on the last 181 | .69 | 61% | 66% of 41 |
| faces + head | first 80 frames of the card | .74 | 100% | 63% of 41 |
| faces + head | 4 other Shoots + first 80 | .73 | 83% | 63% of 41 |
| faces + head | first 40 frames of the card | .72 | 82% | 53% of 57 |

The head lifts ranking a little and the top of the ranking a lot. On the whole card, 20 of the 26 frames it ranks highest are Greg's picks, against 10 for the faces alone. That is the Best Photos problem the split cull had.

Leave-one-Shoot-out on the four older Shoots (leaky, so only a sanity check) never lowered AUC: 2024-08-10 went from .42 to .64, 2023-05-20 from .52 to .53, and the other two stayed level. The top tenth on 2025-03-22 dropped from 80% to 60% (10 frames, so two photos).

## What it doesn't fix

Bursts. Inside a burst the frames share a scene, so their embeddings barely differ, and the head moves the pair score from 51% to 57% on 76 pairs, which is within noise of a coin. A burst keeper needs a pairwise signal (eye focus, expression change, Greg's Survey choices); that is still open.

A logistic head on the full embedding scored within a point or two of the centroid on every split, with a regularisation knob to tune and nothing to show for it, so the centroid shipped. Off-the-shelf aesthetic scorers are still out on licence (`local-decision-models.md`).

Greg's taps on the edit review page were not used. They compare edits of the same photo, so they say which look he prefers and nothing about which frame he would keep.

## Caveats

One clean card. The 77% top tenth is 20 of 26 frames, and the 100% after 80 flags is 18 of 18; both could move a lot on another card. More clean cards culled in Argus itself are what would make this trustworthy, and they also make the head better, since every flag Greg sets is a training example.

`bench/score.py` culls into a fresh scratch Library with no flags, so its cull numbers still measure the faces alone. To measure the head end to end there, a run would need to seed the scratch Library with Greg's flags from the other Shoots first.

The 640 px fixtures miss small faces in group shots; the faces half of the score is stronger on the 1600 px copies.

## Burst signals: eye sharpness and expression (2026-10-21, afternoon)

Greg asked to try the two cheap burst signals next. Both are fixed rules, nothing trained, compared only between frames of one burst (`bench/burst_signals.py`). I chose among the variants on the four older Shoots and report them on 2026-10-10, so the clean card can't tune the choice. A pair is a burst where Greg kept one frame and dropped another; ties and frames without a face count half.

| signal | older Shoots (109 pairs) | 2026-10-10 (76 pairs) |
|---|---|---|
| eye sharpness (eye focus times frame sharpness, mean of main faces) | .56 | .52 |
| same, weakest face | .56 | .48 |
| eye focus as stored (relative to the frame) | .51 | .50 |
| smile | .49 | .48 |
| eyes open | .54 | .51 |
| smile minus blink | .55 | .38 |
| nobody looks off | .49 | .53 |
| frame sharpness (reference) | .61 | .45 |

Neither idea beats chance on the clean card, so neither went into the engine. Restricting to pairs where both frames have a readable face (main face at least a tenth of the frame) didn't change that: 25 pairs on 2026-10-10, eye sharpness .40, eyes open .56.

"Smile minus blink" running backwards (.38, 27 to 46) fits what we already knew: Greg keeps laughs where the eyes squeeze shut, and inside a burst the laughing frame is often the blinkiest one.

The likely limit is the fixtures. At 640 px, faces in group shots are a few dozen pixels wide, and the eye focus and blendshapes are read from that. The 1600 px copies of 2026-10-10 live on Greg's Mac (`out/cull-hd2`); `cull_features.py --photos hd --edge 1600` then `burst_signals.py` there would settle whether these signals are noise or just unreadable at this size. It is light (no model), about five minutes.

## Burst heuristics, round 2 (2026-10-21)

Greg asked for more heuristics. Everything here is free from stored Facts and was scored the same way (`bench/burst_signals.py`, older Shoots / 2026-10-10, share of keep-versus-drop pairs):

| heuristic | older Shoots | 2026-10-10 |
|---|---|---|
| worst face in the group (smile minus looks off) | .50 | .62 (44 to 25) |
| peak motion (frame that differs most from its neighbours) | .54 | .60 (41 to 26) |
| later in the burst | .25 | .58 |
| middle of the burst | .54 | .57 |
| facing the camera | .47 | .55 |
| looking at the camera (gaze) | .51 | .41 |
| most typical frame of the burst | .51 | .46 |
| worst face's eyes open | .56 | .50 |
| no face cut off by the frame edge | too few pairs | too few pairs |

Nothing passes the rule set in advance (pick on the older Shoots, then beat chance on the clean card). The worst-face rule is the best on 2026-10-10 but flat on the older Shoots. Peak motion is the only one above .5 on both, and only weakly (86 to 63 pairs combined, and pairs inside one burst aren't independent). Burst position flips sign between the old Shoots and the new one, so it is about how bursts were exported, not about Greg.

Whole-Shoot "scene interest" (how many frames of a scene he shot, how unlike the rest a frame is) scored .60 to .61 alone on 2026-10-10, but added to the faces and Taste head it moved 2026-10-10 by under a point and cost 2025-03-22 six. Left out.

Worst face and peak motion are the two to recheck on the 1600 px copies, where group faces are readable.

## Round 3: measured on the pictures (2026-10-21)

Greg asked for more. `bench/burst_image.py` measures the bench JPEGs themselves (sharpness inside the face box and the person around it, colourfulness, contrast, how level the frame is), adds face geometry from the stored face boxes (face size, placement near a third, how far heads lean) and blendshapes (a cheek-raised genuine smile, mouth at rest, relaxed brows), and timing (the pause before and after a frame). Same scoring: inside bursts, older Shoots first, then 2026-10-10.

| heuristic | bursts, older Shoots (109) | bursts, 2026-10-10 (76) |
|---|---|---|
| heads upright (eye line level) | .56 | **.70 (53 to 23)** |
| biggest face in the burst | .51 | .61 |
| face near a third | .44 | .58 |
| subject sharpness | .70 | .49 |
| face sharpness | .61 | .41 |
| contrast | .60 | .47 |
| colourfulness | .67 | .38 |
| level frame | .47 | .45 |
| mouth at rest / brows relaxed | .50 / .46 | .51 / .48 |
| pause after / before the frame | .07 / .89 | .55 / .52 |

**Heads upright** is the first burst signal that passes: above chance on the older Shoots and well above on the clean card (sign test p < .01 on 2026-10-10, which survives the roughly 30 rules tried across the three rounds). A tilted head is usually a head mid-motion. It is now how `taste::rate` orders the frames of a burst (`taste::tilt`, from the YuNet face boxes already stored); a frame with no face found counts as the burst's average and taste decides the rest. Added to the whole-Shoot ranking it didn't help (2026-10-10 AUC .685 to .674), so it stays a burst-only signal.

The older Shoots disagree with the clean card on everything tonal and on sharpness, which is the edit leak again: those previews carry Greg's develop, and he edits his picks. The timing rows are another artefact of how those Shoots were exported (a keeper sits after a pause 89% of the time there, 52% on the clean card). Burst size per frame and faces in frame had too few pairs to say anything; the cheek-raise smile never fired at 640 px.

## The 1600 px check (2026-10-21, evening)

The Mac thread reran rounds 1 and 3 on the 1600 px copies of 2026-10-10 (79 burst pairs; no older-Shoots column, since those are 640 px previews). **Heads upright held: 58 to 21 (.73).** Everything about sharpness got worse, not better. Eye sharpness went to .30 (22 to 54), eye focus as stored to .39, eyes open to .35, face sharpness to .38, and frame sharpness to .39. So inside a burst Greg tends to keep the frame with *softer* eyes and more blink, which fits laughter and movement over a frozen, posed instant. These signals point the wrong way consistently, but turning them around into a rule would be fitting one card, so they stay out until another clean Shoot agrees. (The numpy divide-by-zero warnings on the Mac come from the matrix multiply in `burst_image.py`, most likely the spurious warnings numpy's matmul raises on macOS's Accelerate backend; every number in the table came out finite.)
