// Keepers first: the review order where the unflagged frames ahead re-order as the person
// flags, the likeliest keeper first (engine `rerate_shoot`'s `order`, from the Taste head).
// What they have passed and the frame they are on never move: each new order is merged in
// only after the cursor the screen on show reports, so the Loupe's next frame is always the
// best guess left. One order for the open Shoot, shared by the grid, the Loupe and the filmstrip.

import { useSyncExternalStore } from 'react';

let held: string[] = [];
let latest: string[] = [];
let at: string | null = null;
let shown: string[] = [];
let moved = new Set<string>();
let movedAt = 0;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((f) => f());

/** The screen's cursor (the Photo the person is on) and the order it shows: frames up to the
 * cursor keep their place, even before the first order from the engine has arrived. */
export function keepersAt(id: string | null, order: string[]) {
  at = id;
  shown = order;
}

/** A fresh order from the engine (likeliest first). `done(id)`: the person already flagged it,
 * so it has no reason to move up. `reset` starts over (Keepers first was just chosen). */
export function keepersOrder(next: string[], done: (id: string) => boolean, reset = false) {
  latest = next;
  const base = held.length ? held : shown;
  const was = base;
  if (reset || !at || !base.includes(at)) {
    held = next;
    moved = new Set();
  } else {
    const cut = base.indexOf(at) + 1;
    const behind = base.slice(0, cut);
    const keep = new Set(behind);
    const ahead = next.filter((id) => !keep.has(id));
    // Flagged frames ahead were judged already: they follow the unflagged ones, as they were.
    const fresh = ahead.filter((id) => !done(id));
    const judged = base.slice(cut).filter((id) => done(id) && ahead.includes(id));
    held = [...behind, ...fresh, ...judged];
    const pos = (list: string[], id: string) => list.indexOf(id);
    moved = new Set(fresh.filter((id) => pos(was, id) > pos(held, id) + 2));
  }
  movedAt = Date.now();
  emit();
}

/** Keepers first was just chosen: the whole Shoot in the latest order. */
export function keepersReset() {
  held = latest;
  moved = new Set();
  emit();
}

/** Each Photo's place in the held order (lower comes first); Photos it lacks (Rejects, Photos
 * imported since) go last. */
export function keeperRank(id: string): number {
  const i = held.indexOf(id);
  return i < 0 ? held.length + 1 : i;
}

/** Frames that just moved up, to ring them softly for a few seconds. */
export const movedUp = () => (Date.now() - movedAt < 6000 ? moved : new Set<string>());

const subscribe = (f: () => void) => {
  listeners.add(f);
  return () => listeners.delete(f);
};
let version = 0;
listeners.add(() => version++);

/** Re-render when the held order changes; returns how many frames moved up last time. */
export function useKeepers(): { version: number; moved: number } {
  const v = useSyncExternalStore(subscribe, () => version);
  return { version: v, moved: movedUp().size };
}
