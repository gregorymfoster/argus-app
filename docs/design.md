# Design language

This file is the single source of truth for how Argus looks and feels. Code follows it: the tokens in `prototype/src/styles.css`, the primitives in `prototype/src/components/ui/` and `prototype/src/ui/kit.tsx`, and the checks in `prototype/scripts/design-lint.mjs`. If they disagree with this file, fix whichever one is wrong in the same change. Every UI change runs the checklist in the `design` skill (`.claude/skills/design/SKILL.md`).

Mockups live on the Claude Design canvas (https://claude.ai/artifact/DL71SAHY8XDUA6FgK2XxcY). A new screen is agreed there before it's built.

## Where the rules come from

Greg wants the calm of the Claude app, the density and keyboard-first feel of Cursor, and the precision of Linear. We borrow their qualities, never their branding or exact layouts. Claude gives us quiet chrome, soft near-black surfaces, a calm composer and one warm accent. Cursor gives us density, context chips on the composer, compact agent steps with timings, and a shortcut for everything. Linear gives us hairlines, small radii, sentence-case labels and a ⌘K palette that reaches every command.

When rules collide, they rank in this order:

1. Greg's explicit words, dated in this file. They win until he changes his mind.
2. Modern Lightroom for flows and structure (Greg, 2026-10-18). It's the tool he and most photographers know. Simplify where there's a reason, deviate only with a strong opinion, and write the opinion down here.
3. Claude's app for chrome: sidebar, headers, menus, composer, color.
4. Cursor and Linear for density, keyboard and precision.

## Character

Argus is a prosumer power tool, never a friendly, bubbly app with big text (Greg, 2026-10-18). He pushed further the same day: more information on fewer screens, less navigating, no big bubbly buttons. So each screen carries what you would otherwise go elsewhere to read. The Cull footer shows the file, EXIF and reason for the Photo under the cursor. The Loupe keeps a filmstrip of the whole Shoot and an Info panel, so moving on never means going back to the grid.

The photo is the hero. Chrome is quiet so the photo reads loudest, and nothing decorative competes with it: no gradients, no illustration, no color without a meaning.

Quieter wins (Greg, 2026-10-20, "keep modernizing and simplifying"). When a side-by-side with Claude shows a box, a tray, a border or a keycap that Claude doesn't have, take it out.

## Color

Argus wears Claude's dark palette (Greg, 2026-10-18): near-neutral blacks with the faintest warmth, off-white text, warm grey secondary text and Claude's terracotta as the one accent. The ground right around a photo (Loupe and Edit stage, grid cells, filmstrip) is a strictly neutral grey, as in Lightroom, so nothing tints color judgment.

Terracotta marks action and focus only: the primary button, progress bars, keyboard focus, text selection, checked boxes and the chosen import source. Every other color carries one meaning and is used for nothing else. Blue (Claude's link blue) is Pick. Red is Reject. Unflagged has no color. Green is for finished states only. Amber marks the current Shoot and Best Photos.

Use a token, never a literal. Colors live once in the `:root` block of `styles.css` and reach Tailwind through `@theme` (`bg-chip`, `text-text-2`, `border-border`). A new color needs a new token here first. `design-lint` counts literals outside those blocks and fails when a file gains one.

## Type

UI text and Agent prose use the Mac's own SF Pro (Greg, 2026-10-18), with bundled Geist standing in wherever SF isn't installed, such as the Linux test runs. Numbers and metadata (file names, EXIF, counts in tables, timings, slider values) use Nordwand Mono (Greg, 2026-10-20), bundled in `prototype/src/fonts/nordwand-mono/` under the SIL OFL. Fonts are bundled, never fetched. Numbers use tabular figures.

The scale is tight. Body UI text is 13px, secondary text 12 to 12.5px, captions and counts 11 to 11.5px, the smallest labels 10 to 10.5px. Headings are 13 to 15px at weight 500 with slightly tight tracking. Agent prose is 13.5px at a relaxed 1.6 line height. Nothing in the chrome goes above 15px. The allowed sizes are 10, 10.5, 11, 11.5, 12, 12.5, 13, 13.5 and 15px, and `design-lint` holds the line.

## Shape, space and borders

Radii are small. Use 4 to 7px for chips, tiles, buttons, menu items and rows, 8px for panels and menus, 10px for the composer, dialogs and the ⌘K palette, and 999px for pills and dots. Spacing sits on a 4px grid. Panel headers are 40px. Sidebar and list rows are `--row-h` (28px), menu and palette rows 28 to 32px. Header controls are 26 to 27px tall. Icons are `--icon` (14px) in rows and 13px in headers, from lucide.

The window is one flat surface split by hairlines (sidebar | content | Agent), never floating cards with gaps. Hairlines are translucent white so one border reads the same on any surface and over photos.

Fewer borders (Greg, 2026-10-18 and 2026-10-20). Buttons, chips, kbd hints and segmented controls get no outline. Borders stay only where they carry meaning: inputs and the search field, popovers, tiles, the composer, and the panel splits. Don't wrap a group of controls in a box or tray to show they belong together; spacing does that.

## Controls

Use the shared primitives and their variants. They make the right look the default, so a screen should rarely need its own CSS for a control.

Buttons (`components/ui/button.tsx`). There is one primary action per view at most, the solid terracotta `default` variant (Import 112 photos, Export, Accept). Actions in a header, toolbar or bar are `ghost`: no fill at rest, a `--hover` wash under the pointer. Secondary buttons in dialogs and panels are the soft `outline` variant, a `--fill` tint with no actual outline. `pick` and `reject` are only for flag actions.

Groups (`Seg` in `kit.tsx`, over `components/ui/toggle-group.tsx`). A group in a header, tab row or bar is `quiet`: no tray, and only the active item gets the `--chip` fill (white 7.5%). That is the "one chip per group" rule from Design v3. A form choice inside a panel or Settings keeps the default `tray` look, a soft tray with the chosen item raised, because there it reads as a field. Icon groups like the view switch (`.view-switch`) and the Loupe's flag row (`.flag-seg`) follow the quiet look too.

Key hints (`Kbd` in `kit.tsx`). Every common action has a shortcut and shows it somewhere. Keycaps (a small soft cap) belong in menus, the ⌘K palette, tooltips and on a dialog's own buttons (Import ↵, Cancel esc). Everywhere else, in bars, headers, footers and hint lines, use `<Kbd quiet>`: a plain letter with no cap. In the sidebar and on the Loupe's flag row the hint fades in only on hover. Don't write a bare `<kbd>` in a screen; `design-lint` counts them.

Sidebar. Search is a hairline-outlined field, not a filled pill. Import is the first row under it, where modern Lightroom keeps its + and Claude keeps New. Rows are plain text with a muted icon, and only the selected row gets the chip. Counts stay (Lightroom shows them, and Greg wants the information) in the sans at `--text-3`. The open Shoot is marked by the chip and an amber folder. The foot holds background work and the Resource meter as flat rows above one hairline, like Claude's account row.

Standard Mac behavior always works. ⌘, opens Settings, ⌘W and ⌘Q close and quit, ⌘+ ⌘− ⌘0 zoom the app, and Esc closes any dialog. A dialog also has ✕ and Cancel, but a stray click outside never closes it.

## Feedback and motion

Never a bare spinner when progress is knowable (Greg, 2026-10-18). Card scans, imports, previews, culls, Agent edits, renders and exports show `ProgressBar` from `ui/Progress.tsx`: a thin terracotta bar with a count and an ETA (`eta()` formats it). A spinner is only acceptable for a wait that has no size and lasts under a couple of seconds. `design-lint` counts spinners.

Every AI action shows as a compact step with its timing, and offers a review or an undo of exactly what it changed. AI work for the open Photo jumps ahead of background batches, and the UI names the AI by `aiName()`, never a hardcoded model.

Motion is quick, calm and small (Greg wants "small delightful animations"). Colors and borders ease over `--fast` (120ms). Popovers rise 4px and fade in over 160ms on `--ease-out` and leave faster than they came. Buttons press down 1px. Thumbnails fade in as they load and imported Photos pop into the grid. Nothing animates over the photo while you judge it, and `prefers-reduced-motion` turns it all off.

## Writing in the UI

Labels and buttons are sentence case (Import a Shoot, not Import A Shoot). Use the glossary's words exactly (Library, Photo, Shoot, Pick, Reject, Unflagged, Stack, Look, Mask, Suggestion). Say what a thing does in a few plain words, keep counts and units visible (112 photos, 3.8s, about 11 min), and skip exclamation marks and em-dashes. File, folder and Shoot names shown or created follow `docs/naming.md`.

## Design v3: quieter chrome (Greg, 2026-10-20)

Greg put the Argus sidebar and grid toolbar next to the Claude app's and asked to keep modernizing and simplifying. The gap was boxes. Argus wrapped every group in a tinted tray, gave Import and the background-work card their own bordered cards, and showed keycaps at rest. Claude's chrome has almost none of that. The rules above (one chip per group, ghost header buttons, quiet key hints, the outlined search field, Import as the first row, the flat sidebar foot) came out of that pass and now apply everywhere. In Edit, Auto and B&W are ghost buttons, while one-click fixes and Agent prompts keep their soft fill, since an outline on each would add borders. In Import, the From and New/All pickers are borderless and Cancel is ghost beside the one terracotta Import.

## Agreed patterns

Greg chose these in the prototype on 2026-10-17 and later. They hold until he uses the real app and changes his mind, which we expect, so build them to be easy to swap.

The Agent lives in a side panel on the right, always visible. Import is a focused dialog, Lightroom-style: it finds the memory card by itself (or you choose a folder) and shows every photo as a grid. Photos the Library already has are unticked ("New only"). A RAW+JPEG frame is one Photo, and "RAW only" (on by default) leaves the camera's JPEG behind. The local model suggests the Shoot name, which always carries the shoot's date and has no spaces (`2026-10-18_Boat-Trip`). Pressing Import opens the Shoot. The AI cull is already applied when the grid opens, so Greg scans and flips what he disagrees with rather than being walked through each Photo.

Flags follow Lightroom. Every tile shows a small flag chip in its corner (Pick in blue, Reject in red, nothing when Unflagged) and Rejects fade back. Hovering a tile reveals Pick and Reject buttons on that chip, and clicking one sets it (clicking the active one clears it). P, X and U do the same from the keyboard, and flagging jumps to the next Photo. Shift-click and ⌘-click select several Photos so one keypress flags them all. The AI's reason sits as a caption under each tile, after its stars. The AI's stars are dim with a small sparkle; the person's are solid. Keys 0 to 5 rate and move on like flags. Best Photos is a grid filter with a gold award badge carrying the rank.

Grid thumbnails change size with a trackpad pinch, the size slider in the toolbar, or = and − as in Lightroom's Grid; the Photo under the pointer stays put while it zooms. ⌘+, ⌘− and ⌘0 zoom the whole app (View menu and ⌘K), like Cursor and browsers, for text density. Right-click works on Shoots (Rename, Suggest a name, Show in Finder, Export) and on Photos; double-click or Enter renames a Shoot in place and Esc cancels. The grid's Sort menu follows Lightroom's (Capture time, Stars, Flag, File name, each with a direction; ⇧S flips between Stars, most first, and Capture time); a Stack sorts by its best frame and stays together, and the Loupe and filmstrip follow the same order. The grid has "Auto all" (⇧A, or on a selection) so a whole Shoot can get Auto before culling.

For Looks, chat comes first: Greg describes what he wants and the Agent builds it. Before and after shows as split tiles. AI edits apply directly, with one undo for the whole batch. In deep edit, each Mask is a card with its own sliders.

The Edit panel copies Lightroom's (Greg, 2026-10-18: "just copy Lightroom for starters"). A thin tool strip down the right edge opens Edit (E), Crop (C), Masking (M), Presets (⇧P) and History. Edit has Auto (⇧A) and B&W (V) on top, a Profile, then a Recommended row of Looks for this Photo with an Amount (0 to 200) and one-click fixes that toggle (Enhance sky, Brighten faces, Even skin, Lift shadows). Below come collapsible Light and Color sections in Lightroom's order and names, each with an eye that previews the photo without it, then Color mixer and Retouch, closed until opened. Only sliders the engine renders appear. Crop has aspect ratios, Straighten and a crop box dragged on the uncropped photo. Masking shows Create new mask as a grid (Subject, Sky, Background, People, Brush, Linear, Radial, Hair), the Photo's Masks as rows, and for a People Mask Lightroom's Select People. Linear and Radial Masks are placed with handles on the photo. Hovering any preset, Recommended Look or fix previews it on the photo through the engine. The Agent sits under the open tool.

## Components

Use existing libraries and don't hand-roll what they already do: shadcn/ui on Radix primitives for controls and dialogs, Tailwind with the tokens below as theme variables, cmdk for the ⌘K palette, TanStack Virtual for the photo grid, Sonner for undo toasts, lucide for icons. Keep screens thin: they read state and call tool-layer verbs, so a layout can be thrown away without touching anything underneath. Grids use `size="tile"` thumbnails and no blur or filter per tile (`docs/agents/performance.md`).

## Tokens

| Token | Value | Use |
|---|---|---|
| bg | #111111 | chrome and sidebar (translucent in the Mac app) |
| panel | #161615 | content: grid, Agent |
| canvas | #151515 | neutral ground behind the photo in Loupe and Edit |
| photo-well | #222222 | neutral placeholder under grid and filmstrip thumbnails |
| raised | #1F1F1E | cards, composer |
| raised-2 | #262625 | chat bubbles, raised rows |
| glass | #212120 at 82% + blur | menus, ⌘K palette, tooltips |
| border | white 6.5% | panel and card hairlines |
| border-strong | white 10% | inputs, popovers, the search field |
| border-hover | white 16% | hovered tiles, inputs, cards |
| hover / press / selected | white 4% / 7% / 6% | interaction washes over any surface |
| chip | white 7.5% | the one active item in a quiet group, the selected sidebar row |
| fill / fill-hover | white 5% / 8.5% | secondary buttons, keycaps, tray groups (no outline) |
| accent | #D97757 | Claude terracotta: progress, focus, selection |
| accent-strong / on-accent | #C6613F / white | primary button and checked box fill and its text |
| ring | #D97757 at 70% | keyboard focus |
| text | #F0EFEC | primary text |
| text-2 | #B4B2AB | secondary text |
| text-3 | #898782 | captions, counts, quiet key hints |
| pick | #7AA6E7 | Pick |
| reject | #F0716B | Reject |
| done | #7FD1A8 | completed steps |
| shoot | #E0AC5A | current Shoot marker, Best Photos |
| row-h | 28px | sidebar and list rows |
| icon | 14px | row icons (13px in headers) |
| fast / ease-out | 120ms / cubic-bezier(0.16, 1, 0.3, 1) | transitions |
| pop | layered shadow | every popover, menu, dialog, toast and tooltip |

In the Mac app the title bar is an overlay (traffic lights sit over the sidebar, headers drag the window) and the sidebar shows the system's translucent sidebar material; content stays opaque. `?chrome=mac` previews that layout in a browser. Raised surfaces get a 1px inner top highlight.

## Checking a UI change

`npm run design-lint` (also part of `just check`) fails when a file gains a color literal outside the token blocks, a radius or font size off the scales above, a bare `<kbd>`, a spinner or an em-dash in UI text. It is a ratchet over `prototype/scripts/design-lint-baseline.json`: older code keeps its count until someone cleans it, and `--update` lowers the baseline after a cleanup. `just layout` checks headers, footers and the sidebar foot (also squeezed to 150 px) for wrapped or clipped labels. Labels in a bar or the sidebar never wrap: they ellipsize, and secondary bits like a version number drop out first (Greg, 2026-10-20, after Restart to update wrapped). `node prototype/scripts/design-shots.mjs OUTDIR '?chrome=mac#/cull'` takes retina shots of the window, sidebar and Cull toolbar (any route works, e.g. `#/loupe/p3`, `#/bursts`) to look at before and after.
