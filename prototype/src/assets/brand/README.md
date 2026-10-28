# Brand art

Greg's own art for Argus (2026-10-27), in the glass-camera style of the app icon (`app/icons/src/original.png`). Shrunk to WebP from his PNGs; the art is his, not photos, so it ships in the public repo too (`scripts/public-export` allows this folder).

- `hero-wide.webp` (1600×800): wide camera with light trails. README and release page hero.
- `hero-welcome.webp` (1040×650): calmer camera. Banner on the first-launch welcome (`settings/FirstRun.tsx`).
- `social-preview.jpg` (1280×640): icon left on blue. GitHub social preview, set by hand in each repo's Settings > General.
- `empty-library.webp`: empty glass tray. Empty Library, empty Shoot and empty filter states (`ui/Empty.tsx` via `ui/emptyArt.ts`).
- `cull-done.webp`: stacked photos with a check. Nothing left to cull (the grid's Unflagged filter) and All Stacks culled.
- `import-done.webp`: photo in a tray with a check. The finished Export toast (32 px).

The three state pieces are transparent, about 512 px, cropped to the art; show them at 128 px or less so they stay sharp on Retina.
