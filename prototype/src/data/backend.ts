// The one seam between the UI and the engine. Every engine call is a named command with
// JSON arguments (engine/src/commands.rs), carried by whichever transport is present:
//
// - Tauri: inside the desktop app, `invoke('call', ...)`, previews over `argus://`
// - HTTP:  in a browser with `?library=live`, `argus serve` behind Vite's /api proxy
//
// Screens never know which one they're on.

import { invoke, isTauri } from '@tauri-apps/api/core';

export const IN_APP = isTauri();

// A call that never answers must never be silent (Greg, 2026-11-02, after the Apply look
// popover waited forever on the engine and showed nothing). Every call has a budget: past
// `slow` the person sees "still working" with the time so far (ui/SlowCalls.tsx), past `stall`
// it also logs a console error, which fails every scripted check (shots, live, explore,
// mac-e2e), so a hang shows up in a test before it reaches Greg. Commands that legitimately
// run long get a bigger budget here; anything longer still belongs in the Queue.
const LONG = new Set(['agent', 'cull_shoot', 'export_photos', 'analyze_photo', 'scan_folder', 'rebuild_previews', 'move_library', 'suggest_edits', 'rank_presets', 'framing_preview', 'shoot_look_preview', 'export_usage_bench', 'relink_originals', 'set_model', 'models', 'view_depth', 'meaning_model', 'estimate_import', 'find_cards']);
export function budget(command: string): { slow: number; stall: number } {
  return LONG.has(command) ? { slow: 30_000, stall: 600_000 } : { slow: 10_000, stall: 90_000 };
}

/** What a call that is taking long sends on `window` as `argus:slow-call`. */
export interface SlowCall {
  id: number;
  command: string;
  /** Milliseconds so far; `done` once it answered or failed. */
  ms: number;
  stalled: boolean;
  done: boolean;
}
let nextCall = 0;

function watch<T>(command: string, run: Promise<T>): Promise<T> {
  if (typeof window === 'undefined') return run;
  const id = ++nextCall;
  const start = performance.now();
  const { slow, stall } = budget(command);
  let said = false;
  let stalled = false;
  const tell = (done: boolean) => window.dispatchEvent(new CustomEvent<SlowCall>('argus:slow-call', { detail: { id, command, ms: performance.now() - start, stalled, done } }));
  const tick = setInterval(() => {
    const ms = performance.now() - start;
    if (ms < slow) return;
    if (ms >= stall && !stalled) {
      stalled = true;
      console.error(`[stall] engine call ${command} has had no answer for ${Math.round(ms / 1000)} s`);
    }
    said = true;
    tell(false);
  }, 1000);
  const end = () => {
    clearInterval(tick);
    if (said) tell(true);
  };
  run.then(end, end);
  return run;
}

/** Run an engine command, e.g. `call('photos', { shoot: 1 })`. Rejects with the engine's message. */
export async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return watch(command, send<T>(command, args));
}

async function send<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (IN_APP) {
    try {
      return await invoke<T>('call', { command, args });
    } catch (e) {
      throw new Error(String(e));
    }
  }
  const r = await fetch(`/api/${command}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(args) });
  const body = await r.json();
  if (!r.ok) throw new Error(body.error ?? r.statusText);
  return body as T;
}

/** URL for a Library-relative file such as a Photo's `preview`. */
export function libraryUrl(rel: string): string {
  // Tauri's custom protocols are argus://localhost/ on macOS and Linux, http://argus.localhost/ on Windows.
  if (IN_APP) return `${navigator.userAgent.includes('Windows') ? 'http://argus.localhost' : 'argus://localhost'}/${rel.replace(/^\//, '')}`;
  return rel.startsWith('/') ? rel : `/${rel}`;
}

/** Ask for a folder (to import by default). In a browser there is no picker, so it asks for a path. */
export async function pickFolder(title = 'Import a Shoot', defaultPath?: string): Promise<string | null> {
  if (IN_APP) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: false, title, defaultPath });
    return typeof picked === 'string' ? picked : null;
  }
  return window.prompt(`${title}: a folder path on the machine running argus serve`, defaultPath ?? '');
}
