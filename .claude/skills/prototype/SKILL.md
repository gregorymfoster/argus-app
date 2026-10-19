---
name: prototype
description: Run, check, change or republish the Argus UI prototype in prototype/ (dummy data, Lightroom-style flags, design variants, Playwright screenshots).
---

# Argus prototype

1. `cd prototype && npm install` once.
2. Make the change, matching `docs/design.md` and `prototype/DECISIONS.md`. Use the shadcn/ui components in `src/components/ui/` (or add one in the same shape) rather than hand-rolling a widget. New open design question? Add a `Decision` in `src/store/variants.tsx` and branch on `useVariant(id)`; when Greg picks, record it in `DECISIONS.md` and delete the switch.
3. `npm run check` (or `npm run shots <name>`), then look at the PNGs in `out/shots/` before calling it done. Add a shot or a smoke check in `scripts/shots.mjs` for anything new.
4. `npm run artifact`, then publish `out/argus-prototype.html` with the Artifact tool to https://claude.ai/artifact/HLjvuatNrWyscv7pDSHKPu (pass it as `url`) so Greg's link keeps working.
5. Commit straight to main and push (Greg: no PRs in this repo).
