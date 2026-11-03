// The Library grid's filters (flag, stars, Rounds, AI), shared with the Loupe so its filmstrip and
// arrow keys walk only what the grid shows, as Lightroom's filmstrip follows the Library filter.
// Remembered for this viewer, so they stay on until the person changes them (Greg, 2026-11-02).
import type { Photo, Verdict } from '../domain/types';
import { roundIn, starsOf, verdictOf, type State } from '../store/library';
import { usePref } from '../store/variants';
import { useShootTakes } from './suggestions';
import { useAiStatus } from './aiStatus';

export type Filter = 'all' | Verdict | 'best';
export type AiFilter = 'any' | 'ready' | 'waiting';
export const FILTERS: readonly Filter[] = ['all', 'pick', 'reject', 'unflagged', 'best'];

/** A remembered pref limited to known values, so a stale or hand-edited one falls back. */
function usePrefOf<T extends string>(key: string, allowed: readonly T[], fallback: T): [T, (v: T | ((v: T) => T)) => void] {
  const [raw, setRaw] = usePref<T>(key, fallback);
  const v = allowed.includes(raw) ? raw : fallback;
  return [v, (next) => setRaw(typeof next === 'function' ? next(v) : next)];
}

export function useGridFilter() {
  const [filter, setFilter] = usePrefOf<Filter>('grid-filter', FILTERS, 'all');
  const [stars, setStars] = usePrefOf<string>('grid-min-stars', ['0', '1', '2', '3', '4', '5'], '0');
  const [round, setRound] = usePref<string>('grid-min-round', '0');
  const [aiFilter, setAiFilter] = usePrefOf<AiFilter>('grid-ai-filter', ['any', 'ready', 'waiting'], 'any');
  const takes = useShootTakes();
  const aiStatus = useAiStatus();
  const openTakes = (id: string) => (takes[id] ?? []).filter((t) => t.status === 'open').length;
  const aiOf = (id: string) => aiStatus.of(id, openTakes(id) > 0);
  const aiOk = (p: Photo) => aiFilter === 'any' || (aiFilter === 'ready' ? openTakes(p.id) > 0 : aiOf(p.id) != null);
  return {
    filter,
    setFilter,
    minStars: Number(stars),
    setMinStars: (n: number) => setStars(String(n)),
    minRound: Number(round) || 0,
    setMinRound: (n: number) => setRound(String(n)),
    aiFilter,
    setAiFilter,
    takes,
    aiStatus,
    openTakes,
    aiOf,
    aiOk,
  };
}

/** Whether a Photo passes the flag, stars and Round filters (Best Photos is the cull's short list, minus Rejects). */
export function passesFilter(state: State, p: Photo, f: { filter: Filter; minStars: number; minRound: number }): boolean {
  if (f.minStars && starsOf(state, p.id).stars < f.minStars) return false;
  if (f.minRound && roundIn(state, p.id).round < f.minRound) return false;
  if (f.filter === 'best') return p.best != null && Object.keys(state.aiVerdicts).length > 0 && state.overrides[p.id] !== 'reject';
  return f.filter === 'all' || (verdictOf(state, p.id) ?? 'unflagged') === f.filter;
}
