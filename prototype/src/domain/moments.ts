// Cull mode walks a Shoot moment by moment (GLOSSARY: Moment). A moment is one Stack, best
// frame first, or a short run of single frames between Stacks, so every Photo comes up once
// in the same side-by-side view (Greg, 2026-11-01: "Every photo").

export interface MomentPhoto {
  id: string;
  stackId?: string;
  stackRank?: number;
}

export interface Moment {
  /** The Stack's id, or `run:<first photo id>` for single frames. */
  id: string;
  kind: 'stack' | 'singles';
  photoIds: string[];
}

/** The most single frames shown together: about what fits side by side at a useful size. */
export const RUN = 6;

/** Moments in capture order. `photos` come in capture order; a Stack takes the place of its
 *  first frame. A run of singles longer than `run` splits into even parts. */
export function moments(photos: MomentPhoto[], run = RUN): Moment[] {
  const out: Moment[] = [];
  const stacks = new Map<string, MomentPhoto[]>();
  for (const p of photos) if (p.stackId) stacks.set(p.stackId, [...(stacks.get(p.stackId) ?? []), p]);
  let singles: string[] = [];
  const flush = () => {
    const parts = Math.ceil(singles.length / run);
    for (let i = 0; i < parts; i++) {
      const from = Math.round((i * singles.length) / parts);
      const to = Math.round(((i + 1) * singles.length) / parts);
      const ids = singles.slice(from, to);
      out.push({ id: `run:${ids[0]}`, kind: 'singles', photoIds: ids });
    }
    singles = [];
  };
  const placed = new Set<string>();
  for (const p of photos) {
    const frames = p.stackId ? stacks.get(p.stackId)! : null;
    if (!frames || frames.length < 2) {
      singles.push(p.id);
      continue;
    }
    if (placed.has(p.stackId!)) continue;
    placed.add(p.stackId!);
    flush();
    const ranked = frames.slice().sort((a, b) => (a.stackRank ?? 99) - (b.stackRank ?? 99));
    out.push({ id: p.stackId!, kind: 'stack', photoIds: ranked.map((f) => f.id) });
  }
  flush();
  return out;
}

/** What ↵ keeps: the frames you picked here; with none, in a Stack, the focused frame (a
 *  run of singles is not one moment, so it never keeps a frame you didn't pick). */
export function keptOnEnter(shown: Moment[], picked: Set<string>, focus: string | undefined): Set<string> {
  const ids = shown.flatMap((m) => m.photoIds);
  const yours = ids.filter((id) => picked.has(id));
  if (yours.length) return new Set(yours);
  const inStack = focus && shown.some((m) => m.kind === 'stack' && m.photoIds.includes(focus));
  return new Set(inStack ? [focus] : []);
}

/** The grid that shows `n` frames biggest in a W x H box: columns, rows and each frame's width.
 *  Every column count is tried; a frame is limited by its column's width or by its row's height
 *  less the caption (Greg, 2026-11-01: "use as much of the screen space as possible"). */
export function fitGrid(n: number, W: number, H: number, gap = 8, caption = 25, aspect = 1.5) {
  let best = { cols: 1, rows: n, width: 0 };
  for (let cols = 1; cols <= Math.max(1, n); cols++) {
    const rows = Math.ceil(n / cols);
    const byW = (W - gap * (cols - 1)) / cols;
    const byH = ((H - gap * (rows - 1)) / rows - caption) * aspect;
    const width = Math.floor(Math.max(0, Math.min(byW, byH)));
    if (width > best.width) best = { cols, rows, width };
  }
  return best;
}

/** The smallest short side, in CSS px, at which frames of a burst can still be compared. */
export const MIN_SHORT = 240;

/** How many frames fit on one page of the Survey at a size worth comparing (Greg, 2026-11-01:
 *  "limit how many we show ... based on screen size"). At least 2, so a page is still a choice. */
export function pageSize(n: number, W: number, H: number, aspect = 1.5, min = MIN_SHORT) {
  if (!W || !H) return n;
  let best = Math.min(2, n);
  for (let k = 1; k <= n; k++) {
    const { width } = fitGrid(k, W, H, 8, 25, aspect);
    if (Math.min(width, width / aspect) >= min) best = Math.max(best, k);
  }
  return best;
}

/** Split `ids` into even pages of at most `size`. */
export function pages<T>(ids: T[], size: number): T[][] {
  const n = Math.max(1, Math.ceil(ids.length / Math.max(1, size)));
  return Array.from({ length: n }, (_, i) => ids.slice(Math.round((i * ids.length) / n), Math.round(((i + 1) * ids.length) / n)));
}
