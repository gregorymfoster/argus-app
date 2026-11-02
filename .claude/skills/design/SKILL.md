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
8. Applying anything to many Photos uses the Apply look pattern (Greg, 2026-11-02: "this should be my average experience when applying edits to everything"): a popover from the button that starts it, every affected Photo drawn as it will look, a tick per Photo to leave it out, Return applies as one Undo, Esc changes nothing. Build it on `ApplyBody` in `ui/ApplyPopover.tsx` (as `ui/ApplyLook.tsx`, `ui/Framing.tsx` and `ui/ApplyEdit.tsx` do), with the engine drawing the tiles; never a background list of takes to accept elsewhere. See "Applying to many Photos" in `docs/design.md`.
9. Keep screens thin over `src/data/backend.ts`, and follow `docs/agents/performance.md` in grids (tile thumbnails, `usePoll`, no per-tile blur).

## Before you land

9. `cd prototype && npx tsc -b && npm run design-lint`. If design-lint fails, fix the new hits. If you cleaned old ones up, run `node scripts/design-lint.mjs --update` to lower the baseline. Never raise the baseline to get green; change the scale in `docs/design.md` and the script instead, if the scale is what's wrong.
10. Try it with the awkward cases (2026-11-02, after 0.1.17's overlapping Cull tiles and the cut-off "Reading eac..." label). Run `just layout`: every screen at 1024, 1280 and 1500 px, plus the `?stress=1` demo (every third Photo a portrait, a 40-frame Stack that pages, a long Shoot name) with Show next too, a paged Stack and the Apply look popover open. It fails on overlapping tiles, text cut off or squeezed in a bar, popover or dialog, and popovers running off the window (`layoutProblems` in `scripts/layout-lib.mjs`, also run on every shot in `shots.mjs`). If your screen or state isn't in its `STRESS` list, add it. A label that may run long gets room, a shorter wording, or a `title`; never a silent cut. An overlap you mean gets `data-layout-ok="why"`.
11. Waits are never silent: a progress line shows the time so far, and a wait with no end turns into an error with a reason (`docs/agents/concurrency.md`). Engine calls get this from `call()`; anything that waits on several calls or a poll does it itself.
12. Look at it. `node scripts/design-shots.mjs OUT '?chrome=mac#/cull'` (or `#/loupe/p3`, `#/bursts`, `#/edit/p3`, `#/import`) writes retina shots of the window, sidebar and Cull toolbar. Open the PNGs and compare with before. For a header or footer change, also run `just layout`; for Import, `just import-check`.
13. For a visible change, put a before and after in `/mnt/project-files/` and link them in your reply, so Greg can judge without opening the app.
14. Land with `just land` (design-lint runs inside `just check`).
