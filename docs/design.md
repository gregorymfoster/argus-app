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

Key hints (`Kbd` in `kit.tsx`). Every common action has a shortcut and shows it somewhere, and ⌘/ (or ?) opens Settings › Shortcuts, the whole list grouped by screen, as Lightroom's Help › Keyboard Shortcuts does. Combinations use the Mac's modifier order, ⌃⌥⇧⌘ (⇧⌘C, never ⌘⇧C); `MenuKeys` sorts them for you. G goes back to the grid and D opens Edit from every view. Keycaps (a small soft cap) belong in menus, the ⌘K palette, tooltips and on a dialog's own buttons (Import ↵, Cancel esc). Everywhere else, in bars, headers, footers and hint lines, use `<Kbd quiet>`: a plain letter with no cap. In the sidebar and on the Loupe's flag row the hint fades in only on hover. Don't write a bare `<kbd>` in a screen; `design-lint` counts them.

Sidebar. Search is a hairline-outlined field, not a filled pill. Import is the first row under it, where modern Lightroom keeps its + and Claude keeps New. Rows are plain text with a muted icon, and only the selected row gets the chip. Counts stay (Lightroom shows them, and Greg wants the information) in the sans at `--text-3`. The open Shoot is marked by the chip and an amber folder. The foot holds background work and the Resource meter as flat rows above one hairline, like Claude's account row.

Standard Mac behavior always works. ⌘, opens Settings, ⌘W and ⌘Q close and quit, ⌘+ ⌘− ⌘0 zoom the app, and Esc closes any dialog. A dialog also has ✕ and Cancel, but a stray click outside never closes it.

## Feedback and motion

Never a bare spinner when progress is knowable (Greg, 2026-10-18). Card scans, imports, previews, culls, Auto, Agent edits, renders and exports show `ProgressBar` from `ui/Progress.tsx`, one compact row (Greg, 2026-10-21, after the cull bar wrapped): what it's doing on the left, one line and ellipsized; "3 of 266 · ~2 h 34 min left" on the right in the sans face with tabular figures (`eta()` formats it); a quiet Cancel after that wherever the work can stop (Import, Auto all, the AI cull: `cancel_task`), which turns the row into "Stopping"; and a 2px terracotta bar underneath, with a soft glint running over the filled part so a slow job still looks alive. `TaskProgress` is the same row for a running engine task (`progress` kind). In the Agent chat a step the engine counts is that row in place of the spinner step, and a step the person stopped ends with a quiet slashed circle, never the red failure cross. Work the person started outside the Queue (an AI cull from the chat, Auto all, Export) also shows in Background work at the foot of the sidebar, ahead of the Queue's own line. A spinner is only acceptable for a wait that has no size and lasts under a couple of seconds. `design-lint` counts spinners.

Every AI action shows as a compact step with its timing, and offers a review or an undo of exactly what it changed. AI work for the open Photo jumps ahead of background batches, and the UI names the AI by `aiName()`, never a hardcoded model.

Keywords and captions live in the Loupe's Info panel, after the Stack, as in the agreed mockup (2026-10-22). Caption is one editable box; Keywords sit in rows by Facet (People, Places, Events, Activities, Subjects), each chip showing the path under its Facet (`Portugal › Lisbon`). The AI's suggestions are dashed terracotta with a sparkle until kept: a click keeps one, × dismisses it (it won't be offered again), and Accept all (⇧↵) keeps every suggestion and the caption. K opens an add field that autocompletes from the Library's own Keywords, most used first, with "New … under <Facet>" last. People are never guessed: the faces the models found show as dashed circles to name.

Search lives in ⌘K (2026-10-22, as mocked). Typing shows the best six Photos across the Library as a strip of thumbnails with what matched, "Show all N in the grid" (⌘↵ from anywhere in the palette), matching Keywords with their counts, and "Save this search as an Album". The results open in the grid as a Library-wide view titled with the words, with "N in M Shoots" and Save as Album beside the title. Albums sit in the sidebar between Library and Shoots, only once there is one; right-click shows the search again or deletes the Album.

Toasts rise from the bottom centre just above the screen's bottom bar (the Cull footer, the Loupe's flag row, the Look bar), so an Undo toast never covers the key hints.

