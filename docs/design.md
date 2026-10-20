# Design direction

The look Argus's UI follows. Mockups live on the Claude Design canvas (https://claude.ai/artifact/DL71SAHY8XDUA6FgK2XxcY), and every screen is agreed there before it's built.

## Inspiration

Greg wants the calm of the Claude app, the density and keyboard-first feel of Cursor, and the precision of Linear. We take their qualities, never their branding or exact layouts.

From Claude, conversation is quiet and the composer is a calm box with its model picker underneath. From Cursor, the composer carries context chips and a model picker, agent tool steps are shown compactly with timings, and everything has a shortcut. From Linear, hairline borders and small radii, labels are in sentence case, and there's a ⌘K palette for everything.

## Power tool, not friendly app

Greg (2026-10-18): Argus should feel like a prosumer power tool in the spirit of Cursor, never friendly and bubbly with big text. That is the standing direction for every screen. UI text sits at 12 to 13px with a tight scale, radii stay small, padding is lean so more Photos and metadata fit on screen, and borders are crisp hairlines. Metadata (file names, EXIF, counts, timings, slider values) is set in monospace where it helps scanning. Everything is keyboard-first: every action has a shortcut, the ⌘K palette reaches every command, and shortcuts show in tooltips, menus and the palette.

Greg pushed this further the same day: more information on fewer screens, less navigating, no big bubbly buttons. So each screen carries what you'd otherwise go elsewhere to read. The Cull grid's footer shows the file, EXIF and reason for the Photo under the cursor. The loupe keeps a filmstrip of the whole Shoot and a Metadata panel, so moving on never means going back to the grid. Grid marks are icon-only, and choices like Pick, Unflagged and Reject are one 26px segmented row, not big tiles.

When in doubt about a flow, follow modern Lightroom (Greg, 2026-10-18). It's the tool he and most photographers already know, so we simplify where we have a reason to but keep its structure and interactions unless we hold a strong opinion. Lightroom's flows, Cursor's crispness.

## Rules

The photo is the hero. Argus wears Claude's dark palette (Greg, 2026-10-18): near-neutral blacks with the faintest warmth, off-white text, warm grey secondary text and Claude's terracotta as the one accent. The ground right around a photo (Loupe and Edit stage, grid cells, filmstrip) stays a strictly neutral grey, as in Lightroom, so nothing tints color judgment. No gradients or decorative color.

