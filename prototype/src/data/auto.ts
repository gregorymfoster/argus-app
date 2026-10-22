// Lightroom's Auto: exposure, tone, white balance and vibrance worked out from each Photo's
// own light. Live, the engine's `auto_edit` tool measures the real render; in the demo a
// stand-in derived from the Photo keeps the screens clickable.

import type { Adjust, Recipe } from '../domain/types';
import { toast } from 'sonner';
import type { Action, ChatMessage } from '../store/library';
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
export function autoAction(ids: string[], recipeOf: (id: string) => Recipe, onDone?: (applied: number) => void): Action {
  const label = ids.length === 1 ? 'Auto' : `Auto on ${ids.length} Photos`;
  if (LIVE) {
    const batch = asPerson('auto_edit', { photo_ids: ids.map(Number) }, label, (r) => {
      const done = Object.keys((r.photos as Record<string, unknown>) ?? {}).map(Number);
      onDone?.(done.length);
      return done;
    });
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

/**
 * Auto from a screen (Auto all, ⇧A, the Shoot menu, ⌘K): one undoable step, and an entry in
 * the Shoot's chat like an AI cull's: the engine's progress row while it measures each Photo,
 * then what it did, with Undo.
 */
export function runAuto(ids: string[], recipeOf: (id: string) => Recipe, dispatch: (a: Action) => void) {
  if (!ids.length) return;
  let applied = ids.length;
  const action = autoAction(ids, recipeOf, (n) => (applied = n));
  dispatch(action);
  const what = (n: number) => `${n} Photo${n === 1 ? '' : 's'}`;
  const msgId = `auto-${Date.now()}`;
  const t0 = performance.now();
  const say = (patch: Partial<ChatMessage>) => dispatch({ type: 'chat/patch', ctx: 'cull', msgId, patch });
  dispatch({ type: 'chat/push', ctx: 'cull', msg: { id: msgId, role: 'agent', steps: [{ label: `Auto on ${what(ids.length)}`, ms: 0, done: false, task: 'auto' }] } });
  const finish = (batch: number | null) => {
    const secs = ((performance.now() - t0) / 1000).toFixed(1);
    // Cancel stops between Photos; the ones done keep their Auto, undoable as one step.
    const stopped = applied < ids.length;
    say({
      steps: [{ label: `${stopped ? 'Auto stopped' : 'Auto'} on ${what(ids.length)} ·${secs}s`, ms: 0, done: true, stopped }],
      text: stopped ? `Stopped after ${what(applied)} of ${ids.length}: Auto applied to those, the rest are as they were.` : `Auto applied to ${what(ids.length)}: exposure, tone and white balance from each one's own light.`,
      actions: applied ? [{ label: 'Undo', kind: 'undo', batch: batch ?? undefined }] : [],
    });
    toast(stopped ? `Auto stopped after ${what(applied)}` : `Auto applied to ${what(ids.length)}`, { id: 'argus', description: applied ? '⌘Z undoes it.' : undefined });
  };
  if (action.type !== 'engine/saved') return finish(null);
  action.batch.then(
    (w) => finish(w.batch),
    (e) => say({ steps: [{ label: 'Auto failed', ms: 0, done: true, failed: true }], text: String(e).replace(/^Error: /, '') }),
  );
}
