// The one seam between the UI and the Mac app's shell (app/src): window, menu, zoom and
// update calls. backend.ts is the seam to the engine; between them they are the only files
// that import @tauri-apps (`just check` greps for it). In a browser every call here is a
// no-op, so screens never ask which one they're on.

import { IN_APP } from './backend';

/** Resolves once `f` runs on each app-shell event `name`, with the stop function. Never in a browser. */
export async function listenApp<T = unknown>(name: string, f: (payload: T) => void): Promise<() => void> {
  if (!IN_APP) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<T>(name, (e) => f(e.payload));
}

/**
 * Run `f` on each app-shell event `name` (the app menu's items arrive this way); nothing in a
 * browser. Returns the stop function, safe to call before the listener is up.
 */
export function onAppEvent<T = unknown>(name: string, f: (payload: T) => void): () => void {
  if (!IN_APP) return () => {};
  let off = () => {};
  let gone = false;
  listenApp<T>(name, f)
    .then((u) => {
      if (gone) u();
      else off = u;
    })
    .catch((e) => console.warn(`listen ${name}:`, e));
  return () => {
    gone = true;
    off();
  };
}

/** A command of the app shell itself (updates), not the engine. Only inside the app. */
export async function shellInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

/** The app window's page zoom. Only inside the app. */
export async function setWebviewZoom(z: number): Promise<void> {
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  await getCurrentWebview().setZoom(z);
}

/** Drag the window from a title-bar-like spot, or toggle its size on a double click. Only inside the app. */
export function dragWindow(doubleClick: boolean): void {
  void import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
    const w = getCurrentWindow();
    return doubleClick ? w.toggleMaximize() : w.startDragging();
  });
}
