// Lightroom's Auto: exposure, tone, white balance and vibrance worked out from each Photo's
// own light. Live, the engine's `auto_edit` tool measures the real render; in the demo a
// stand-in derived from the Photo keeps the screens clickable.

import type { Adjust, Recipe } from '../domain/types';
import type { Action } from '../store/library';
import { LIVE, asPerson } from './live';

/** The sliders Auto sets (engine/src/auto.rs AUTO_SLIDERS); the rest of a Recipe is kept. */
export const AUTO_KEYS = ['exposure', 'contrast', 'highlights', 'shadows', 'whites', 'blacks', 'temp', 'tint', 'vibrance'] as const;

/** A plausible Auto for a demo Photo, stable per id. */
function demoAuto(id: string): Adjust {
  let h = 0;
  for (const c of id) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const r = (n: number) => ((h >>> n) % 21) - 10; // -10..10
  return { exposure: Math.round(r(0) * 4) / 100 + 0.15, contrast: 8 + r(3), highlights: -25 + r(6), shadows: 22 + r(9), whites: 12 + r(12), blacks: -8 + r(15) / 2, temp: r(18), tint: Math.round(r(21) / 3), vibrance: 12 };
}

/** Auto on these Photos as one undoable step. */
export function autoAction(ids: string[], recipeOf: (id: string) => Recipe): Action {
  const label = ids.length === 1 ? 'Auto' : `Auto on ${ids.length} Photos`;
  if (LIVE) {
    const batch = asPerson('auto_edit', { photo_ids: ids.map(Number) }, label, (r) => Object.keys((r.photos as Record<string, unknown>) ?? {}).map(Number));
    return { type: 'engine/saved', label, batch };
  }
  return {
    type: 'batch',
    label,
    actions: ids.map((id) => {
      const r = recipeOf(id);
      return { type: 'recipe/set', id, recipe: { ...r, global: { ...r.global, ...demoAuto(id) } }, label: 'Auto', by: 'you' };
    }),
  };
}
