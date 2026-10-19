// Remove from Library (Lightroom's Remove): Photos or a whole Shoot leave the Library, and
// Undo (the toast's button, or ⌘Z right after) puts them back exactly. Live, the engine
// keeps what was removed in the Library's trash.db (engine/src/library/remove.rs); with
// dummy data the Photos are held here.

import { call } from './backend';
import { OTHER_SHOOTS, PHOTOS, PHOTO_BY_ID, SHOOT, STACKS, STACK_BY_ID } from './dummy';
import { LIVE, openShoot, refreshLivePhotos } from './live';
import type { Photo } from '../domain/types';

export interface Removed {
  removal: number;
  label: string;
  photos: number;
  shoots: number;
  originals_trashed: number;
  errors: string[];
}

/** The newest removal, so ⌘Z can undo it when it's the last thing that happened. */
let last: { at: number; undo: () => Promise<number | null> } | null = null;

export const lastRemovalAt = () => last?.at ?? 0;

/** Undo the newest removal. Resolves with how many Photos the grid shows now (null when the page reloads). */
export function undoRemoval(): Promise<number | null> {
  const l = last;
  last = null;
  return l ? l.undo() : Promise.resolve(null);
}

const PENDING = 'argus:removed';

/** Take Photos out of the open Shoot in place, returning how to put them back. */
function drop(ids: Set<string>): () => void {
  const gone: [number, Photo][] = PHOTOS.map((p, i) => [i, p] as [number, Photo]).filter(([, p]) => ids.has(p.id));
  const stacks = STACKS.map((s) => ({ s, ids: [...s.photoIds] }));
  for (let i = PHOTOS.length - 1; i >= 0; i--) if (ids.has(PHOTOS[i].id)) PHOTOS.splice(i, 1);
  for (const id of ids) delete PHOTO_BY_ID[id];
  for (const s of STACKS) s.photoIds = s.photoIds.filter((id) => !ids.has(id));
  SHOOT.count = PHOTOS.length;
  return () => {
    for (const [i, p] of gone) {
      PHOTOS.splice(i, 0, p);
      PHOTO_BY_ID[p.id] = p;
    }
    for (const { s, ids: before } of stacks) {
      s.photoIds = before;
      STACK_BY_ID[s.id] = s;
    }
    SHOOT.count = PHOTOS.length;
  };
}

/** Remove Photos of the open Shoot. Resolves with what went and the grid's new length. */
export async function removePhotos(ids: string[], deleteOriginals: boolean): Promise<{ removed: Removed; shown: number }> {
  let removed: Removed;
  if (LIVE) removed = await call<Removed>('remove_photos', { photo_ids: ids.map(Number), delete_originals: deleteOriginals });
  else removed = { removal: Date.now(), label: `Removed ${ids.length} Photo${ids.length === 1 ? '' : 's'}`, photos: ids.length, shoots: 0, originals_trashed: deleteOriginals ? ids.length : 0, errors: [] };
  const putBack = drop(new Set(ids));
  last = {
    at: Date.now(),
    undo: async () => {
      if (!LIVE) {
        putBack();
        return PHOTOS.length;
      }
      await call('restore_removed', { removal: removed.removal });
      return refreshLivePhotos();
    },
  };
  return { removed, shown: PHOTOS.length };
}

/**
 * Remove a whole Shoot. Live, the page reloads (onto the next Shoot when it was the open
 * one) and shows the Undo toast after: see `takeRemovedToast`.
 */
export async function removeShoot(id: string, deleteOriginals: boolean): Promise<Removed | null> {
  if (!LIVE && id === SHOOT.id) {
    const n = PHOTOS.length;
    const putBack = drop(new Set(PHOTOS.map((p) => p.id)));
    last = { at: Date.now(), undo: async () => (putBack(), PHOTOS.length) };
    return { removal: Date.now(), label: `Removed ${SHOOT.name} (${n} Photos)`, photos: n, shoots: 1, originals_trashed: 0, errors: [] };
  }
  if (!LIVE) {
    const i = OTHER_SHOOTS.findIndex((s) => s.id === id);
    if (i < 0) return null;
    const [shoot] = OTHER_SHOOTS.splice(i, 1);
    const removed = { removal: Date.now(), label: `Removed ${shoot.name} (${shoot.count} Photos)`, photos: shoot.count, shoots: 1, originals_trashed: 0, errors: [] };
    last = {
      at: Date.now(),
      undo: async () => {
        OTHER_SHOOTS.splice(i, 0, shoot);
        return PHOTOS.length;
      },
    };
    return removed;
  }
  const removed = await call<Removed>('remove_photos', { shoot_ids: [Number(id)], delete_originals: deleteOriginals });
  try {
    sessionStorage.setItem(PENDING, JSON.stringify({ ...removed, shoot: id }));
  } catch {
    // No storage: the Shoot is removed all the same; Settings › Library can restore it.
  }
  if (id === SHOOT.id) openShoot(OTHER_SHOOTS[0]?.id ?? '');
  else location.reload();
  return null;
}

/** After a reload that removed a Shoot: what was removed, once, with Undo armed. */
export function takeRemovedToast(): Removed | null {
  let r: (Removed & { shoot: string }) | null = null;
  try {
    r = JSON.parse(sessionStorage.getItem(PENDING) ?? 'null');
    sessionStorage.removeItem(PENDING);
  } catch {
    return null;
  }
  if (!r) return null;
  const shoot = r.shoot;
  const removal = r.removal;
  last = {
    at: Date.now(),
    undo: async () => {
      await call('restore_removed', { removal });
      openShoot(shoot);
      return null;
    },
  };
  return r;
}

/** Removals the Library can still restore, newest first (Settings › Library). */
export interface Removal {
  removal: number;
  label: string;
  at: string;
  photos: number;
  shoots: number;
  originals_trashed: number;
}

export const listRemoved = () => (LIVE ? call<Removal[]>('list_removed') : Promise.resolve([] as Removal[]));
export const restoreRemoved = (removal: number) => call<Removed>('restore_removed', { removal });
export const emptyRemoved = () => call<{ emptied: number }>('empty_removed', {});