Motion is quick, calm and small (Greg wants "small delightful animations"). Colors and borders ease over `--fast` (120ms). Popovers rise 4px and fade in over 160ms on `--ease-out` and leave faster than they came. Buttons press down 1px. Thumbnails fade in as they load and imported Photos pop into the grid. Nothing animates over the photo while you judge it, and `prefers-reduced-motion` turns it all off.

## Writing in the UI

Labels and buttons are sentence case (Import a Shoot, not Import A Shoot). Use the glossary's words exactly (Library, Photo, Shoot, Pick, Reject, Unflagged, Stack, Look, Mask, Suggestion). Say what a thing does in a few plain words, keep counts and units visible (112 photos, 3.8s, about 11 min), and skip exclamation marks and em-dashes. File, folder and Shoot names shown or created follow `docs/naming.md`.

## Design v3: quieter chrome (Greg, 2026-10-20)

Greg put the Argus sidebar and grid toolbar next to the Claude app's and asked to keep modernizing and simplifying. The gap was boxes. Argus wrapped every group in a tinted tray, gave Import and the background-work card their own bordered cards, and showed keycaps at rest. Claude's chrome has almost none of that. The rules above (one chip per group, ghost header buttons, quiet key hints, the outlined search field, Import as the first row, the flat sidebar foot) came out of that pass and now apply everywhere. In Edit, Auto and B&W are ghost buttons, while one-click fixes and Agent prompts keep their soft fill, since an outline on each would add borders. In Import, the From and New/All pickers are borderless and Cancel is ghost beside the one terracotta Import.

## Agreed patterns

Greg chose these in the prototype on 2026-10-17 and later. They hold until he uses the real app and changes his mind, which we expect, so build them to be easy to swap.

The Agent lives in a side panel on the right, always visible. Import is a focused dialog, Lightroom-style: it finds the memory card by itself (or you choose a folder) and shows every photo as a grid. Photos the Library already has are unticked ("New only"). A RAW+JPEG frame is one Photo, and "RAW only" (on by default) leaves the camera's JPEG behind. The local model suggests the Shoot name, which always carries the shoot's date and has no spaces (`2026-10-18_Boat-Trip`). Pressing Import opens the Shoot. The AI cull is already applied when the grid opens, so Greg scans and flips what he disagrees with rather than being walked through each Photo.

Flags follow Lightroom. Every tile shows a small flag chip in its corner (Pick in blue, Reject in red, nothing when Unflagged) and Rejects fade back. Hovering a tile reveals Pick and Reject buttons on that chip, and clicking one sets it (clicking the active one clears it). P, X and U do the same from the keyboard, and flagging jumps to the next Photo. Shift-click and ⌘-click select several Photos so one keypress flags them all. The AI's reason sits as a caption under each tile, after its stars. The AI's stars are dim with a small sparkle; the person's are solid. Keys 0 to 5 rate and move on like flags. As the person flags, the AI's stars refresh from their Picks and Rejects (learn as you cull): changed stars glow briefly, a quiet footer line counts what it learned from, and a grid sorted by Stars holds its order until Stars is chosen again, so nothing moves under the cursor (Greg, 2026-10-22). Best Photos is a grid filter with a gold award badge carrying the rank.

J cycles what grid tiles show, like Lightroom's grid view styles (2026-10-22): compact (flag, Stack and stars, with the reason left to the footer and the Loupe; the default, Greg 2026-10-22), everything (adds Best Photos, Suggestions, eye marks and the reason), or the photo alone. It is remembered. The AI cull never Picks, so once it is done and nothing is Picked, one line under the grid's header offers the next step: Pick 4★ and up, Pick Best Photos, or Review one by one (the Loupe on the first unflagged frame). It goes away with the first Pick and can be closed per Shoot (Greg, 2026-10-22). Grid thumbnails change size with a trackpad pinch, the size slider in the toolbar, or = and − as in Lightroom's Grid; the Photo under the pointer stays put while it zooms. ⌘+, ⌘− and ⌘0 zoom the whole app (View menu and ⌘K), like Cursor and browsers, for text density. Right-click works on Shoots (Rename, Suggest a name, Show in Finder, Export) and on Photos; double-click or Enter renames a Shoot in place and Esc cancels. The grid's Sort menu follows Lightroom's (Capture time, Stars, Flag, File name, each with a direction; ⇧S flips between Stars, most first, and Capture time); a Stack sorts by its best frame and stays together, and the Loupe and filmstrip follow the same order. The grid has "Auto all" (⇧A, or on a selection) so a whole Shoot can get Auto before culling.

