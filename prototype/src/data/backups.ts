// Backup places: copies of the Library on another disk or in the Google Drive or iCloud
// Drive folder (engine/src/library/places.rs, the backup tools, ADR 0017). In the dummy
// prototype these return sample places.

import { call } from './backend';
import { LIVE } from './live';

export type Copies = 'edits' | 'originals' | 'everything';
export type When = 'after_import' | 'daily' | 'on_connect' | 'manual';
export type PlaceKind = 'google_drive' | 'icloud' | 'dropbox' | 'one_drive' | 'external_disk' | 'network_disk' | 'folder';

export interface BackupRun {
  at: string;
  why: string;
  files: number;
  bytes: number;
  secs: number;
  check: string;
  ok: boolean;
}

export interface Place {
  id: string;
  name: string;
  folder: string;
  copies: Copies;
  include_elsewhere: boolean;
  when: When;
  keep_snapshots: number;
  verify_monthly: boolean;
  copy_dir: string;
  kind: PlaceKind;
  available: boolean;
  free_bytes: number | null;
  last_ok: string | null;
  last_verified: string | null;
  last_error: string | null;
  bytes: number;
  files: number;
  history: BackupRun[];
  photos_in: number;
  waiting: number;
  status: 'up to date' | 'backing up' | 'waiting' | 'not connected' | 'failed' | 'never run';
}

export interface Places {
  places: Place[];
  photos: number;
  two_copies: number;
  one_copy: number;
}

export interface Found {
  name: string;
  folder: string;
  kind: PlaceKind;
  free_bytes: number | null;
  in_use: boolean;
  note: string | null;
}

export interface Estimate {
  files: number;
  bytes: number;
  free_bytes: number | null;
  secs: number;
  fits: boolean;
}

const GB = 1e9;
const hist = (at: string, why: string, files: number, bytes: number, secs: number, check = 'verified'): BackupRun => ({ at, why, files, bytes, secs, check, ok: true });
let demo: Place[] = [
  {
    id: 'samsung-t7', name: 'Samsung T7', folder: '/Volumes/Samsung-T7', copies: 'originals', include_elsewhere: false, when: 'after_import', keep_snapshots: 30, verify_monthly: true,
    copy_dir: '/Volumes/Samsung-T7/Argus-Backup/library', kind: 'external_disk', available: true, free_bytes: 1070 * GB,
    last_ok: '2026-11-02T16:42:00Z', last_verified: '2026-10-30T06:00:00Z', last_error: null, bytes: 59.6 * GB, files: 1181,
    history: [
      hist('2026-11-02T16:42:00Z', 'Import · 2026-11-02_Harbour-Walk', 212, 9.8 * GB, 240),
      hist('2026-11-02T02:10:00Z', 'Finish Shoot · 2026-11-01_Halloween', 277, 12.4 * GB, 300),
      hist('2026-10-30T06:00:00Z', 'Monthly check', 1181, 0, 6720, 'all match'),
    ],
    photos_in: 1180, waiting: 0, status: 'up to date',
  },
  {
    id: 'google-drive', name: 'Google Drive', folder: '/Users/greg/Library/CloudStorage/GoogleDrive-greg@example.com/My Drive', copies: 'originals', include_elsewhere: false, when: 'daily', keep_snapshots: 30, verify_monthly: true,
    copy_dir: '/Users/greg/Library/CloudStorage/GoogleDrive-greg@example.com/My Drive/Argus-Backup/library', kind: 'google_drive', available: true, free_bytes: 1400 * GB,
    last_ok: '2026-11-02T16:58:00Z', last_verified: null, last_error: null, bytes: 59.6 * GB, files: 1181,
    history: [hist('2026-11-02T16:58:00Z', 'Scheduled', 212, 9.8 * GB, 95)],
    photos_in: 1180, waiting: 0, status: 'up to date',
  },
  {
    id: 'gf-storage', name: 'gf-storage', folder: '/Volumes/gf-storage', copies: 'everything', include_elsewhere: true, when: 'on_connect', keep_snapshots: 30, verify_monthly: true,
    copy_dir: '/Volumes/gf-storage/Argus-Backup/library', kind: 'network_disk', available: false, free_bytes: null,
    last_ok: '2026-10-30T22:00:00Z', last_verified: null, last_error: null, bytes: 50.1 * GB, files: 2300,
    history: [hist('2026-10-30T22:00:00Z', 'Scheduled', 830, 31 * GB, 1900)],
    photos_in: 1080, waiting: 332, status: 'not connected',
  },
];
const demoFound: Found[] = [
  { name: 'Google Drive (greg@example.com)', folder: '/Users/greg/Library/CloudStorage/GoogleDrive-greg@example.com/My Drive', kind: 'google_drive', free_bytes: 1400 * GB, in_use: true, note: 'Set the Google Drive app to Mirror files to keep a copy on this Mac too, or Stream files to save space here.' },
  { name: 'iCloud Drive', folder: '/Users/greg/Library/Mobile Documents/com~apple~CloudDocs', kind: 'icloud', free_bytes: 610 * GB, in_use: false, note: 'Turn off Optimize Mac Storage, or iCloud may remove the local copies.' },
  { name: 'Samsung T7', folder: '/Volumes/Samsung-T7', kind: 'external_disk', free_bytes: 1070 * GB, in_use: true, note: null },
];

