// Where the Library lives and what it takes on disk (engine/src/libraries.rs and the
// storage tools, ADR 0011). In the dummy prototype these return sample numbers.

import { call } from './backend';
import { LIVE } from './live';

export interface LibraryInfo {
  current: string | null;
  exists: boolean;
  recent: { path: string; exists: boolean }[];
  suggested: string;
}

export interface Usage {
  bytes: number;
  files: number;
}

export interface Storage {
  library: string;
  photos: number;
  originals_in_library: Usage;
  originals_elsewhere: Usage;
  missing: number;
  previews: Usage;
  cache: Usage;
  database: Usage;
  backups: Usage;
  total: Usage;
  free_bytes: number | null;
  settings: LibrarySettings;
}

export interface LibrarySettings {
  preview_long_edge: number;
  preview_quality: number;
}

const GB = 1e9;
const DEMO_INFO: LibraryInfo = {
  current: '/Users/greg/Pictures/Argus/library',
  exists: true,
  recent: [
    { path: '/Users/greg/Pictures/Argus/library', exists: true },
    { path: '/Volumes/Photo-SSD/Argus-Library', exists: false },
  ],
  suggested: '/Users/greg/Pictures/Argus/library',
};
let demoSettings: LibrarySettings = { preview_long_edge: 2048, preview_quality: 85 };
const demoStorage = (): Storage => ({
  library: DEMO_INFO.current!,
  photos: 1412,
  originals_in_library: { bytes: 58.4 * GB, files: 1180 },
  originals_elsewhere: { bytes: 11.2 * GB, files: 232 },
  missing: 232,
  previews: { bytes: 1.06 * GB, files: 1412 },
  cache: { bytes: 0.31 * GB, files: 96 },
  database: { bytes: 0.04 * GB, files: 2 },
  backups: { bytes: 0.21 * GB, files: 14 },
  total: { bytes: 60.02 * GB, files: 2704 },
  free_bytes: 212.6 * GB,
  settings: demoSettings,
});

export const libraryInfo = () => (LIVE ? call<LibraryInfo>('library') : Promise.resolve(DEMO_INFO));
export const createLibrary = (path: string) => call<LibraryInfo>('create_library', { path });
export const openLibrary = (path: string) => call<LibraryInfo>('open_library', { path });
export const forgetLibrary = (path: string) => (LIVE ? call<LibraryInfo>('forget_library', { path }) : Promise.resolve(DEMO_INFO));
export const moveLibrary = (to: string) => call<{ moved: { to: string; copied: boolean }; library: LibraryInfo }>('move_library', { to });
export const storage = () => (LIVE ? call<Storage>('library_storage') : Promise.resolve(demoStorage()));
export const clearCache = () => (LIVE ? call<Usage>('clear_cache') : Promise.resolve({ bytes: 0.31 * GB, files: 96 }));
export const relinkOriginals = (folder: string) => call<{ relinked: number; still_missing: number }>('relink_originals', { folder });
export const rebuildPreviews = () => call<{ rebuilt: number; offline: number; previews: Usage }>('rebuild_previews');
export function setLibrarySettings(s: Partial<LibrarySettings>): Promise<LibrarySettings> {
  if (!LIVE) {
    demoSettings = { ...demoSettings, ...s };
    return Promise.resolve(demoSettings);
  }
  return call<LibrarySettings>('library_settings', s);
}

/** Start over on the newly opened Library: its Shoots, its previews. */
export function reloadOnLibrary() {
  try {
    sessionStorage.removeItem('argus:shoot');
  } catch {
    // no storage
  }
  location.hash = '#/cull';
  location.reload();
}

export function bytes(n: number | null | undefined): string {
  if (n == null) return '–';
  if (n < 1e6) return `${Math.round(n / 1e3)} KB`;
  if (n < 1e9) return `${Math.round(n / 1e6)} MB`;
  if (n < 1e12) return `${(n / 1e9).toFixed(n < 1e10 ? 2 : 1)} GB`;
  return `${(n / 1e12).toFixed(2)} TB`;
}

/** A path for display, with the home folder as ~. */
export function tidy(path: string | null | undefined): string {
  if (!path) return '';
  return path.replace(/^\/(Users|home)\/[^/]+/, '~');
}
