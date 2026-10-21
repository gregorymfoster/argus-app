---
name: design
description: Read before any change to Argus's UI (prototype/ screens, CSS, components, copy). The checklist that keeps every screen in the one design language of docs/design.md.
---

# Argus design checklist

Argus has one design language, written down in `docs/design.md`. It is calm like the Claude app, dense and keyboard-first like Cursor, precise like Linear, and follows modern Lightroom's flows when in doubt. Go through this list for every UI change, however small.

## Before you build

1. Read `docs/design.md` from the top. It ranks the sources (Greg's dated words, then Lightroom flows, then Claude's chrome, then Cursor and Linear) and holds every token and rule.
2. A new screen or a new layout is mocked up on the Claude Design canvas and agreed with Greg first. Small changes inside an agreed screen don't need a mockup.
3. If Greg states a new preference, write it into `docs/design.md` with the date in the same change, and update the primitive or lint that enforces it.

## While you build

4. Use the primitives, not one-off CSS:
   - `Button` from `components/ui/button`: `default` (terracotta) for the one primary action, `ghost` in headers, toolbars and bars, `outline` (soft fill, no outline) for secondary buttons in dialogs and panels.
   - `Seg` from `ui/kit`: `quiet` in headers, tab rows and bars (no tray, one chip on the active item); the default tray only for form choices in panels and Settings.
   - `Kbd` from `ui/kit`: keycaps only in menus, ⌘K, tooltips and on a dialog's own buttons; `<Kbd quiet>` everywhere else. Never a bare `<kbd>`.
   - `ProgressBar` and `eta()` from `ui/Progress` for any wait whose size is knowable. No bare spinners.
5. Colors, radii, sizes and timings come from tokens (`var(--chip)`, `bg-chip`, `text-text-2`, `--row-h`, `--fast`). A value that isn't a token yet becomes one in `styles.css` `:root` and `@theme`, and a row in the Tokens table of `docs/design.md`.
6. Quieter wins: no tray or box around a group, no outline on a button, no keycap at rest in bars, no bubbly size. If Claude's app wouldn't show it, cut it.
7. Copy is sentence case, uses the glossary's words (`GLOSSARY.md`), names the AI with `aiName()`, and has no em-dash or exclamation mark. Names Argus creates go through `engine::naming`.
8. Keep screens thin over `src/data/backend.ts`, and follow `docs/agents/performance.md` in grids (tile thumbnails, `usePoll`, no per-tile blur).

## Before you land

9. `cd prototype && npx tsc -b && npm run design-lint`. If design-lint fails, fix the new hits. If you cleaned old ones up, run `node scripts/design-lint.mjs --update` to lower the baseline. Never raise the baseline to get green; change the scale in `docs/design.md` and the script instead, if the scale is what's wrong.
10. Look at it. `node scripts/design-shots.mjs OUT '?chrome=mac#/cull'` (or `#/loupe/p3`, `#/bursts`, `#/edit/p3`, `#/import`) writes retina shots of the window, sidebar and Cull toolbar. Open the PNGs and compare with before. For a header or footer change, also run `just layout`; for Import, `just import-check`.
11. For a visible change, put a before and after in `/mnt/project-files/` and link them in your reply, so Greg can judge without opening the app.
12. Land with `just land` (design-lint runs inside `just check`).