const demoPlaces = (): Places => ({ places: demo, photos: 1412, two_copies: 1180, one_copy: 1180 });

export const backupPlaces = () => (LIVE ? call<Places>('backup_places') : Promise.resolve(demoPlaces()));
export const findBackupPlaces = () => (LIVE ? call<Found[]>('find_backup_places') : Promise.resolve(demoFound));
export const estimateBackup = (folder: string, copies: Copies, include_elsewhere: boolean) =>
  LIVE ? call<Estimate>('estimate_backup', { folder, copies, include_elsewhere }) : Promise.resolve({ files: 1413, bytes: 59.6 * GB, free_bytes: 610 * GB, secs: 150, fits: true });

export function addBackupPlace(a: { folder: string; name?: string; copies: Copies; when: When; include_elsewhere: boolean; run: boolean }) {
  if (LIVE) return call<{ place: Place }>('add_backup_place', a);
  const p: Place = { ...demo[0], id: a.name?.toLowerCase().replace(/\W+/g, '-') ?? 'new', name: a.name ?? 'New', folder: a.folder, copies: a.copies, when: a.when, include_elsewhere: a.include_elsewhere, history: [], last_ok: null, photos_in: 0, waiting: 1412, status: 'never run', bytes: 0, files: 0 };
  demo = [...demo, p];
  return Promise.resolve({ place: p });
}

export function updateBackupPlace(id: string, change: Partial<Pick<Place, 'name' | 'copies' | 'when' | 'include_elsewhere' | 'keep_snapshots' | 'verify_monthly'>>) {
  if (LIVE) return call<Place>('update_backup_place', { id, ...change });
  demo = demo.map((p) => (p.id === id ? { ...p, ...change } : p));
  return Promise.resolve(demo.find((p) => p.id === id)!);
}

export function removeBackupPlace(id: string) {
  if (LIVE) return call('remove_backup_place', { id });
  demo = demo.filter((p) => p.id !== id);
  return Promise.resolve(null);
}

export const runBackup = (a: { id?: string; verify?: boolean; why?: string }) => (LIVE ? call('run_backup', a) : Promise.resolve([]));

export const KIND_LABEL: Record<PlaceKind, string> = {
  google_drive: 'Google Drive',
  icloud: 'iCloud Drive',
  dropbox: 'Dropbox',
  one_drive: 'OneDrive',
  external_disk: 'External disk',
  network_disk: 'Network disk',
  folder: 'Folder',
};

export const COPIES: { value: Copies; label: string; note: string }[] = [
  { value: 'edits', label: 'Edits only', note: 'The Library database: every Verdict, Recipe and Look. Small.' },
  { value: 'originals', label: 'Originals and edits', note: "Everything you can't rebuild. Recommended." },
  { value: 'everything', label: 'Everything', note: 'Previews too, so the copy opens instantly on another Mac.' },
];

export const WHEN: { value: When; label: string }[] = [
  { value: 'after_import', label: 'After each Import' },
  { value: 'daily', label: 'Daily' },
  { value: 'on_connect', label: 'When connected' },
  { value: 'manual', label: 'By hand' },
];

/** "11:42 AM", "Yesterday 9:10 PM", "Oct 30", in the Mac's own time zone. */
export function when(iso: string | null): string {
  if (!iso) return 'never';
  const d = new Date(iso);
  const now = new Date();
  const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const days = Math.floor((new Date(now.toDateString()).getTime() - new Date(d.toDateString()).getTime()) / 864e5);
  if (days === 0) return `Today ${time}`;
  if (days === 1) return `Yesterday ${time}`;
  return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
}
