// Greg's glass art for empty and finished states (2026-10-27), from the shared brand folder.
import tray from '../assets/brand/empty-library.webp';
import culled from '../assets/brand/cull-done.webp';
import done from '../assets/brand/import-done.webp';

/** `tray`: nothing in the Library yet. `culled`: nothing left to cull. `done`: an Import or Export finished. */
export type EmptyArt = 'tray' | 'culled' | 'done';
export const EMPTY_ART: Record<EmptyArt, string> = { tray, culled, done };
