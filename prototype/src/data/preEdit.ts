// Edit as you cull: once the person has edited a RAW of the Shoot by hand, the engine works out a
// Shoot match for every Pick and likely keeper they haven't edited (engine `pre_edit_shoot`, no
// model call), each as a Suggestion. Nothing changes until they accept one: ⇧↵ in the Loupe, or
// Accept all in the grid's line (one Undo). With dummy data the matches are made up here.

import { useMemo, useSyncExternalStore } from 'react';
import { toast } from 'sonner';
import { LIVE } from './live';
import { PHOTOS } from './dummy';
import { acceptMany, acceptSuggestion, useShootTakes } from './suggestions';
import { undoBatch } from './liveAgent';
import { starsOf, verdictOf, type Action, type State } from '../store/library';
import type { Suggestion } from '../domain/types';

/** The engine's title for the take (`suggest::SHOOT_MATCH`). */
export const SHOOT_MATCH = 'Shoot match';
/** Unflagged frames rated this high count as likely keepers (`pre_edit::MIN_STARS`). */
const MIN_STARS = 4;

export interface PreEdits {
  /** Open Shoot matches, one per Photo, by Photo id. */
  byPhoto: Record<string, Suggestion>;
  ready: Suggestion[];
  /** How many hand-edited Photos they were worked out from. */
  heroes: number;
}

// Dummy mode: which made-up matches the person answered.
const answered = new Set<string>();
let bump = 0;
const subs = new Set<() => void>();
const changed = () => {
  bump++;
  subs.forEach((f) => f());
};
const subscribe = (f: () => void) => {
  subs.add(f);
  return () => subs.delete(f);
};

const handEdited = (state: State, id: string) => state.versions[id]?.at(-1)?.by === 'you';

export function usePreEdits(state: State): PreEdits {
  const takes = useShootTakes();
  const n = useSyncExternalStore(subscribe, () => bump);
  return useMemo(() => {
    if (LIVE) {
      const ready = Object.values(takes).flatMap((list) => list.filter((s) => s.status === 'open' && s.title === SHOOT_MATCH && s.referenceId != null).slice(0, 1));
      const heroes = Math.max(0, ...ready.map((s) => Number(/the (\d+) Photos you edited/.exec(s.why)?.[1] ?? 1)));
      return { ready, heroes, byPhoto: Object.fromEntries(ready.map((s) => [s.photoId, s])) };
    }
    const all = PHOTOS.slice(0, state.imported);
    const heroes = all.filter((p) => handEdited(state, p.id));
    if (!heroes.length) return { ready: [], heroes: 0, byPhoto: {} };
    const hero = heroes[heroes.length - 1];
    const style = state.recipes[hero.id];
    const ready: Suggestion[] = all
      .filter((p) => !handEdited(state, p.id) && !answered.has(p.id))
      .filter((p) => verdictOf(state, p.id) !== 'reject' && (verdictOf(state, p.id) === 'pick' || starsOf(state, p.id).stars >= MIN_STARS))
      .map((p) => ({
        id: `pre-${p.id}`,
        photoId: p.id,
        rank: 1,
        title: SHOOT_MATCH,
        why: heroes.length === 1 ? 'The style of the Photo you edited, moved for this Photo’s own light.' : `The style of the ${heroes.length} Photos you edited in this Shoot, moved for this Photo’s own light.`,
        // The demo's stand-in for moving the style for each frame's light: its exposure evened out.
        recipe: { ...style, global: { ...style.global, exposure: Number(((style.global.exposure ?? 0) - p.signals.exposure * 0.6).toFixed(2)) } },
        referenceId: hero.id,
        status: 'open' as const,
      }));
    return { ready, heroes: heroes.length, byPhoto: Object.fromEntries(ready.map((s) => [s.photoId, s])) };
  }, [takes, state, n]);
}

/** Accept Shoot matches (one Undo). Live through the engine; in the demo as AI edits. */
export function acceptPreEdits(list: Suggestion[], dispatch: (a: Action) => void) {
  if (!list.length) return;
  const what = list.length === 1 ? 'Shoot match accepted' : `Shoot match accepted on ${list.length} Photos`;
  if (!LIVE) {
    list.forEach((s) => answered.add(s.photoId));
    changed();
    dispatch({ type: 'batch', label: what, actions: list.map((s) => ({ type: 'recipe/set', id: s.photoId, recipe: s.recipe, label: `Suggestion: ${s.title}`, by: 'ai' })) });
    return;
  }
  void (list.length === 1 ? acceptSuggestion(list[0]) : acceptMany(list)).then(({ batch }) =>
    toast(what, {
      id: 'argus',
      action: batch == null ? undefined : { label: 'Undo', onClick: () => void undoBatch(batch).then(() => window.dispatchEvent(new Event('argus:suggestions'))) },
    }),
  );
}
