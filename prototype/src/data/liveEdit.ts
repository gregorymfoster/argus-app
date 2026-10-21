// Live editing: the engine renders the Photo from its original with the Recipe being
// edited (render_photo). Saving is the store's job: each change is an engine batch
// (saveRecipe in live.ts), so ⌘Z is `undo_batch`.

import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { call } from './backend';
import { photoFacts } from './facts';
import type { LocalAdjust, MaskRef, Recipe } from '../domain/types';

/** Long edge of edit renders; the engine caps it at 1536. */
const EDGE = 1536;

interface Rendered {
  images: string[];
}

/** The Photo's saved Recipe, or null when it was never edited. */
export async function loadRecipe(photoId: string): Promise<Recipe | null> {
  const r = await call<{ recipe: Recipe } | null>('read_recipe', { photo_id: Number(photoId) });
  return r?.recipe ?? null;
}

/** Where the engine would copy from for a Spot at x, y, r on `recipe` (`pick_spot_source`). */
export async function pickSpotSource(photoId: string, recipe: Recipe, x: number, y: number, r: number, avoid: { x: number; y: number }[] = []): Promise<{ x: number; y: number }> {
  const out = await call<{ from: { x: number; y: number } }>('pick_spot_source', { photo_id: Number(photoId), recipe, x, y, r, avoid });
  return out.from;
}

// What a render shows can change without the Recipe changing: AI Denoise's base lands
// minutes after the Amount was set. Bumping this stamp re-renders the open Photo.
let baseStamp = 0;
const baseListeners = new Set<() => void>();

/** A Photo's AI Denoise base just finished: render it again. */
export function baseChanged() {
  baseStamp++;
  for (const l of baseListeners) l();
}

function useBaseStamp() {
  return useSyncExternalStore(
    (l) => {
      baseListeners.add(l);
      return () => baseListeners.delete(l);
    },
    () => baseStamp,
  );
}

/**
 * Render `recipe` whenever it changes, latest wins: at most one render runs at a time, and
 * slider drags in between collapse into the newest Recipe. Returns the newest render as a data URL (null until the first one lands) and
 * whether a render is in flight.
 */
export function useLiveRender(photoId: string, recipe: Recipe | null, enabled: boolean) {
  const [url, setUrl] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const state = useRef({ running: false, next: null as Recipe | null });
  // Compared by content: the store hands out a fresh empty Recipe on every render.
  const json = recipe ? JSON.stringify(recipe) : null;
  const stamp = useBaseStamp();

  useEffect(() => {
    if (!enabled || !json) return;
    const recipe = JSON.parse(json) as Recipe;
    const s = state.current;
    s.next = recipe;
    const pump = async () => {
      if (s.running || !s.next) return;
      const r = s.next;
      s.next = null;
      s.running = true;
      setBusy(true);
      try {
        const out = await call<Rendered>('render_photo', { photo_id: Number(photoId), recipe: r, long_edge: EDGE });
        setUrl(out.images[0] ?? null);
      } catch (e) {
        console.error('render failed', e);
      } finally {
        s.running = false;
        setBusy(Boolean(s.next));
        void pump();
      }
    };
    void pump();
  }, [photoId, json, enabled, stamp]);

  return { url, busy };
}

/** Long edge of the Mask images the overlay tints; masks are soft, so this is plenty. */
const MASK_EDGE = 768;

/** A greyscale JPEG (white = in the Mask) as a white PNG whose alpha is the Mask. */
async function toAlpha(jpeg: string): Promise<string> {
  const img = new Image();
  img.src = jpeg;
  await img.decode();
  const c = document.createElement('canvas');
  c.width = img.naturalWidth;
  c.height = img.naturalHeight;
  const g = c.getContext('2d')!;
  g.drawImage(img, 0, 0);
  const d = g.getImageData(0, 0, c.width, c.height);
  for (let i = 0; i < d.data.length; i += 4) {
    d.data[i + 3] = d.data[i];
    d.data[i] = d.data[i + 1] = d.data[i + 2] = 255;
  }
  g.putImageData(d, 0, 0);
  return c.toDataURL('image/png');
}