Terracotta marks action and focus: the primary button, progress bars, keyboard focus, text selection, checked boxes and chosen import sources. Every other color is reserved for meaning: blue for Pick (Claude's link blue), red for Reject, no color for Unflagged, green only for "done" states, and a single amber marker for the current Shoot. Reject moved from orange to red with the palette change so it never reads as the accent.

UI text and Agent prose use the Mac's own SF Pro (Greg, 2026-10-18). Numbers and metadata use Nordwand Mono (Greg, 2026-10-20), bundled in `prototype/src/fonts/nordwand-mono/` under the SIL OFL; it replaced SF Mono. Bundled Geist stands in for SF Pro wherever SF isn't available, such as the Linux test runs.

Every AI action shows as a compact step with its timing, and offers a review or undo of exactly what it changed.

Every common action has a visible keyboard hint, and the standard Mac shortcuts work: ⌘, opens Settings, ⌘W and ⌘Q close and quit, and Esc closes any dialog. A dialog also has ✕ and Cancel, but a stray click outside it never closes it.

When in doubt, follow modern Lightroom's flows (Greg, 2026-10-18). It's the tool Greg and most photographers know, so Import, Settings and the Library and Develop structure stay close to its patterns. We simplify where we have a reason to, and only deviate when we hold a strong opinion.

Waiting is never a bare spinner (Greg, 2026-10-18). When progress is knowable (card scan, import, previews, cull, Agent edits, renders), show a progress bar with a count and an ETA. Small, delightful animation is welcome, as long as it stays calm and never covers the photo.

## Star Ratings (proposed 2026-10-18, awaiting Greg)

Lightroom stars under each tile, before the reason. The AI's are dim with a small sparkle ("suggested"); the person's are solid. Keys 0-5 rate and move on like flags; pressing the AI's own number confirms it. Best Photos is a grid filter (B): #1 large, the rest in rank order with reasons, and a gold award badge with the rank on grid tiles. A ★ menu in the grid header filters by least stars. Mock: the "AI star ratings" boards on the design canvas.

## Design v3: quieter chrome (Greg, 2026-10-20)

Greg put Argus's sidebar and grid toolbar next to the Claude app's and asked to keep modernizing and simplifying. The gap was boxes. Argus wrapped every group in a tinted tray, gave Import and the background-work card their own bordered cards, and showed keycaps at rest. Claude's chrome has almost none of that: rows are plain text with a muted icon, only the selected item gets a soft chip, and actions in a header are bare icons and labels.

So the rule now is one chip per group. A segmented group (the view switch, the flag filter) has no tray, and only its active item gets the `--chip` fill (white 7.5%). Header buttons are ghost: no fill at rest, a `--hover` wash under the pointer. Keycaps don't show at rest in the sidebar or headers; a row's shortcut fades in as a quiet hint when you hover it, and tooltips and ⌘K still teach every key. Counts stay (Lightroom shows them, and Greg wants information on the screen) but they're set in the sans at `--text-3`, not mono.

Sidebar: search is a hairline-outlined field, not a filled pill. Import is the first row under it, where modern Lightroom keeps its + and Claude keeps New. Rows are `--row-h` (28px) with 7px radii, labels in `--text` and 14px (`--icon`) icons in `--text-2`. The open Shoot is marked by the chip and an amber folder (`--shoot`), no terracotta bar. The foot holds background work and the Resource meter as flat rows above one hairline, like Claude's account row.

Header controls are 26 to 27px tall at 12.5px. The same rule covers the bars around a photo: the Loupe flag row has no tray (its P, X, U fade in on hover), the Info and Agent tabs chip only the open one, and footer and header key hints are plain letters with no keycap. In Edit, Auto and B&W are ghost buttons; one-click fixes and Agent prompts keep their soft fill, since an outline on each would add borders. In Import, the From and New/All pickers are borderless and Cancel is ghost beside the one terracotta Import. The rules live in one "Design v3" block at the end of `prototype/src/styles.css`, scoped to `.sidebar` and `.panel-head` so later screens pick them up by using those classes. `node prototype/scripts/design-shots.mjs OUTDIR` takes retina shots of the window, sidebar and Cull toolbar for before/after comparisons.

| Token | Value | Use |
|---|---|---|
| chip | white 7.5% | the one active item in a group, the selected sidebar row |
| row-h | 28px | sidebar rows |
| icon | 14px | sidebar icons (13px in headers) |

## Agreed patterns

Greg chose these in the prototype on 2026-10-17. They hold until he uses the real app and changes his mind, which we expect, so build them to be easy to swap.

The Agent lives in a side panel on the right, always visible. Import is a focused dialog, Lightroom-style: it finds the memory card by itself (or you choose a folder) and shows every photo as a grid. Photos the Library already has are unticked ("New only"). A RAW+JPEG frame is one Photo, and "RAW only" (on by default) leaves the camera's JPEG behind. The local model suggests the Shoot name, which always carries the shoot's date and has no spaces (`2026-10-18_Boat-Trip`). Pressing Import opens the Shoot. The AI cull is already applied when the grid opens, so Greg scans and flips what he disagrees with rather than being walked through each Photo.

Flags follow Lightroom. Every tile shows a small flag chip in its corner (Pick in blue, Reject in red, nothing when Unflagged) and Rejects fade back. Hovering a tile reveals Pick and Reject buttons on that chip, and clicking one sets it (clicking the active one clears it). P, X and U do the same from the keyboard, and flagging jumps to the next Photo. Shift-click and ⌘-click select several Photos so one keypress flags them all. The AI's reason sits as a caption under each tile.

Grid thumbnails change size with a trackpad pinch, the size slider in the toolbar, or = and − as in Lightroom's Grid; the Photo under the pointer stays put while it zooms. ⌘+, ⌘− and ⌘0 zoom the whole app (View menu and ⌘K), like Cursor and browsers, for text density. Right-click works on Shoots (Rename, Suggest a name, Show in Finder, Export) and on Photos; double-click or Enter renames a Shoot in place and Esc cancels. The loupe keeps a side panel holding the reason, the signals and the three flag buttons.

For Looks, chat comes first: Greg describes what he wants and the Agent builds it. Before and after shows as split tiles. AI edits apply directly, with one undo for the whole batch. In deep edit, each Mask is a card with its own sliders.

The Edit panel copies Lightroom's (Greg, 2026-10-18: "just copy Lightroom for starters"), laid out as design pass v2 on the canvas (Greg, 2026-10-18: "much better, implement it"). A thin tool strip down the right edge opens Edit (E), Crop (C), Masking (M), Presets (⇧P) and History. Edit has Auto (⇧A) and B&W (V) on top, a Profile (Argus Color or Monochrome, which is what B&W switches, as Lightroom's), then a Recommended row of four Looks for this Photo with an Amount (0 to 200, scaling the Look from what it replaced) and one-click fixes that toggle (Enhance sky, Brighten faces, Even skin, Lift shadows). Below come collapsible Light and Color sections in Lightroom's order and names, each with an eye that previews the photo without it, then Color mixer and Retouch, closed until opened. Only sliders the engine renders appear. Crop has aspect ratios, Straighten and a crop box dragged on the uncropped photo. Masking shows Create new mask as a grid (Subject, Sky, Background, People, Brush, Linear, Radial, Hair), the Photo's Masks as rows, and for a People Mask Lightroom's Select People: a face thumbnail per person the engine finds, Everyone, and checkboxes for face skin, body skin, eyes, teeth, hair and the whole person. Linear and Radial Masks are placed with handles on the photo. The Presets tool keeps the full list of Looks, B&W and adaptive presets; hovering any preset, Recommended Look or fix previews it on the photo through the engine. The Agent sits under the open tool. The grid has "Auto all" (⇧A, or on a selection) so a whole Shoot can get Auto before culling. Its Sort menu follows Lightroom's (Capture time, Stars, Flag, File name, each with a direction; ⇧S flips between Stars, most first, and Capture time); a Stack sorts by its best frame and stays together, and the Loupe and filmstrip follow the same order.

