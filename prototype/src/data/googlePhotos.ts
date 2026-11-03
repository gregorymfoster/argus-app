// Google Photos sign-in, as Settings › Export and the Export dialog show it. The engine
// keeps the person's own OAuth client and the sign-in on this Mac (engine/src/google_photos.rs);
// this file only asks it how things stand, every 1.5 s while Google's page is open in the
// browser. With dummy data it pretends: Sign in connects after two seconds.

import { useEffect, useSyncExternalStore } from 'react';
import { LIVE } from './live';
import { call } from './backend';

export interface GoogleStatus {
  /** The person's OAuth client is saved. */
  client: boolean;
  client_id?: string | null;
  connected: boolean;
  /** Google's page is open in the browser, waiting for the person. */
  waiting: boolean;
  waited_s?: number | null;
  auth_url?: string | null;
  /** How the last sign-in failed. */
  error?: string | null;
}

const demo = (): GoogleStatus => {
  const q = typeof location !== 'undefined' ? new URLSearchParams(location.search).get('gphotos') : null;
  if (q === 'connected') return { client: true, connected: true, waiting: false };
  if (q === 'client') return { client: true, connected: false, waiting: false };
  if (q === 'waiting') return { client: true, connected: false, waiting: true, waited_s: 12 };
  return { client: false, connected: false, waiting: false };
};

let status: GoogleStatus | null = LIVE ? null : demo();
const subs = new Set<() => void>();
const emit = () => subs.forEach((f) => f());
let timer: ReturnType<typeof setTimeout> | null = null;

function set(s: GoogleStatus) {
  status = s;
  emit();
  if (timer) clearTimeout(timer);
  timer = s.waiting && LIVE ? setTimeout(() => void refresh(), 1500) : null;
}

export async function refresh() {
  if (!LIVE) return;
  try {
    set(await call<GoogleStatus>('google_photos_status'));
  } catch {
    // The engine is away; the next look asks again.
  }
}

export function useGooglePhotos(): GoogleStatus | null {
  useEffect(() => {
    if (LIVE && !status) void refresh();
  }, []);
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => status,
  );
}

/** Save the person's Desktop OAuth client (ID and secret from Google Cloud). */
export async function saveClient(client_id: string, client_secret: string) {
  if (!LIVE) return set({ client: true, client_id, connected: false, waiting: false });
  set(await call<GoogleStatus>('google_photos_client', { client_id, client_secret }));
}

/** Open Google's consent page in the browser and wait for the person there. */
export async function connect() {
  if (!LIVE) {
    set({ ...(status ?? demo()), waiting: true, waited_s: 0, error: null });
    setTimeout(() => set({ client: true, connected: true, waiting: false }), 2000);
    return;
  }
  await call('google_photos_connect', { open: true });
  await refresh();
}

export async function disconnect() {
  if (!LIVE) return set({ client: true, connected: false, waiting: false });
  set(await call<GoogleStatus>('google_photos_disconnect'));
}
