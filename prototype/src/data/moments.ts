// Cull mode's two engine calls: log a choice to the Usage log (bench data, `moment` in
// docs/usage-log.md) and merge Stacks the AI split. Both work on the dummy Shoot too.

import { call } from './backend';
import { PHOTO_BY_ID, STACKS, STACK_BY_ID } from './dummy';
import { LIVE, liveShootId } from './live';

export interface MomentLog {
  moment_id: string;
  kind: 'stack' | 'stacks' | 'singles';
  shown: string[];
  stacks: string[][];
  kept: string[];
  rejected: string[];
  ai_best: string[];
  zoomed: string[];
  looked: string[];
  seconds: number;
  rest: 'unflagged' | 'reject';
  /** The AI's best was marked on screen (the Best chip), so the keep isn't blind. */
  ai_best_shown: boolean;
  outcome: 'some' | 'none' | 'all';
  merged_from?: string[][];
}

/** Choices logged this session, newest last (the dummy Shoot keeps them here; tests read it). */
export const LOGGED: MomentLog[] = [];

export function logMoment(m: MomentLog) {
  LOGGED.push(m);
  if (!LIVE) return;
  const ids = (l: string[]) => l.map(Number);
  void call('log_moment', {
    ...m,
    shown: ids(m.shown),
    stacks: m.stacks.map(ids),
    kept: ids(m.kept),
    rejected: ids(m.rejected),
    ai_best: ids(m.ai_best),
    zoomed: ids(m.zoomed),
    looked: ids(m.looked),
    merged_from: m.merged_from?.map(ids) ?? [],
  }).catch((e) => console.warn('usage log: moment', e));
}

/** Frames of a Stack, best first. */
const ranked = (id: string) =>
  STACK_BY_ID[id].photoIds.slice().sort((a, b) => (PHOTO_BY_ID[a].stackRank ?? 99) - (PHOTO_BY_ID[b].stackRank ?? 99));

/** Merge Stacks into one, the first Stack's frames first. Returns the new Stack's id. */
export async function mergeStacks(stackIds: string[]): Promise<string> {
  const frames = stackIds.flatMap(ranked);
  if (LIVE) {
    const shoot = liveShootId();
    const all = STACKS.filter((s) => !stackIds.includes(s.id)).map((s) => ranked(s.id).map(Number));
    await call('set_stacks', { shoot_id: shoot, stacks: [...all, frames.map(Number)] });
    // The engine renumbers Stacks: read them back.
    const photos = await call<{ id: number; stack: number | null; stack_rank: number | null }[]>('photos', { shoot_id: shoot });
    for (const p of photos) {
      const ph = PHOTO_BY_ID[String(p.id)];
      if (!ph) continue;
      ph.stackId = p.stack == null ? undefined : `s${p.stack}`;
      ph.stackRank = p.stack_rank ?? undefined;
    }
  } else {
    const into = stackIds[0];
    frames.forEach((id, i) => Object.assign(PHOTO_BY_ID[id], { stackId: into, stackRank: i }));
  }
  const stacks = new Map<string, string[]>();
  for (const p of Object.values(PHOTO_BY_ID)) if (p.stackId) stacks.set(p.stackId, [...(stacks.get(p.stackId) ?? []), p.id]);
  STACKS.splice(0, STACKS.length, ...[...stacks].map(([id, photoIds]) => ({ id, photoIds })));
  for (const k of Object.keys(STACK_BY_ID)) delete STACK_BY_ID[k];
  for (const s of STACKS) STACK_BY_ID[s.id] = s;
  return PHOTO_BY_ID[frames[0]].stackId!;
}