## Components

Use existing libraries and don't hand-roll what they already do: shadcn/ui on Radix primitives for controls and dialogs, Tailwind with the tokens below as theme variables, cmdk for the ⌘K palette, TanStack Virtual for the photo grid, and Sonner for undo toasts. Keep screens thin: they read state and call tool-layer verbs, so a layout can be thrown away without touching anything underneath.

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
| border-strong | white 10% | inputs, popovers |
| border-hover | white 16% | hovered tiles, inputs, cards |
| hover / press / selected | white 4% / 7% / 6% | interaction washes over any surface |
| fill / fill-hover | white 5% / 8.5% | secondary buttons, chips and segmented controls (no outline) |
| accent | #D97757 | Claude terracotta: progress, focus, selection |
| accent-strong / on-accent | #C6613F / white | primary button and checked box fill and its text |
| ring | #D97757 at 70% | keyboard focus |
| text | #F0EFEC | primary text |
| text-2 | #B4B2AB | secondary text |
| text-3 | #898782 | captions, counts |
| pick | #7AA6E7 | Pick |
| reject | #F0716B | Reject |
| done | #7FD1A8 | completed steps |
| shoot | #E0AC5A | current Shoot marker |

The window is one flat surface split by hairlines (sidebar | content | Agent), never floating cards with gaps, as in Cursor and Claude. Surfaces are Claude's soft near-blacks rather than pure black, so long sessions are easy on the eye. In the Mac app the title bar is an overlay (traffic lights sit over the sidebar, headers drag the window) and the sidebar shows the system's translucent sidebar material; content stays opaque. `?chrome=mac` previews that layout in a browser.

Hairlines are translucent white so one border reads the same on the ground, on panels and over photos. Buttons don't get them. Greg (2026-10-18) asked for fewer borders and gentler hovers, like the chips in Claude's thread header, so secondary buttons, chips, kbd hints and segmented controls are a soft `--fill` tint with no outline, and hover lifts it to `--fill-hover`. Outlines stay where they carry meaning: inputs, popovers, tiles, the composer and the selected crop ratio's terracotta fill. Primary actions keep the solid terracotta fill and keyboard focus keeps its ring. Raised surfaces get a 1px inner top highlight, and popovers (menus, palette, dialogs, toasts, tooltips) share one layered `--pop` shadow.

Radii are 4 to 7 (chips, tiles, buttons, menu items, sidebar rows), 8 (panels, menus) and 10 (composer, dialogs, ⌘K palette). Spacing follows a 4px grid: panel headers are 40px, sidebar rows 28px, menu and palette rows 28 to 32px.

Type: UI text is 13px with secondary text at 11.5 to 12px, headings are 13 to 15px at weight 500 with slightly tight tracking, and Agent prose is 13.5px sans at a relaxed 1.6 line height. Numbers use tabular figures. Fallback fonts are bundled (Fontsource packages), never fetched from Google.

Motion is quick and quiet: colors and borders ease over 120ms, popovers rise 4px and fade in over 160ms on an ease-out curve and leave faster than they came, buttons press down 1px, and `prefers-reduced-motion` turns it all off. Kbd hints are small sans keycaps (18px tall). The ⌘K palette has a search icon, compact rows with keycaps on the right and a footer that teaches ↑ ↓ ↵ Esc.
