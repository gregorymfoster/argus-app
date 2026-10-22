# A Technical check without a model (2026-10-21)

Greg asked whether a custom cull could be faster and lighter than Qwen, the way Taste already is. After the split cull, Qwen's only job is the Technical check: is a frame plainly broken. That takes it about 40 minutes for a 261-frame card on the Mac. Most of what it catches is blur, and blur is something the stored sharpness Signal already measures.

## The rules

`engine::technical` (`--provider rules`, or `"provider": "rules"` on the `cull_shoot` tool) rejects a frame when either holds:

- it is under 0.12 of the Shoot's median sharpness (missed focus, heavy blur), or
- it is under 0.2 of the sharpness of a look-alike shot seconds from it and under 0.4 of the Shoot's median (shake or missed focus on a moment the camera also caught sharp).

A look-alike is the same scene (embedding similarity 0.85) within 30 s, or anything roughly alike (0.6) within 2 s, because blur and shake move a frame's embedding off its sharp sibling. Everything else is left unflagged, exposure included: a dark RAW can be rescued, and Taste already ranks a much darker copy of a moment below it. Taste now does the same for a much softer copy (under 0.4 of a look-alike), so a shaken frame the rules keep still ranks below its sharp sibling.

The sharpness Signal depends on preview size, so the rules only compare frames within one Library and one Shoot.

## Numbers

Through the real cull path (`bench/cull_degrade.py --provider rules`, `bench/score.py --only cull --provider rules`), against Qwen 3.8 27B's split cull at fdd211a:

| | rules | Qwen |
|---|---|---|
| damaged twins rejected | 17 of 30 | 21 of 30 |
| blur / shake / dark / crop | 10 / 4 / 0 / 3 | 9 / 10 / 0 / 2 |
| original ranked above its twin | 29 of 30 | 29 of 30 |
| Greg's keepers rejected on 2026-10-10 | 0 | 0 |
| frames rejected on 2026-10-10 | 1 | 6 |
| star AUC on 2026-10-10 (same Taste) | .65 (640 px) | .62 (1600 px) |
| time for the 2026-10-10 card | 13 s on a 4-core cloud CPU, Facts included | 40 min on the M4 Max |

The offline replica (`bench/technical_rules.py`) found the trade-off: loosening the look-alike threshold to 0.25 rejects 21 of 30 twins, as many as Qwen, but also one of Greg's picks on 2025-03-22. These thresholds were chosen on the same 30 twins they are scored on, so the rules' numbers are a little optimistic.

What Qwen catches that rules can't: shut eyes that aren't a laugh (two of its six real rejects on 2026-10-10), subtle shake that leaves the frame's sharpest tiles intact, and odd framing. What rules give: the same zero wrong rejects, the same order of originals over their twins, and a cull that finishes seconds after the Facts are in.

## Status

Rules are the default Technical check (Greg, 2026-10-21). Import queues a `check_shoot` job that runs the rules cull right away, since it needs no model. Qwen is now an opt-in second opinion: `argus cull --check model`, `ARGUS_CULL_CHECK=model`, or `"check":"model"` on the `cull_shoot` tool. It keeps the rules' rejects and adds its own (shut eyes, odd framing), so it can only reject more. A harness variant with `cull_taste: "model"` still asks the model for everything. Bench runs that mean to score Qwen must pass `--check model` or set the variable, or they quietly score the rules.

## Shake, shut eyes and doubtful frames (2026-10-22)

Greg asked for small dedicated checks for the two things the rules missed, shake and shut eyes, and for Qwen to see only the frames the rules aren't sure of (`bench/shake_eyes.py`).

Shake smears a frame along the direction the camera moved, so the edges that survive all run the other way. The Signals Fact now stores `streak`, the coherence of the structure tensor summed over the frame's sharpest tiles: 0 when edges run every which way, 1 when they're all parallel. Greg's keepers sit at a median of 0.17 (the streakiest, 0.74, is a sharp frame of stripes). Shaken copies sit at 0.73 to 0.96, whether shaken sideways, at 45 degrees or upright. A frame is rejected as "camera shake" when its streak is over 0.6 and it is under 0.6 of the Shoot's median sharpness. Through the real cull on all five Shoots the rules now reject 24 of 30 damaged twins (blur 10 of 10, shake 10 of 10, bad crop 4 of 5, dark 0 of 5), up from 17 and above Qwen's 21, still with none of Greg's picks rejected and one real reject on 2026-10-10.

Shut eyes don't make a safe reject. Of the frames where a main face has its eyes shut without laughing, Greg kept 4 of 16 across the five Shoots, and requiring an eyes-open look-alike nearby still caught two of his picks. So shut eyes only make a frame doubtful.

A doubtful frame is one the rules keep but a second look could help: under 0.35 of the Shoot's median sharpness, under half as sharp as a look-alike (and under 0.7 of the median), streaky (over 0.45, under 0.8 of the median), or eyes shut without a laugh (`technical::doubtful`). That is 46 of 661 frames on the bench Shoots, about one in fifteen. Qwen's second opinion (`--check model`) now sees only those, as a sheet of their own (`cull::Doubtful`; `ARGUS_CULL_ASK=all` asks about every frame as before), so it takes minutes, not most of an hour. Replaying last night's Qwen run, 8 of its 16 wrong rejects and 3 of its 5 right ones fall on doubtful frames, so routing halves the keepers it throws away but it still throws away more than it catches. Rules alone stay the default.

These thresholds were again chosen on the bench Shoots they're scored on, and synthetic shake (a box blur) is cleaner than the real thing, so expect real shake to slip through more often.

## Greg's backed-up rejects at scale (2026-10-22)

Greg's external drive holds Lightroom exports (2048 px) of frames he rejected or left unflagged before deleting them, and iCloud holds his favorites. `bench/drive_rules.py` grouped them by capture day, kept the 35 days with at least 3 favorites and some rejects (3,595 frames), imported each day as a Shoot and ran the default rules cull on the Mac (31 minutes at background priority).

The rules rejected 37 of his 2,777 rejects, 0 of 315 unflagged frames and 1 of 503 favorites. So the rules catch only the plainly broken frames, as designed; most of what Greg throws away is taste. The one favorite rejected was a shallow-focus portrait (a face 26% of the frame high, the background melted), rejected as "out of focus" because the whole frame was far under the Shoot's median sharpness. Frame-wide sharpness can't tell a soft portrait from a missed focus, so a frame with a main face at least 15% of the frame high (`PORTRAIT_FACE`) is no longer rejected on Shoot sharpness alone: it is only doubtful, unless a look-alike shows it is soft beside its twin. On the bench twins this keeps every blur reject (the look-alike rule catches them) and loses one bad crop.

These are edited exports, not RAWs, and the favorites are edited while the rest mostly aren't, so the numbers are a check on wrong rejects more than a recall figure.
