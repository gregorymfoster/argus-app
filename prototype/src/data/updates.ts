// App updates (app/src/updates.rs): a release downloads and installs in the background,
// then the UI offers "Restart to update", like Cursor and Claude. In a browser there is
// nothing to update; `?update=ready` (or `=downloading`, `=current`) fakes a state for
// screenshots.

import { useEffect, useState } from 'react';
import { IN_APP } from './backend';
import { listenApp, onAppEvent, shellInvoke } from './shell';

export interface UpdateStatus {
  state: 'off' | 'idle' | 'checking' | 'downloading' | 'ready' | 'current' | 'error';
  current: string;
  version: string | null;
  notes: string | null;
  done: number;
  total: number | null;
  checked_at: number | null;
  error: string | null;
}

const fake = new URLSearchParams(location.search).get('update') as UpdateStatus['state'] | null;
const FAKE: UpdateStatus | null = fake
  ? { state: fake, current: '0.1.1', version: fake === 'ready' || fake === 'downloading' ? '0.1.2' : null, notes: 'Design pass v2: Import, Cull grid and background work.\nRestart to update from the Status bar.', done: 18e6, total: 41e6, checked_at: Date.now() / 1000 - 120, error: null }
  : null;

let status: UpdateStatus | null = FAKE;
const subs = new Set<(s: UpdateStatus) => void>();
const publish = (s: UpdateStatus) => {
  status = s;
  subs.forEach((f) => f(s));
};

let started = false;
function start() {
  if (started || !IN_APP || FAKE) return;
  started = true;
  void listenApp<UpdateStatus>('update', publish)
    .then(async () => publish(await shellInvoke<UpdateStatus>('update_status')))
    .catch((e) => console.warn('update status:', e));
}

/** Where updates stand; null in a browser. */
export function useUpdate(): UpdateStatus | null {
  const [s, setS] = useState(status);
  useEffect(() => {
    start();
    subs.add(setS);
    if (status) setS(status);
    return () => void subs.delete(setS);
  }, []);
  return s;
}

/** "Check for Updates…": the outcome arrives through `useUpdate`. */
export async function checkForUpdates(): Promise<void> {
  if (FAKE) return publish({ ...FAKE, state: FAKE.state === 'ready' ? 'ready' : 'current', checked_at: Date.now() / 1000 });
  if (!IN_APP) return publish({ state: 'off', current: '', version: null, notes: null, done: 0, total: null, checked_at: null, error: null });
  // What the check finds arrives as `update` events; the reply is the state before it,
  // which only matters when this build can't update at all.
  const before = await shellInvoke<UpdateStatus>('check_for_updates');
  if (before.state === 'off' || before.state === 'ready') publish({ ...before });
}

/** "Restart to update": the new version is already installed. */
export async function restartToUpdate(): Promise<void> {
  if (!IN_APP) return;
  await shellInvoke('restart_to_update');
}

/** Run `f` when the app menu's Check for Updates… is chosen. */
export function onMenuCheck(f: () => void): () => void {
  return onAppEvent('check-updates', () => f());
}

/** "2 min ago" for a Unix time in seconds. */
export function ago(t: number | null): string {
  if (!t) return 'never';
  const s = Math.max(0, Date.now() / 1000 - t);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}
