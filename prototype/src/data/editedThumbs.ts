// Live: the grid, filmstrip, Loupe and Compare show an edited Photo with its edits, as in Lightroom.
// The Library's previews are of the unedited Photo, so an edited Photo's thumbnail is the
// engine's edited preview (`edited_previews`): a JPEG the engine keeps on disk by what it
// shows, so after the first render it loads at file speed, across launches too. Here it is
// keyed by the Recipe's stamp (`photos` → `edit`) so it is asked for again only when the
// Recipe changes. Requests go in small batches, two at a time, newest first, so the tiles
// on screen fill before ones scrolled past; when Recipes change (Auto on a whole Shoot),
// every changed Photo's tile is queued behind them, so scrolling finds them ready.

import { useEffect, useState } from 'react';
import { call, libraryUrl } from './backend';
import { photosQuery } from './live';
import type { Photo } from '../domain/types';

const stamps = new Map<string, string | null>();
const urls = new Map<string, string>();
const listeners = new Set<() => void>();
let wanted: string[] = [];
/** Changed Photos' tiles, rendered ahead once nothing on screen is waiting. */
let warm: string[] = [];
let running = 0;

const keyOf = (id: string, edge: number) => {
  const s = stamps.get(id);
  return s ? `${id}|${edge}|${s}` : null;
};
const notify = () => listeners.forEach((f) => f());

/** The grid's tile size: changed Photos are rendered ahead at this size. */
export const TILE_EDGE = 400;
const BATCH = 4;

function pump() {
  while (running < 2 && (wanted.length || warm.length)) {
    // The newest request and up to three more at the same size.
    const from = wanted.length ? wanted : warm;
    const batch: string[] = [];
    let edge = '';
    for (let i = from.length - 1; i >= 0 && batch.length < BATCH; i--) {
      const key = from[i];
      const [id, e] = key.split('|');
      if (urls.has(key) || keyOf(id, Number(e)) !== key) {
        from.splice(i, 1);
        continue;
      }
      if (edge && e !== edge) continue;
      edge = e;
      batch.push(key);
      from.splice(i, 1);
    }
    if (!batch.length) return;
    running++;
    const ids = batch.map((k) => Number(k.split('|')[0]));
    call<Record<string, string | null>>('edited_previews', { photo_ids: ids, long_edge: Number(edge) })
      .then((r) => {
        for (const key of batch) {
          const rel = r[key.split('|')[0]];
          if (rel) urls.set(key, libraryUrl(rel));
        }
        if (urls.size > 4000) urls.delete(urls.keys().next().value!);
      })
      .catch(() => {}) // the unedited preview stays: better than a hole in the grid
      .finally(() => {
        running--;
        notify();
        pump();
      });
  }
}

/** Re-read which Photos are edited (after any change the engine saved, undo included). */
export async function refreshEditStamps(): Promise<void> {
  const photos = await call<{ id: number; edit?: string | null }[]>('photos', photosQuery).catch(() => null);
  if (!photos) return;
  let changed = false;
  for (const p of photos) {
    const id = String(p.id);
    if (stamps.get(id) === (p.edit ?? null)) continue;
    stamps.set(id, p.edit ?? null);
    changed = true;
    // In grid order: pump takes from the end.
    if (p.edit) warm.unshift(`${id}|${TILE_EDGE}|${p.edit}`);
  }
  if (!changed) return;
  notify();
  pump();
}

/** `photo` rendered with its edits at `edge` px, or null to show the plain preview. */
export function useEdited(photo: Photo, on: boolean, edge: number): string | null {
  if (on && !stamps.has(photo.id)) stamps.set(photo.id, photo.edit ?? null);
  const [, bump] = useState(0);
  useEffect(() => {
    if (!on) return;
    const f = () => bump((n) => n + 1);
    listeners.add(f);
    return () => void listeners.delete(f);
  }, [on]);
  const key = on ? keyOf(photo.id, edge) : null;
  useEffect(() => {
    if (!key || urls.has(key)) return;
    wanted = [...wanted.filter((k) => k !== key), key];
    pump();
    return () => void (wanted = wanted.filter((k) => k !== key));
  }, [key]);
  if (!key) return null;
  // While a new edit renders, the previous render of the Photo beats the unedited preview.
  return urls.get(key) ?? [...urls].reverse().find(([k]) => k.startsWith(`${photo.id}|${edge}|`))?.[1] ?? null;
}
