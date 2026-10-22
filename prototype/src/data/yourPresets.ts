// The person's own Presets (group Yours), as Lightroom's Create Preset makes them. Live they
// live in the Library (engine `save_preset`, `delete_preset`, listed by `list_presets`); in
// the demo they stay in this tab. Either way domain/presets reads them through
// setYourPresets, so applying one works like any other Preset.

import { useEffect, useSyncExternalStore } from 'react';
import { call } from './backend';
import { LIVE } from './live';
import { YOURS, presetFromRecipe, setYourPresets, yourPresets, type Preset } from '../domain/presets';
import type { Recipe } from '../domain/types';

const listeners = new Set<() => void>();
function publish(list: Preset[]) {
  setYourPresets(list);
  listeners.forEach((l) => l());
}
const subscribe = (l: () => void) => (listeners.add(l), () => void listeners.delete(l));

let loaded = false;
async function load() {
  const r = await call<{ presets: Preset[] }>('list_presets');
  publish(r.presets.filter((p) => p.group === YOURS));
}

/** The person's own Presets, kept current. */
export function useYourPresets(): Preset[] {
  useEffect(() => {
    if (LIVE && !loaded) {
      loaded = true;
      load().catch(() => (loaded = false));
    }
  }, []);
  return useSyncExternalStore(subscribe, yourPresets);
}

const slug = (name: string) => name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'preset';

/**
 * Save the Photo's settings in `groups` as a Preset named `name`, or update `id` with them.
 * Rejects with the engine's message (a name already taken, no groups).
 */
export async function savePreset(name: string, photoId: string, recipe: Recipe, groups: string[], id?: string): Promise<Preset> {
  if (LIVE) {
    const p = await call<Preset>('save_preset', { name, photo_id: Number(photoId), groups, ...(id ? { id } : {}) });
    await load();
    return { ...p, settings: (p as { settings?: string[] }).settings };
  }
  const list = yourPresets();
  if (!name.trim()) throw new Error('A Preset needs a name');
  if (list.some((p) => p.name.toLowerCase() === name.trim().toLowerCase() && p.id !== id)) throw new Error(`You already have a Preset named “${name.trim()}”`);
  const p = presetFromRecipe(name, recipe, groups);
  if (id) p.id = id;
  else {
    const base = `yours-${slug(name)}`;
    let n = 1;
    p.id = base;
    while (list.some((x) => x.id === p.id)) p.id = `${base}-${++n}`;
  }
  publish(id ? list.map((x) => (x.id === id ? p : x)) : [...list, p]);
  return p;
}

export async function renamePreset(id: string, name: string): Promise<void> {
  if (LIVE) {
    await call('save_preset', { id, name });
    return load();
  }
  if (!name.trim()) throw new Error('A Preset needs a name');
  publish(yourPresets().map((p) => (p.id === id ? { ...p, name: name.trim() } : p)));
}

export async function deletePreset(id: string): Promise<void> {
  if (LIVE) {
    await call('delete_preset', { id });
    return load();
  }
  publish(yourPresets().filter((p) => p.id !== id));
}
