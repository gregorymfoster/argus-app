// App-wide zoom for text density, like Cursor and browsers: ⌘+ / ⌘− / ⌘0 from the View
// menu (the app menu owns the keys and sends a Tauri event), and the ⌘K palette. In the
// app it is the webview's own zoom, so the whole layout reflows; in a browser the
// browser's ⌘+ already does this, and the palette falls back to CSS zoom. With a photo
// open in Loupe or Edit the keys zoom the photo instead (routeZoom); the palette still
// zooms the app.

import { IN_APP } from '../data/backend';

const KEY = 'argus-pref-zoom';
export const ZOOM_STEPS = [0.8, 0.9, 1, 1.1, 1.25, 1.5];

function saved(): number {
  try {
    const z = Number(localStorage.getItem(KEY));
    return ZOOM_STEPS.includes(z) ? z : 1;
  } catch {
    return 1;
  }
}

let level = saved();
const listeners = new Set<() => void>();

/** Follow the zoom as it changes (Settings › General shows it). */
export function onZoom(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** Set the zoom to one of ZOOM_STEPS. */
export function setZoom(z: number): Promise<void> {
  return apply(ZOOM_STEPS.includes(z) ? z : 1);
}

/** While a photo is open in Loupe or Edit, ⌘+ / ⌘− / ⌘0 zoom the photo, as in Lightroom. */
let photoZoom: ((step: 'in' | 'out' | 'reset') => void) | null = null;
export function routeZoom(to: typeof photoZoom) {
  photoZoom = to;
  return () => {
    if (photoZoom === to) photoZoom = null;
  };
}

async function apply(z: number) {
  level = z;
  listeners.forEach((l) => l());
  try {
    localStorage.setItem(KEY, String(z));
  } catch {
    /* private window: not remembered */
  }
  if (IN_APP) {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview');
    await getCurrentWebview().setZoom(z);
  } else {
    document.documentElement.style.zoom = z === 1 ? '' : String(z);
  }
}

export function zoomLevel(): number {
  return level;
}

/** Step the zoom: 'in', 'out' or 'reset' (Actual Size). */
export function zoom(step: 'in' | 'out' | 'reset'): Promise<void> {
  const i = ZOOM_STEPS.indexOf(level);
  const next = step === 'reset' ? 1 : ZOOM_STEPS[Math.max(0, Math.min(ZOOM_STEPS.length - 1, i + (step === 'in' ? 1 : -1)))];
  return apply(next);
}

/** Restore the remembered zoom and follow the View menu. Call once at start. */
export function initZoom() {
  if (level !== 1) void apply(level);
  const menu = (step: 'in' | 'out' | 'reset') => (photoZoom ? photoZoom(step) : void zoom(step));
  // The same path as a window event, so headless checks (live.mjs) can drive View › Zoom.
  window.addEventListener('argus:zoom', (e) => menu((e as CustomEvent<'in' | 'out' | 'reset'>).detail));
  if (IN_APP) {
    void import('@tauri-apps/api/event').then(({ listen }) => listen<'in' | 'out' | 'reset'>('zoom', (e) => menu(e.payload)));
  }
}