Stacks read as piles (Greg, 2026-10-21: make Stacks and expanding them clearer). A collapsed Stack shows its cover (the kept Pick, else the best frame) with two sheets peeking out above it and a count badge with a layers icon; clicking the badge or pressing S expands it. Expanded frames sit on one soft tinted band behind the tiles and number themselves (2/6), and the cover keeps the layers icon. S from any frame, the badge, or a click on the band collapses the Stack and lands on the cover, as in Lightroom. Right-click has Expand or Collapse Stack and Expand all or Collapse all Stacks. In the filmstrip each Stack's frames share a thin underline, and the open Photo's Stack is underlined in white.

For Looks, chat comes first: Greg describes what he wants and the Agent builds it. Before and after shows as split tiles. AI edits apply directly, with one undo for the whole batch. In deep edit, each Mask is a card with its own sliders.

Optics opens with Lens corrections, a three-way tray (As camera, On, Off) above Lightroom's Remove chromatic aberration checkbox, with one faint line saying where the data comes from. As camera is the default and doesn't count as a change in the section's count.

Copy, Paste and Sync settings follow Lightroom (Greg, 2026-10-21). ⇧⌘C opens Copy settings, a small dialog of Settings groups in two columns (White balance to Retouch ticked, Masks, Spot removal and Crop not, as Lightroom does) that remembers the last choice; ⇧⌘V pastes on the selection or the current Photo, ⌥⌘V pastes the previous Photo's settings with those groups, and ⇧⌘S (or the grid's right-click menu) syncs the current Photo to the rest of the selection through the same dialog. Every paste is one Undo step. A dialog's Esc only closes the dialog: screens pass `ESC_OUTSIDE_DIALOGS` to their own Esc.

