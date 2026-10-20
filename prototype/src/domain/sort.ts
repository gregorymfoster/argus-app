// The grid's sort order, as in Lightroom's Sort menu. Photos come from the engine in capture
// order, so `index` is capture time. A Stack sorts as one unit by the frame it shows (its kept Pick, else rank 0) and
// keeps its frames together in rank order; ties fall back to capture order.

import type { Photo, Verdict } from './types';

export type SortKey = 'capture' | 'stars' | 'flag' | 'file';
export interface Sort {
  key: SortKey;
  desc: boolean;
}

export const SORTS: { key: SortKey; label: string; short: string; desc: boolean; dirs: [asc: string, desc: string] }[] = [
  { key: 'capture', label: 'Capture time', short: 'Time', desc: false, dirs: ['Oldest first', 'Newest first'] },
  { key: 'stars', label: 'Stars', short: 'Stars', desc: true, dirs: ['Fewest first', 'Most first'] },
  { key: 'flag', label: 'Flag', short: 'Flag', desc: true, dirs: ['Rejects first', 'Picks first'] },
  { key: 'file', label: 'File name', short: 'Name', desc: false, dirs: ['A to Z', 'Z to A'] },
];

export const DEFAULT_SORT: Sort = { key: 'capture', desc: false };

/** "stars:desc" and back, for a remembered pref. */
export const sortToString = (s: Sort) => `${s.key}:${s.desc ? 'desc' : 'asc'}`;
export function sortFromString(v: string | null | undefined): Sort {
  const [key, dir] = (v ?? '').split(':');
  return SORTS.some((s) => s.key === key) ? { key: key as SortKey, desc: dir === 'desc' } : DEFAULT_SORT;
}

/** A key's natural direction: Stars and Flag put the best first, time and name go up. */
export const sortBy = (key: SortKey): Sort => ({ key, desc: SORTS.find((s) => s.key === key)!.desc });

const FLAG_RANK: Record<Verdict, number> = { reject: 0, unflagged: 1, pick: 2 };

export interface SortFacts {
  stars: (id: string) => number;
  verdict: (id: string) => Verdict | null;
}

/** `photos` in `sort` order. `stacks` keeps each Stack together behind its best frame. */
export function sortPhotos(photos: Photo[], sort: Sort, facts: SortFacts, stacks = true): Photo[] {
  if (sort.key === 'capture' && !sort.desc) return [...photos].sort((a, b) => a.index - b.index);
  const value = (p: Photo): number | string =>
    sort.key === 'stars' ? facts.stars(p.id) : sort.key === 'flag' ? FLAG_RANK[facts.verdict(p.id) ?? 'unflagged'] : sort.key === 'file' ? p.file.toLowerCase() : p.index;
  // Each Stack sorts by the frame it shows collapsed: its best Pick, else its best frame.
  const leadRank = (p: Photo) => (facts.verdict(p.id) === 'pick' ? -100 : 0) + (p.stackRank ?? 0);
  const lead = new Map<string, Photo>();
  if (stacks)
    for (const p of photos) {
      if (!p.stackId) continue;
      const l = lead.get(p.stackId);
      if (!l || leadRank(p) < leadRank(l)) lead.set(p.stackId, p);
    }
  const head = (p: Photo) => (stacks && p.stackId ? lead.get(p.stackId)! : p);
  const dir = sort.desc ? -1 : 1;
  return [...photos].sort((a, b) => {
    const ha = head(a);
    const hb = head(b);
    if (ha !== hb) {
      const va = value(ha);
      const vb = value(hb);
      if (va !== vb) return (va < vb ? -1 : 1) * dir;
      return ha.index - hb.index;
    }
    return (a.stackRank ?? 0) - (b.stackRank ?? 0) || a.index - b.index;
  });
}
