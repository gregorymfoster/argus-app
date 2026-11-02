// The Shoot look (Greg, 2026-10-25): while the person edits a hero, the whole Shoot shows the
// edit, each frame matched to its own light (engine `shoot_look_preview`, no model call). The
// first pass reads each frame's light in small batches, so the strip fills in with a real
// progress bar; after that a slider drag re-matches and redraws every tile, latest wins.
// Edit's Apply look popover (ui/ApplyLook.tsx) draws it and applies it.

import { useEffect, useRef, useState } from 'react';
import { call } from './backend';
import { LIVE } from './live';
import type { Recipe } from '../domain/types';

interface LookFrame {
  photo_id: number;
  /** Edited by hand (the hero among them): it shows its own edit. */
  anchor: boolean;
  /** Its Basic sliders as they render, by engine name (exposure, temp…). */
  sliders: Record<string, number>;
  /** How well the hand-edited frames cover its scene, 0 to 1. */
  reach: number;
  /** Its scene is unlike every hand-edited frame: worth a look before accepting. */
  check: boolean;
  image?: string;
  error: string | null;
}

interface Preview {
  anchors: number[];
  frames: LookFrame[];
  ms: number;
}

/** Tiles are drawn this big (twice the strip's tile, for Retina). */
const EDGE = 224;
/** Frames read per call on the first pass, so the bar moves. */
const CHUNK = 6;

export interface ShootLook {
  frames: Record<string, LookFrame>;
  /** First pass: frames read so far, of how many; null once every frame has been read. */
  reading: { done: number; total: number; eta: number | null } | null;
  /** The last redraw of the whole strip, in ms. */
  ms: number | null;
  anchors: string[];
  error: string | null;
}

/** The Shoot `ids` as it will look with hero `heroId` edited as `recipe`. */
export function useShootLook(heroId: string, recipe: Recipe, ids: string[], on: boolean): ShootLook {
  const [out, setOut] = useState<ShootLook>({ frames: {}, reading: null, ms: null, anchors: [], error: null });
  const json = JSON.stringify(recipe);
  const key = ids.join(',');
  // What each tile shows now (the Recipe it was drawn for), and the newest Recipe and frames
  // asked for. One call runs at a time; it picks up whatever is newest when it ends.
  const s = useRef({ running: false, mounted: true, want: json, hero: heroId, ids, drawn: new Map<string, string>(), started: 0, read: 0 });

  useEffect(() => {
    const st = s.current;
    st.mounted = true;
    return () => {
      st.mounted = false;
    };
  }, []);

  useEffect(() => {
    if (!on || !LIVE || !ids.length) return;
    const st = s.current;
    if (st.hero !== heroId) {
      st.hero = heroId;
      st.drawn.clear();
      st.started = 0;
      st.read = 0;
      setOut({ frames: {}, reading: null, ms: null, anchors: [], error: null });
    }
    st.want = json;
    st.ids = ids;
    const pump = async () => {
      if (st.running || !st.mounted) return;
      const [want, hero, all] = [st.want, st.hero, st.ids];
      const unread = all.filter((id) => !st.drawn.has(id));
      const stale = all.filter((id) => st.drawn.get(id) !== want);
      if (!stale.length) return;
      // Unread frames go a few at a time (each is a RAW decode the first time); after that the
      // whole strip redraws in one call.
      const batch = unread.length ? unread.slice(0, CHUNK) : stale;
      if (unread.length && !st.started) st.started = performance.now();
      st.running = true;
      let failed = false;
      try {
        const r = await call<Preview>('shoot_look_preview', { hero_id: Number(hero), recipe: JSON.parse(want), photo_ids: batch.map(Number), long_edge: EDGE });
        if (!st.mounted || st.hero !== hero) return;
        for (const id of batch) st.drawn.set(id, want);
        if (unread.length) st.read += batch.length;
        const left = st.ids.filter((id) => !st.drawn.has(id)).length;
        const rate = st.read ? (performance.now() - st.started) / 1000 / st.read : null;
        setOut((o) => ({
          frames: { ...o.frames, ...Object.fromEntries(r.frames.map((f) => [String(f.photo_id), f])) },
          reading: left ? { done: st.ids.length - left, total: st.ids.length, eta: rate != null ? rate * left : null } : null,
          ms: unread.length ? o.ms : r.ms,
          anchors: r.anchors.map(String),
          error: null,
        }));
      } catch (e) {
        failed = true;
        if (st.mounted) setOut((o) => ({ ...o, error: String(e).replace(/^Error: /, '') }));
      } finally {
        st.running = false;
      }
      // A failed call waits for the next drag; otherwise on to whatever is newest.
      if (!failed) void pump();
    };
    void pump();
    // `key` stands for `ids`, compared by content.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [heroId, json, key, on]);

  return out;
}
