# Argus prototype

A clickable, dummy-data version of the Argus MVP, for refining the UI and flow before the real Tauri app exists. It is React + TypeScript on Vite, so its screens and domain types can seed `app/` later. Nothing here touches real photos: the 112 "photos" are painted on a canvas from a fixed seed, and the Agent is a keyword script.

## Run it

```sh
cd prototype
npm install
npm run dev        # http://localhost:5173
```

Start at Import, or jump straight in with `#/cull`, `#/look`, `#/loupe/p15`, `#/compare/p1,p2,p3` or `#/edit/p15` (any hash other than `#/import` skips Import and starts with AI Verdicts already written).

## What's in it

- **Import**: a dialog over the grid. Card or folder, Shoot name, destination path (no spaces), then it opens the Shoot straight away and culls when previews are ready.
- **Cull grid**: Lightroom flags (`P` `X` `U`, jump to next), hover flag buttons, right-click menu, Shift/⌘-click multi-select, filters for Picks, Rejects and Unflagged, Stacks collapsed to their best frame, Comfortable/Dense density, virtualized with TanStack Virtual.
- **Loupe**: face boxes with blink flags, signals, three flag buttons, Stack filmstrip.
- **Compare**: Lightroom's Survey for a Stack or selection (`C`), with Claude's best frame crowned and `K` to keep one and reject the rest.
- **Look**: one Look for the whole Shoot, described to the Agent, per-photo Corrections, split before/after tiles.
- **Suggestions** (in Edit): a strip of takes under the photo, like Lightroom's preset previews. `Tab` previews the next take on the photo (with what it changes in the panel), `↵` accepts it as a new Version, `⌫` dismisses it, `R` asks for new takes, and **Apply to the rest** (`⌘⇧A`) matches every other Photo to this one as Suggestions. Live, takes come from the engine's background Queue (`src/data/suggestions.ts`) and a chip in the header shows its progress. In the grid, tiles show how many takes are waiting and `A` accepts the first take on every selected Photo as one undo.
- **Deep edit**: Mask cards (face, sky, region, brush), global sliders, Sync to Look (`⇧S`), Version history, readable Recipe JSON, hold `\` for before.
- `⌘K` palette (cmdk), export dialog, Sonner toasts with Undo, provider picker.

What Greg chose and why is in [DECISIONS.md](DECISIONS.md).

## Live mode (real Photos)

The prototype can run on a real Library instead of the painted dummy Shoot. Inside the desktop app (`app/`) it always does. In a browser, build one with the engine CLI, start the bridge, and open the dev server with `?library=live`:

```sh
cargo build --profile fast -p argus-cli
../target/fast/argus import ~/Pictures/some-shoot --library ~/tmp/argus-lib
../target/fast/argus serve --library ~/tmp/argus-lib     # 127.0.0.1:7878
npm run dev    # then open http://localhost:5173/?library=live#/cull
```

Vite proxies `/api` and `/previews` to the bridge. The grid shows the newest Shoot with the engine's own preview JPEGs, and every flag change (including undo) is written back to the Library. Asking the Agent to "cull this shoot" runs the real Shoot-wide AI cull through the bridge (`argus cull` underneath) and reloads with Claude's Verdicts, reasons and Stacks. Import picks a real folder and opens the new Shoot, and the sidebar switches between Shoots. Any other request to the Agent goes to the engine's Agent loop (`src/data/liveAgent.ts`), which edits through the tool layer as one undoable batch; the Edit screen reloads the Photo's Recipe and its History from the Library. The Look screen and Masks still run on prototype data. `src/data/live.ts` is the adapter and `src/data/backend.ts` the transport (Tauri `invoke` in the app, `/api/<command>` on the bridge). `npm run live <library-dir>` is the self-test. It copies the Library to a temp folder, starts the bridge and Vite, checks the grid loads engine previews, presses a flag key and confirms the Verdict reached the Library, runs a cull from the Agent panel against a mock Claude endpoint, asks the Agent for an edit in Edit (the mock answers with an `edit_recipe` call) and undoes it from the chat, presses `R` for Suggestions (the bridge's Queue asks the mock for takes) and accepts one with its crop, and writes `out/shots/live-*.png`.

## Variants

When there's an open design question, add a `Decision` in `src/store/variants.tsx` and read it with `useVariant(id)`. The amber Variants panel (`⇧V`) then appears with the options, a notes field and "Copy all feedback". It hides while the list is empty, as it is now.

## Self-test loop

```sh
npm run check      # typecheck + screenshots + smoke tests
npm run shots look # only shots whose name contains "look"
npm run dead-code  # knip: unused files, exports and dependencies (config in knip.json)
npm run preset-parity  # src/domain/presets.ts against the engine's apply_preset, every built-in Preset
```

`scripts/shots.mjs` starts Vite, drives Chromium with Playwright, writes PNGs of every screen to `out/shots/`, and runs smoke checks through the real UI (flag keys and jump-to-next, click to clear, multi-select and batch undo, Compare keep-best, Look slider changes the render, Sync to Look, Agent creates Masks, no spaces in the import path). It exits non-zero on any page error or React warning.

## Share it

`npm run artifact` builds a single self-contained page at `out/argus-prototype.html` for publishing as a Claude artifact. The current one is https://claude.ai/artifact/HLjvuatNrWyscv7pDSHKPu.

## Layout

- `src/domain/`: glossary types, Looks, and `render.ts`, which turns a Recipe into CSS layers (a stand-in for the WGSL pipeline)
- `src/data/`: the seeded dummy Shoot and the canvas painter
- `src/store/`: the Library reducer (verbs mirror the tool layer: set Verdict, apply Look, sync Look, write Recipe, undo) and the Variants registry
- `src/agent/`: the scripted Agent and its panel
- `src/components/ui/`: shadcn/ui components; `src/ui/`: Argus compositions (flags, kit, overlays)
- `src/screens/`: one file per screen, kept thin so it's cheap to throw away
