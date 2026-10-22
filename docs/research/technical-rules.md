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
