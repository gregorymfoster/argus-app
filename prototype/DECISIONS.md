# Prototype decisions

What Greg chose in the prototype, so the next round starts from here instead of from scratch. `docs/design.md` ("Agreed patterns" and "Components") is the source of truth; this file records how the prototype implements it and what's still open. Expect all of it to change once Greg uses the real app.

## Greg's picks (2026-10-17)

The Agent lives in a side panel on the right. AI edits apply directly, and the whole batch undoes in one step (Sonner toast with Undo, or ⌘Z).

Import is a focused dialog over the grid. Pressing Import starts reading and opens the Shoot at once, with no separate "Open shoot" step; thumbnails fill in behind. The AI cull is already applied when the grid opens.

Flags work like Lightroom. Each tile has a corner chip (blue Pick, orange Reject, nothing when Unflagged), Rejects fade back, and the AI's reason is the caption. Hovering a tile (or landing on it with the arrows) swaps the chip for Pick and Reject buttons; clicking the active one clears it. P, X and U flag from the keyboard and jump to the next Photo. Shift-click and ⌘-click select a range or a set, and one key flags them all. Right-click opens the same actions as a menu. The old Maybe is gone: a close call is simply Unflagged.

Grid density defaults to Comfortable (about 5 across), with Dense (about 8) one click away, remembered per browser. The loupe keeps a side panel with the reason, the signals and three flag buttons, and setting a flag moves on.

Choosing the best of a burst uses Compare, Lightroom's Survey view. Press C on a Stack (or on a multi-selection), see the frames side by side with Claude's pick crowned, then flag each or press K to keep the focused one and reject the rest. Cross-photo AI context (comparing similar frames, keeping a consistent style across the Shoot) is engine work and went to the engine thread.

Looks are Shoot-wide. Describe the Look to the Agent and it lands on every Pick, with per-photo Corrections underneath so the set matches. Before and after shows as split tiles. In Edit, "Sync to Look" (⇧S) folds one photo's sliders into the Shoot's Look, like Lightroom's Sync. Each Mask is a card with its own sliders.

Paths never contain spaces. Defaults join words with hyphens and a date prefix with an underscore (`~/Pictures/Argus/2026-10-12_tahoe`), and a typed space becomes a hyphen.

## Components

shadcn/ui on Radix for controls and dialogs (hand-written into `src/components/ui/` because the shadcn registry is blocked from the build container; the files match shadcn's shapes, so `npx shadcn add` can replace them later), Tailwind v4 with the design tokens as theme colors, cmdk for ⌘K, TanStack Virtual for the grid, Sonner for toasts, react-hotkeys-hook for shortcuts and lucide for icons. `src/ui/kit.tsx` composes these into Argus-specific pieces (labelled slider, segmented toggle, tooltip with a key hint), so a control changes in one place.

## Asking about the next open question

Add a `Decision` to `DECISIONS` in `src/store/variants.tsx` and branch on `useVariant(id)` in the screen. The amber Variants panel appears by itself when the list isn't empty and hides when it is. Once Greg picks, write the winner here, delete the switch and the losing code.
