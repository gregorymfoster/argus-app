// Learn as you cull: once the person has flagged a few Picks and Rejects, the AI's stars on the
// rest of the Shoot refresh from them (engine `rerate_shoot`, the Taste head on the Photos'
// embeddings), so likely keepers rise while they cull. Only the AI's stars change; flags,
// Stacks and the person's own stars stay, and it is not an Undo step.

import { useEffect, useRef, useState } from 'react';
import { LIVE, liveShootId, readCull, type CullState } from './live';
import { call } from './backend';
import { PHOTOS } from './dummy';
import type { Action, State } from '../store/library';

/** Fewest Picks and fewest Rejects before the AI reads anything into them (engine `taste::MIN_FLAGS`). */
export const MIN_FLAGS = 5;
/** How long flagging has to pause before the stars refresh. */
const REST_MS = 1200;

export interface Learning {
  picks: number;
  rejects: number;
  /** Enough flags to learn from (here or, live, on other Shoots too). */
  learning: boolean;
  /** Photos whose AI stars changed last time, and when. */
  changed: number;
  at: number;
}

const mine = (s: State) => {
  const v = Object.values(s.overrides);
  return { picks: v.filter((x) => x === 'pick').length, rejects: v.filter((x) => x === 'reject').length };
};

export function useLearnAsYouCull(state: State, dispatch: (a: Action) => void, on: boolean): Learning {
  const { picks, rejects } = mine(state);
  const [l, setL] = useState<Learning>({ picks, rejects, learning: picks >= MIN_FLAGS && rejects >= MIN_FLAGS, changed: 0, at: 0 });
  const latest = useRef(state);
  latest.current = state;
  const culled = Object.keys(state.aiStars).length > 0;
  useEffect(() => {
    if (!on || !culled) return;
    const t = setTimeout(() => {
      void (LIVE ? rerateLive() : rerateDemo(latest.current)).then(({ learning, changed, states }) => {
        if (states) dispatch(states);
        setL({ picks, rejects, learning, changed, at: Date.now() });
      });
    }, REST_MS);
    return () => clearTimeout(t);
  }, [picks, rejects, on, culled, dispatch]);
  return { ...l, picks, rejects };
}

type Rerated = { learning: boolean; changed: number; states?: Action };

async function rerateLive(): Promise<Rerated> {
  const r = await call<{ learning: boolean; changed: number }>('rerate_shoot', { shoot_id: liveShootId() });
  if (!r.changed) return r;
  return { ...r, states: { type: 'cull/states', states: await readCull(PHOTOS.map((p) => p.id)) } };
}

/** The demo's stand-in for the Taste head: each Photo's few signals, from Rejects towards Picks. */
async function rerateDemo(s: State): Promise<Rerated> {
  const { picks, rejects } = mine(s);
  if (picks < MIN_FLAGS || rejects < MIN_FLAGS) return { learning: false, changed: 0 };
  const feat = (id: string) => {
    const p = PHOTOS.find((x) => x.id === id)!;
    const [open, of] = p.signals.eyesOpen;
    return [p.signals.focus, of ? open / of : 1, -Math.abs(p.signals.exposure), ['Awkward', 'Neutral', 'Good', 'Great'].indexOf(p.signals.expression) / 3];
  };
  const mean = (ids: string[]) => ids.map(feat).reduce((a, b) => a.map((x, i) => x + b[i] / ids.length), [0, 0, 0, 0]);
  const by = (v: string) => Object.entries(s.overrides).filter(([, x]) => x === v).map(([id]) => id);
  const [p, r] = [mean(by('pick')), mean(by('reject'))];
  const dir = p.map((x, i) => x - r[i]);
  const ids = Object.keys(s.aiStars).filter((id) => (s.overrides[id] ?? s.aiVerdicts[id]) !== 'reject');
  const score = (id: string) => feat(id).reduce((a, x, i) => a + x * dir[i], 0);
  const ranked = [...ids].sort((a, b) => score(b) - score(a));
  const stars = Object.fromEntries(ranked.map((id, i) => {
    const q = i / Math.max(1, ranked.length);
    return [id, q < 0.1 ? 5 : q < 0.35 ? 4 : q < 0.7 ? 3 : 2];
  }));
  const changed = ids.filter((id) => stars[id] !== s.aiStars[id]);
  const states: CullState[] = changed.map((id) => ({ id: Number(id), verdict: s.aiVerdicts[id] ?? 'unflagged', verdict_by: 'ai', stars: stars[id], stars_by: 'ai' }));
  return { learning: true, changed: changed.length, states: changed.length ? { type: 'cull/states', states } : undefined };
}
