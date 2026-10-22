// A Photo's caption and Keywords (engine: `read_tags`, `set_keywords`, `set_caption`,
// `list_keywords`). Keywords are hierarchical paths under a Facet, Lightroom style
// (`Places|Portugal|Lisbon`); the AI's stay suggestions (dashed) until the person keeps them.
// Live it polls the engine while shown, since the background tagging job can add
// suggestions while the Photo is open; with dummy data it keeps its own small store.

import { useSyncExternalStore } from 'react';
import { call } from './backend';
import { refreshPolls, usePoll } from './poll';
import { LIVE } from './live';

export const FACETS = ['People', 'Places', 'Events', 'Activities', 'Subjects'] as const;
export type Facet = (typeof FACETS)[number];

export interface PhotoKeyword {
  /** Full path: `Places|Portugal|Lisbon`. */
  keyword: string;
  facet: Facet;
  leaf: string;
  author: 'person' | 'ai';
  suggested: boolean;
}

interface Caption {
  text: string;
  author: 'person' | 'ai';
  suggested: boolean;
}

export interface Tags {
  photo_id: number;
  caption: Caption | null;
  keywords: PhotoKeyword[];
}

export interface KeywordCount {
  keyword: string;
  photos: number;
}

const FACET_WORDS: Record<string, Facet> = {
  people: 'People', person: 'People', who: 'People',
  places: 'Places', place: 'Places', location: 'Places', locations: 'Places', where: 'Places',
  events: 'Events', event: 'Events', occasion: 'Events',
  activities: 'Activities', activity: 'Activities', doing: 'Activities',
  subjects: 'Subjects', subject: 'Subjects', things: 'Subjects', what: 'Subjects', topics: 'Subjects',
};