The Edit panel copies Lightroom's (Greg, 2026-10-18: "just copy Lightroom for starters"). A thin tool strip down the right edge opens Edit (E), Crop (C), Heal (H), Masking (M), Presets (⇧P) and History. Edit has Auto (⇧A) and B&W (V) on top, a Profile, then the AI takes (your edit first, then each Suggestion with its name and reason, two across), a take's Amount (0 to 150%) with Accept and Dismiss and what it changes, and one-click fixes that toggle (Enhance sky, Brighten faces, Even skin, Lift shadows). Tab and ⇧Tab step through the takes from any tool and bring up Edit, R asks for new ones, and while they come a bar with the stage, ETA and Cancel sits over developing tiles. The takes used to be a strip under the photo and Looks a Recommended row here; Greg chose one place for the AI, with the photo keeping its height, and the recommended Looks moved to Presets (2026-10-22). Below come collapsible Light and Color sections in Lightroom's order and names, each with an eye that previews the photo without it, then Color mixer and Retouch, closed until opened. Only sliders the engine renders appear. Crop has aspect ratios, Straighten and a crop box dragged on the uncropped photo, plus Lightroom's Straighten tool (a button, or ⌘-drag on the photo): draw along a horizon or wall and the photo turns so it is level or plumb (2026-10-22). Masking shows Create new mask as a grid of ten in Lightroom's order (Subject, Sky, Background, People, Hair, then Brush, Linear, Radial, Color and Luminance ranges), the Photo's Masks as rows, and for a People Mask Lightroom's Select People. The selected Mask lists its components under it, indented with an Add, Subtract or Intersect icon, and Add, Subtract and Intersect sit below the rows as menus of the kinds that need no setup; a Color range starts by clicking a colour on the photo (up to five), a Luminance range shows From, To and Smoothness over a tone ramp. Amount (0 to 200) leads the selected Mask's sliders (2026-10-22). Linear and Radial Masks are placed with handles on the photo. Heal is Lightroom's Heal and Clone (2026-10-21): the photo shows uncropped, a click covers that spot from a source Argus picks, drawn as a dashed circle joined by a line, and either circle drags. Heal or Clone, Size, Feather and Opacity set the next Spot or change the selected one; [ and ] size the brush, / picks a new source, Delete removes. Hovering any Preset or fix previews it on the photo through the engine. Presets has Auto, then an Amount slider named after the Preset that's on, then "Suggested for this Shoot" (the AI's top six, with its reason on hover and Rank again; until the AI has ranked the Shoot, the Looks that suit this photo), then Lightroom-style groups collapsed except Essentials; clicking an applied Preset takes it off (2026-10-21). A + in the Presets header (⇧⌘N) is Lightroom's Create Preset: a name and the Settings groups in the Copy settings dialog's two columns (all but White balance and Masks ticked), and the person's own Presets show as a Yours group above Suggested, with Update, Rename and Delete on right-click (2026-10-22). Lens Blur is Lightroom's, as in the agreed mockup (2026-10-22): a closed section after Effects with Amount, then (once Amount is above 0) Bokeh as four icon segments (Circle, Bubble, Blade, Ring), Boost, the Focal range as Subject or Custom with a crosshair (F) to focus on a clicked point, and a depth bar: the depth histogram far to near over the depth ramp with two handles for the range, dimmed outside it. Visualize depth (⇧Z) swaps the photo for the colour depth map. Refine is Off, Focus or Blur: paint on the uncropped photo what stays sharp or goes to the background, ⌥ flips, [ and ] size the brush, Clear removes the strokes. While depth is first computed a "Finding depth" bar shows. The Navigator starts closed and remembers when the person opens it. The Agent sits under the open tool as its composer alone until it has a conversation or the composer has focus, so the sliders and the photo get the height (2026-10-21, after Edit showed three sliders at 1280×800).

People follows Lightroom's People view (as mocked, 2026-10-22). A sidebar row opens a page of round face tiles: Named people first (sorted by most Photos or by name, with a Find field), then To name, one tile per group of faces that look alike. Each group has a small name field under it; typing offers people already named, ↵ names the whole group, and when a group looks like someone named the field says "Name?" with a ✓ to confirm. ⇧-click selects several groups, ⌫ hides faces that aren't worth naming, and dragging a group onto a named person merges it in. A person's page shows "Is this Maya?" faces on top (Y, N, ←→, ⇧↵ confirms all), then their Photos by Shoot, with Open in grid. Faces are only ever named by the person: the AI groups and suggests, never names.

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
| stack-sheet-1 / -2 | #4A4A47 / #343432 | the sheets above a collapsed Stack |
| stack-band / -hover | white 9% / 14% | the band behind an expanded Stack |
| stack-line | white 40% | a Stack's underline in the filmstrip |
| depth-ramp | #1A0F47 → #FCE08C (5 stops) | Lens Blur's depth, far to near: the depth bar and Visualize depth |
| depth-out | #111111 at 62% | dims depths outside the focal range, and the histogram bars |
| refine-focus / refine-blur | white 28% / black 38% | Lens Blur Refine brush dots on the photo |
| row-h | 28px | sidebar and list rows |
| icon | 14px | row icons (13px in headers) |
| fast / ease-out | 120ms / cubic-bezier(0.16, 1, 0.3, 1) | transitions |
| pop | layered shadow | every popover, menu, dialog, toast and tooltip |

In the Mac app the title bar is an overlay (traffic lights sit over the sidebar, headers drag the window) and the sidebar shows the system's translucent sidebar material; content stays opaque. `?chrome=mac` previews that layout in a browser. Raised surfaces get a 1px inner top highlight.

## Checking a UI change

`npm run design-lint` (also part of `just check`) fails when a file gains a color literal outside the token blocks, a radius or font size off the scales above, a bare `<kbd>`, a spinner or an em-dash in UI text. It is a ratchet over `prototype/scripts/design-lint-baseline.json`: older code keeps its count until someone cleans it, and `--update` lowers the baseline after a cleanup. `just layout` checks headers, footers and the sidebar foot (also squeezed to 150 px) for wrapped or clipped labels. Labels in a bar or the sidebar never wrap: they ellipsize, and secondary bits like a version number drop out first (Greg, 2026-10-20, after Restart to update wrapped). `node prototype/scripts/design-shots.mjs OUTDIR '?chrome=mac#/cull'` takes retina shots of the window, sidebar and Cull toolbar (any route works, e.g. `#/loupe/p3`, `#/bursts`) to look at before and after.

Search by meaning is a one-time download, offered in Settings › AI and Compute › Search as a Download button that turns into a progress bar with MB and an ETA while it runs (Greg, 2026-10-22: download on first use rather than bundle it). Until it is installed, ⌘K searches by words alone.
