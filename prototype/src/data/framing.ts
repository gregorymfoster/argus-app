// Crop & straighten (Greg, 2026-11-01): the AI's straighten and crop for each frame of the
// Shoot (engine `framing_preview`, local models, no model call). Frames are read a few at a
// time so the popover fills in under a real progress bar; each is read once, then kept.
// Edit's Crop & straighten popover (ui/Framing.tsx) draws and applies it.

import { useEffect, useRef, useState } from 'react';
import { call } from './backend';
import { LIVE } from './live';
import type { Crop } from '../domain/types';

export interface FramingFrame {
  photo_id: number;
  /** What it would write; null when it's left as shot. */
  crop: Crop | null;
  /** "Straighten 1.8°", "Tighter". */
  reasons: string[];
  /** Why it's left as shot: "already cropped", "nothing to level or tighten". */
  kept: string | null;
  image?: string;
  error: string | null;
}

/** Tiles are drawn this big (twice the tile, for Retina). */
const EDGE = 320;
/** Frames read per call, so the bar moves. */
const CHUNK = 4;

export interface Framing {
  frames: Record<string, FramingFrame>;
  reading: { done: number; total: number; eta: number | null } | null;
  error: string | null;
}

/** The prototype's stand-in: every third frame a small straighten, every fifth a punch-in. */
function dummy(ids: string[], withCrop: boolean): Record<string, FramingFrame> {
  return Object.fromEntries(
    ids.map((id, i) => {
      const turn = i % 3 === 1 ? (i % 2 ? 1.4 : -1.1) : 0;
      const tight = withCrop && i % 5 === 2;
      const crop: Crop | null = turn || tight ? { angle: turn, x: tight ? 0.08 : 0, y: tight ? 0.06 : 0, w: tight ? 0.8 : 1, h: tight ? 0.8 : 1 } : null;
      const reasons = [...(turn ? [`Straighten ${Math.abs(turn).toFixed(1)}°`] : []), ...(tight ? ['Tighter'] : [])];
      return [id, { photo_id: Number(id), crop, reasons, kept: crop ? null : 'nothing to level or tighten', error: null }];
    }),
  );
}

/** Crop & straighten's proposals for `ids`, read while `on`; `withCrop` adds punch-ins. */
export function useFraming(ids: string[], withCrop: boolean, on: boolean): Framing {
  const [out, setOut] = useState<Framing>({ frames: {}, reading: null, error: null });
  const s = useRef({ mounted: true, running: false, read: new Set<string>(), started: 0, ids, withCrop });
  const key = ids.join(',');

  useEffect(() => {
    const st = s.current;
    st.mounted = true;
    return () => {
      st.mounted = false;
    };
  }, []);

  useEffect(() => {
    if (!on || !ids.length) return;
    if (!LIVE) {
      setOut({ frames: dummy(ids, withCrop), reading: null, error: null });
      return;
    }
    const st = s.current;
    if (st.withCrop !== withCrop) {
      // Another mode is another set of proposals: read them afresh (the Facts are kept, so
      // the second pass is quick).
      st.withCrop = withCrop;
      st.read.clear();
      st.started = 0;
      setOut({ frames: {}, reading: null, error: null });
    }
    st.ids = ids;
    const pump = async () => {
      if (st.running || !st.mounted) return;
      const unread = st.ids.filter((id) => !st.read.has(id));
      if (!unread.length) return;
      const batch = unread.slice(0, CHUNK);
      const mode = st.withCrop;
      if (!st.started) st.started = performance.now();
      st.running = true;
      let failed = false;
      try {
        const r = await call<{ frames: FramingFrame[] }>('framing_preview', { photo_ids: batch.map(Number), crop: mode, long_edge: EDGE });
        if (!st.mounted || st.withCrop !== mode) return;
        for (const id of batch) st.read.add(id);
        const left = st.ids.filter((id) => !st.read.has(id)).length;
        const done = st.ids.length - left;
        const rate = (performance.now() - st.started) / 1000 / Math.max(1, st.read.size);
        setOut((o) => ({
          frames: { ...o.frames, ...Object.fromEntries(r.frames.map((f) => [String(f.photo_id), f])) },
          reading: left ? { done, total: st.ids.length, eta: rate * left } : null,
          error: null,
        }));
      } catch (e) {
        failed = true;
        if (st.mounted) setOut((o) => ({ ...o, reading: null, error: String(e).replace(/^Error: /, '') }));
      } finally {
        st.running = false;
      }
      if (!failed) void pump();
    };
    const unread = ids.filter((id) => !st.read.has(id)).length;
    if (unread) setOut((o) => ({ ...o, reading: o.reading ?? { done: ids.length - unread, total: ids.length, eta: null } }));
    void pump();
    // `key` stands for `ids`, compared by content.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, withCrop, on]);

  return out;
}
