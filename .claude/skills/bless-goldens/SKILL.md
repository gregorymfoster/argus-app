---
name: bless-goldens
description: Re-accept the golden reference images after a deliberate pipeline, colour or tone change makes `golden` tests fail. Use when engine/tests/golden.rs fails and the new look is intended; do not use to silence a regression.
---

# Bless goldens

Goldens are `fixtures/golden/<stem>.png`: each `fixtures/raw/*.ARW` rendered at 1024 px long edge with default options. The test (`engine/tests/golden.rs`) fails if any channel's MAE exceeds 1.0 (0..255 scale).

## 1. Decide: intended change or regression?

A failure prints MAE per channel and writes, for each failing Photo:

- `out/golden/<stem>_actual.png` the new render
- `out/golden/<stem>_diff.png` |actual - reference| x8 (bright = changed)

Look at them with the Read tool before doing anything else: `_actual.png`, `_diff.png`, and the reference `fixtures/golden/<stem>.png`.

- **Bless** only if you changed the look on purpose (exposure, curve, white balance, colour matrix, demosaic quality) and the diff matches what you meant to change: broad and smooth for a tone/colour change, edges only for a demosaic/sharpening change, and every Photo moved the way you expect.
- **Regression** if the diff shows something you did not intend: localised blotches, colour casts, banding, shifted or rotated image, one Photo changing alone, or a "refactor" that moved MAE at all. Fix the code, do not bless. Dimension mismatches are never a reason to bless unless the output size changed on purpose.

## 2. Bless

```
just bless
```

Without `just`: `ARGUS_BLESS=1 cargo test --profile fast --test engine golden::`. Then `git status --short fixtures/golden` shows which references changed (each PNG should stay under ~1.5 MB).

## 3. Verify and commit

1. View every changed `fixtures/golden/*.png` with the Read tool. They must be sane photos (right orientation, colour, exposure, no artefacts).
2. Run the plain test again (`cargo test --profile fast --test engine golden::`); it must pass.
3. Commit the PNGs together with the code change, and say why the look changed in the message (e.g. "Raise default exposure to 1.0 EV; goldens re-blessed, midtones ~+3 levels").
