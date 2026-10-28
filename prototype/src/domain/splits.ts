// A card split into Shoots at Import (engine `scan_folder` proposes the Splits): which
// Splits the person kept, and which photos they dragged to another Shoot.

import type { ScannedPhoto, ScanShoot } from '../data/live';

export interface ShootGroup {
  /** The proposal it starts from; stable while Splits are removed around it. */
  key: number;
  /** The engine's proposed name (`2026-10-25_Morning`). */
  proposed: string;
  photos: ScannedPhoto[];
  /** Minutes since the previous Shoot's last frame, as proposed; null for the first. */
  gapBefore: number | null;
}

/** The proposal a photo or a removed Split ends up in: the nearest kept one at or before it. */
function owner(i: number, removed: ReadonlySet<number>): number {
  while (i > 0 && removed.has(i)) i--;
  return i;
}

/**
 * The Shoots to import: the engine's proposals with the removed Splits merged into the Shoot
 * before them, and dragged photos in the Shoot they were dropped on. Photos stay in capture
 * order; a Shoot left empty is dropped.
 */
export function shootGroups(photos: ScannedPhoto[], proposals: ScanShoot[], removed: ReadonlySet<number>, moved: ReadonlyMap<string, number>): ShootGroup[] {
  const groups = new Map<number, ShootGroup>();
  proposals.forEach((s, i) => {
    if (owner(i, removed) === i) groups.set(i, { key: i, proposed: s.name, photos: [], gapBefore: s.gap_before_minutes });
  });
  proposals.forEach((s, i) => {
    for (const idx of s.photos) {
      const p = photos[idx];
      const to = moved.get(p.path);
      groups.get(owner(to ?? i, removed))?.photos.push(p);
    }
  });
  const order = (p: ScannedPhoto) => p.captured_at ?? '';
  return [...groups.values()]
    .map((g) => ({ ...g, photos: g.photos.sort((a, b) => order(a).localeCompare(order(b)) || a.file_name.localeCompare(b.file_name)) }))
    .filter((g) => g.photos.length > 0);
}

/** "3 h 40 min", "45 min", "2 days" for a gap between Shoots. */
export function gapText(minutes: number): string {
  if (minutes >= 48 * 60) return `${Math.round(minutes / 1440)} days`;
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  return h ? (m ? `${h} h ${m} min` : `${h} h`) : `${m} min`;
}

/** "9:12 AM to 10:48 AM" for a Shoot's capture times (local, as the camera wrote them). */
export function timeRange(photos: ScannedPhoto[]): string {
  const t = (s: string | null | undefined) => (s && s.length >= 16 ? new Date(s.slice(0, 19)) : null);
  const a = t(photos[0]?.captured_at);
  const b = t(photos[photos.length - 1]?.captured_at);
  if (!a || !b) return '';
  const time = (d: Date) => d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  // Across midnight (or a photo dragged in from another day) the days are named too.
  const oneDay = a.toDateString() === b.toDateString();
  const fmt = (d: Date) => (oneDay ? time(d) : `${d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}, ${time(d)}`);
  return a.getTime() === b.getTime() ? fmt(a) : `${fmt(a)} to ${fmt(b)}`;
}