/** The engine's `Keyword::parse`: levels split on `|`, `>`, `/` or `›`; no Facet means Subjects. */
export function parseKeyword(text: string): { facet: Facet; levels: string[] } | null {
  const parts = text
    .split(/[|>/›]/)
    .map((p) => p.replace(/[,;"“”]/g, ' ').trim().replace(/\s+/g, ' '))
    .filter(Boolean);
  const facet = parts.length ? FACET_WORDS[parts[0].toLowerCase()] : undefined;
  if (facet) parts.shift();
  return parts.length ? { facet: facet ?? 'Subjects', levels: parts } : null;
}

export const keywordPath = (k: { facet: Facet; levels: string[] }) => [k.facet, ...k.levels].join('|');

/** How a chip reads: the levels under the Facet, `Portugal › Lisbon`. */
export const keywordLabel = (path: string) => path.split('|').slice(1).join(' › ');

// Dummy data: a few believable tags so the panel shows its states without an engine.
const DEMO: [string, boolean][][] = [
  [['Places|USA|New York', false], ['Activities|relaxing', true], ['Subjects|Nature|autumn', false], ['Subjects|Nature|forest', false], ['Subjects|bench', true], ['Subjects|drink', true], ['Subjects|friends', false]],
  [['Places|USA|New York|Hudson River', true], ['Activities|Travel|boat trip', true], ['Subjects|skyline', true]],
  [['Events|Birthday', false], ['Subjects|cake', true], ['Subjects|candle', true]],
];
const DEMO_CAPTIONS = [
  'Two friends on a wooden bench in an autumn wood, one holding a drink in a jar.',
  'The Manhattan skyline from the deck of a boat on the Hudson River.',
  'Blowing out the candles on a birthday cake at the kitchen table.',
];
const store = new Map<string, Tags>();
const subs = new Set<() => void>();
const notify = () => subs.forEach((f) => f());
const subscribe = (f: () => void) => {
  subs.add(f);
  return () => {
    subs.delete(f);
  };
};

function demo(id: string): Tags {
  let t = store.get(id);
  if (!t) {
    const n = [...id].reduce((a, c) => a + c.charCodeAt(0), 0) % DEMO.length;
    t = {
      photo_id: Number(id) || 0,
      caption: { text: DEMO_CAPTIONS[n], author: 'ai', suggested: true },
      keywords: DEMO[n].map(([keyword, suggested]) => {
        const levels = keyword.split('|');
        return { keyword, facet: levels[0] as Facet, leaf: levels[levels.length - 1], author: suggested ? 'ai' : 'person', suggested };
      }),
    };
    store.set(id, t);
  }
  return t;
}

function editDemo(id: string, f: (t: Tags) => Tags) {
  store.set(id, f(demo(id)));
  notify();
}

/** A Photo's tags: undefined while the first answer is on its way. */
export function useTags(photoId: string): Tags | undefined {
  const live = usePoll<Tags[]>(LIVE ? 'read_tags' : null, { photo_ids: [Number(photoId)] }, 5000);
  const dummy = useSyncExternalStore(subscribe, () => (LIVE ? undefined : demo(photoId)));
  return LIVE ? live?.[0] : dummy;
}

export interface KeywordChange {
  add?: string[];
  remove?: string[];
  accept?: string[];
  acceptAll?: boolean;
}

/** Add, remove or keep Keywords on Photos (the person's own change). */
export async function changeKeywords(photoIds: string[], c: KeywordChange): Promise<void> {
  if (LIVE) {
    await call('set_keywords', {
      photo_ids: photoIds.map(Number),
      ...(c.add?.length ? { add: c.add } : {}),
      ...(c.remove?.length ? { remove: c.remove } : {}),
      ...(c.accept?.length ? { accept: c.accept } : {}),
      ...(c.acceptAll ? { accept_all: true } : {}),
    });
    refreshPolls('read_tags');
    return;
  }
  for (const id of photoIds)
    editDemo(id, (t) => {
      let keywords = t.keywords.filter((k) => !c.remove?.includes(k.keyword));
      keywords = keywords.map((k) => (c.acceptAll || c.accept?.includes(k.keyword) ? { ...k, suggested: false } : k));
      for (const text of c.add ?? []) {
        const k = parseKeyword(text);
        if (!k) continue;
        const keyword = keywordPath(k);
        const at = keywords.findIndex((x) => x.keyword === keyword);
        const row: PhotoKeyword = { keyword, facet: k.facet, leaf: k.levels[k.levels.length - 1], author: 'person', suggested: false };
        if (at >= 0) keywords[at] = { ...keywords[at], suggested: false };
        else keywords.push(row);
      }
      const caption = c.acceptAll && t.caption ? { ...t.caption, suggested: false } : t.caption;
      return { ...t, keywords, caption };
    });
}

/** Set the caption of Photos as the person's own; empty removes it. */
export async function setCaption(photoIds: string[], text: string): Promise<void> {
  if (LIVE) {
    await call('set_caption', { photo_ids: photoIds.map(Number), text });
    refreshPolls('read_tags');
    return;
  }
  for (const id of photoIds) editDemo(id, (t) => ({ ...t, caption: text.trim() ? { text: text.trim(), author: 'person', suggested: false } : null }));
}

/** Keywords in use across the Library, most used first, for autocomplete. */
export async function keywordCounts(prefix: string): Promise<KeywordCount[]> {
  if (LIVE) return call<KeywordCount[]>('list_keywords', { prefix, limit: 8 });
  const counts = new Map<string, number>();
  for (const pool of DEMO) for (const [k] of pool) counts.set(k, (counts.get(k) ?? 0) + 40 + k.length);
  for (const t of store.values()) for (const k of t.keywords) if (!k.suggested) counts.set(k.keyword, (counts.get(k.keyword) ?? 0) + 1);
  counts.set('Activities|hiking', 212);
  counts.set('Places|USA|Hikers Rock', 4);
  const p = prefix.trim().toLowerCase();
  return [...counts]
    .filter(([k]) => !p || k.toLowerCase().startsWith(p) || k.toLowerCase().split('|').some((l) => l.startsWith(p)))
    .sort((a, b) => b[1] - a[1])
    .slice(0, 8)
    .map(([keyword, photos]) => ({ keyword, photos }));
}