const maskCache = new Map<string, Promise<string>>();

/** The engine's own Mask (view_mask weights) for one Photo, cached by its description. */
function engineMask(photoId: string, mask: MaskRef): Promise<string> {
  const key = `${photoId}:${JSON.stringify(mask)}`;
  let p = maskCache.get(key);
  if (!p) {
    p = call<Rendered>('view_mask', { photo_id: Number(photoId), mask, long_edge: MASK_EDGE, as: 'weights' }).then((r) => toAlpha(r.images[0]));
    p.catch(() => maskCache.delete(key));
    maskCache.set(key, p);
  }
  return p;
}

/**
 * The real Masks of `locals`, as alpha images keyed by local id, for tinting over the
 * engine's render. Only fetched while `enabled` (Masks shown); a Mask that fails is left out.
 */
export function useEngineMasks(photoId: string, locals: LocalAdjust[], enabled: boolean) {
  const [masks, setMasks] = useState<Record<string, string>>({});
  const json = JSON.stringify(locals.map((l) => [l.id, l.mask]));
  useEffect(() => {
    if (!enabled) return;
    let on = true;
    const list = JSON.parse(json) as [string, MaskRef][];
    Promise.all(list.map(([id, m]) => engineMask(photoId, m).then((url) => [id, url] as const, () => null))).then((got) => {
      if (on) setMasks(Object.fromEntries(got.filter((g) => g !== null)));
    });
    return () => {
      on = false;
    };
  }, [photoId, json, enabled]);
  return masks;
}

export interface FoundFace {
  /** Box as fractions of the image: left, top, width, height. */
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The faces the engine's local models find in a Photo, left to right (`analyze_photo`, cached). */
export function findFaces(photoId: string): Promise<FoundFace[]> {
  return photoFacts(photoId).then((r) => r.faces);
}

/** Sizes a zoomed view asks for, so panning and small zoom changes reuse one render. */
const DETAIL_EDGES = [3072, 6144];
const detailCache = new Map<string, Promise<string>>();

/**
 * The sharp render for a zoomed Loupe or Edit view: once the view needs more pixels than
 * `have` (the long edge of what's on screen), render the Photo at that size, up to the
 * original's full size (`render_detail`), after the zoom settles. `recipe` null renders the
 * saved Recipe. Null while not needed, or while the render for the current Recipe is out.
 */
export function useDetailRender(photoId: string, recipe: Recipe | null, need: number, have: number, natural: number, enabled: boolean) {
  const [out, setOut] = useState<{ key: string; photoId: string; json: string; url: string } | null>(null);
  const json = recipe ? JSON.stringify(recipe) : '';
  const stamp = useBaseStamp();
  const edge = need > have * 1.05 ? Math.min(natural, DETAIL_EDGES.find((e) => e >= need) ?? natural) : 0;
  const key = edge > have ? `${photoId}|${edge}|${stamp}|${json}` : '';
  useEffect(() => {
    if (!enabled || !key) return;
    let live = true;
    const t = setTimeout(() => {
      let p = detailCache.get(key);
      if (!p) {
        const args: Record<string, unknown> = { photo_id: Number(photoId), long_edge: edge };
        if (recipe) args.recipe = JSON.parse(json);
        p = call<Rendered>('render_detail', args).then((r) => r.images[0]);
        p.catch(() => detailCache.delete(key));
        detailCache.set(key, p);
        // Full-size JPEGs are big: keep the last few.
        while (detailCache.size > 4) detailCache.delete(detailCache.keys().next().value!);
      }
      p.then((url) => live && setOut({ key, photoId, json, url })).catch((e) => console.error('render_detail failed', e));
    }, 220);
    return () => {
      live = false;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled]);
  // A bigger render than needed is still sharp; one for an older Recipe is not shown.
  const ok = out && key && out.json === json && out.photoId === photoId;
  return { url: ok ? out!.url : null, loading: Boolean(enabled && key && out?.key !== key) };
}
