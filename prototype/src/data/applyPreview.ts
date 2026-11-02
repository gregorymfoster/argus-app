// The Apply popover's tiles for Paste, Sync and Auto (engine `apply_preview`): each Photo as
// the edit would leave it, nothing saved. Photos are drawn a few at a time so the popover fills
// in under a real progress bar; a call that runs long names the Photo it is on, and one that
// never comes back is left behind (its Photos marked not drawn) so the popover never sits
// silent (Greg, 2026-11-02).

import { useEffect, useRef, useState } from 'react';
import { call } from './backend';
import { LIVE } from './live';

export type Change = { kind: 'paste'; from: string; groups: string[] } | { kind: 'auto' } | { kind: 'recipes'; recipes: Record<string, unknown> } | { kind: 'sync_look'; from: string };

export interface EditFrame {
  photo_id: number;
  /** Its latest Version is the person's own. */
  hand: boolean;
  /** The AI Denoise Amount Auto sets on it (a noisy RAW); null when none. Tiles can't show it. */
  denoise?: number | null;
  image?: string;
  error: string | null;
}

export interface ApplyPreview {
  frames: Record<string, EditFrame>;
  /** Drawn so far of how many, with an ETA; null once every Photo is drawn. */
  reading: { done: number; total: number; eta: number | null } | null;
  /** A call running longer than SLOW_MS: its first Photo and how long so far. */
  slow: { id: string; seconds: number } | null;
  /** Photos left behind after GIVE_UP_MS. */
  gaveUp: string[];
  error: string | null;
}

/** Tiles are drawn this big (twice the tile, for Retina). */
const EDGE = 256;
/** Photos per call, so the bar moves. */
const CHUNK = 4;
const SLOW_MS = 8000;
// The engine gives up on a stuck Photo after a minute (apply_preview::STALL); this is the
// backstop if the call itself never comes back.
const GIVE_UP_MS = 90000;

/** The engine's ids are numbers. */
const wire = (c: Change) => (c.kind === 'paste' || c.kind === 'sync_look' ? { ...c, from: Number(c.from) } : c);

const EMPTY: ApplyPreview = { frames: {}, reading: null, slow: null, gaveUp: [], error: null };

/** `ids` as `change` would leave them, drawn while `on`. A new change draws them afresh. */
export function useApplyPreview(change: Change, ids: string[], on: boolean): ApplyPreview {
  const [out, setOut] = useState<ApplyPreview>(EMPTY);
  const want = JSON.stringify(change);
  const key = ids.join(',');
  const s = useRef({ mounted: true, running: false, want, ids, drawn: new Map<string, string>(), started: 0, read: 0 });

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
    if (st.want !== want) {
      st.want = want;
      st.started = 0;
      st.read = 0;
      setOut((o) => ({ ...o, error: null, gaveUp: [] }));
    }
    st.ids = ids;
    const pump = async () => {
      if (st.running || !st.mounted) return;
      const w = st.want;
      const stale = st.ids.filter((id) => st.drawn.get(id) !== w);
      if (!stale.length) return;
      const batch = stale.slice(0, CHUNK);
      if (!st.started) st.started = performance.now();
      st.running = true;
      const t0 = performance.now();
      const tick = setInterval(() => {
        const ms = performance.now() - t0;
        if (ms > SLOW_MS && st.mounted) setOut((o) => ({ ...o, slow: { id: batch[0], seconds: Math.round(ms / 1000) } }));
      }, 1000);
      let timer: ReturnType<typeof setTimeout> | undefined;
      let failed = false;
      try {
        const late = new Promise<'late'>((ok) => (timer = setTimeout(() => ok('late'), GIVE_UP_MS)));
        const got = await Promise.race([call<{ frames: EditFrame[] }>('apply_preview', { change: wire(JSON.parse(w)), photo_ids: batch.map(Number), long_edge: EDGE }), late]);
        if (!st.mounted) return;
        // A newer change asked meanwhile: drop this answer and draw that one.
        if (st.want === w) {
          for (const id of batch) st.drawn.set(id, w);
          st.read += batch.length;
          const left = st.ids.filter((id) => st.drawn.get(id) !== w).length;
          const rate = (performance.now() - st.started) / 1000 / st.read;
          const reading = left ? { done: st.ids.length - left, total: st.ids.length, eta: got === 'late' ? null : rate * left } : null;
          if (got === 'late') setOut((o) => ({ ...o, slow: null, reading, gaveUp: [...o.gaveUp, ...batch] }));
          else
            setOut((o) => ({
              ...o,
              frames: { ...o.frames, ...Object.fromEntries(got.frames.map((f) => [String(f.photo_id), f])) },
              reading,
              slow: null,
              error: null,
            }));
        }
      } catch (e) {
        failed = true;
        if (st.mounted) setOut((o) => ({ ...o, slow: null, reading: null, error: String(e).replace(/^Error: /, '') }));
      } finally {
        clearInterval(tick);
        clearTimeout(timer);
        st.running = false;
      }
      if (!failed) void pump();
    };
    const stale = ids.filter((id) => st.drawn.get(id) !== want).length;
    if (stale) setOut((o) => ({ ...o, reading: { done: ids.length - stale, total: ids.length, eta: null } }));
    void pump();
    // `key` stands for `ids`, compared by content.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [want, key, on]);

  return out;
}
