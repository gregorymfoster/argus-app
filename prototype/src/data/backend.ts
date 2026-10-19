// The one seam between the UI and the engine. Every engine call is a named command with
// JSON arguments (engine/src/commands.rs), carried by whichever transport is present:
//
// - Tauri: inside the desktop app, `invoke('call', ...)`, previews over `argus://`
// - HTTP:  in a browser with `?library=live`, `argus serve` behind Vite's /api proxy
//
// Screens never know which one they're on.

import { invoke, isTauri } from '@tauri-apps/api/core';

export const IN_APP = isTauri();

/** Run an engine command, e.g. `call('photos', { shoot: 1 })`. Rejects with the engine's message. */
export async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
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
