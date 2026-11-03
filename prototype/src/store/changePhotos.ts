// One change to many Photos as a single ⌘Z step, on whichever adapter is present: on a live
// Library the engine runs a tool as the person (one batch of theirs in the Library, read back
// into the store); with demo data the store sets each Photo's Recipe itself. The Apply
// popover's verbs (Apply look, Crop & straighten, Paste) go through here, so the undo and
// batch rules live in one place and a screen only says what to change and what to tell the
// person.

import { LIVE, asPerson } from '../data/live';
import type { Actor, Recipe } from '../domain/types';
import type { Action } from './library';

export interface PhotosChange {
  /** The Undo step's name. */
  label: string;
  /** Live: the tool the engine runs, its arguments, and the Photos whose Recipes its reply says it changed. */
  tool: string;
  args: Record<string, unknown>;
  written: (reply: Record<string, unknown>) => number[];
  /** Demo: each Photo's new Recipe. */
  local: () => { id: string; recipe: Recipe }[];
  by: Actor;
  /** The name of each Photo's own change in the demo (the step's label if left out). */
  each?: string;
}

/** Make the change. Resolves with how many Photos changed and the engine's reply (`{}` in the demo). */
export function changePhotos(dispatch: (a: Action) => void, c: PhotosChange): Promise<{ changed: number; reply: Record<string, unknown> }> {
  if (!LIVE) {
    const next = c.local();
    dispatch({ type: 'batch', label: c.label, actions: next.map(({ id, recipe }) => ({ type: 'recipe/set' as const, id, recipe, label: c.each ?? c.label, by: c.by })) });
    return Promise.resolve({ changed: next.length, reply: {} });
  }
  let reply: Record<string, unknown> = {};
  const batch = asPerson(c.tool, c.args, c.label, (r) => {
    reply = r;
    return c.written(r);
  });
  dispatch({ type: 'engine/saved', label: c.label, batch });
  return batch.then((w) => ({ changed: Object.keys(w.recipes ?? {}).length, reply }));
}
