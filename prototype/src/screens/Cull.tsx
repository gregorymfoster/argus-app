import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Award, Check, ChevronDown, Columns2, Copy, Download, Flag, Folder, Grid2x2, Grid3x3, Layers, SlidersHorizontal, Sparkles, Square, Star, Trash2, Wand2, X } from 'lucide-react';
import { toast } from 'sonner';
import { acceptMany, useShootTakes } from '../data/suggestions';
import { undoBatch } from '../data/liveAgent';
import { LIVE, importingBytes, inCollection, revealInFinder } from '../data/live';
import { call } from '../data/backend';
import { useAiStatus, type AiState } from '../data/aiStatus';
import { useEyeFlags, type EyeFlags } from '../data/facts';
import { EyeBadges } from '../ui/EyeBadges';
import { askRemove } from '../ui/Remove';
import { PHOTOS, PHOTO_BY_ID, SHOOT, STACK_BY_ID, stackOf } from '../data/dummy';
import type { Photo, Verdict } from '../domain/types';
import { recipeOf, starsOf, useLibrary, verdictOf } from '../store/library';
import { autoAction, runAuto } from '../data/auto';
import { usePref } from '../store/variants';
import { go } from '../router';
import { Agent } from '../agent/AgentPanel';
import { PhotoView } from '../ui/PhotoView';
import { eta } from '../ui/Progress';
import { FlagButtons, VerdictChip } from '../ui/Flags';
import { STAR_KEYS, Stars } from '../ui/Stars';
import { ViewSwitch } from '../ui/ViewBar';
import { SortMenu, toggleStars, useSort } from '../ui/Sort';
import { sortPhotos } from '../domain/sort';
import { aiName } from '../data/live';
import { ESC_OUTSIDE_DIALOGS, Kbd, MenuKeys, Seg, Tip, VERDICT_KEYS } from '../ui/kit';
import { askSettings, hasCopiedSettings, pasteClip, pasteSettings, useSettingsKeys } from '../ui/PasteSettings';
import { ALL_GROUPS } from '../domain/settings';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';

type Filter = 'all' | Verdict | 'best';

// Thumbnail width in CSS pixels, before tiles stretch to fill the row.
export const THUMB_MIN = 72;
export const THUMB_MAX = 420;
export const THUMB_DEFAULT = 132;
/** Matches the engine's grid thumbnails (`THUMB_EDGE` in engine/src/commands.rs). */
const THUMB_EDGE = 480;
const clampThumb = (n: number) => Math.max(THUMB_MIN, Math.min(THUMB_MAX, n));

interface TileProps {
  p: Photo;
  v: Verdict | null;
  you: boolean;
  stars: { stars: number; you: boolean };
  /** Rank in Best Photos, shown once the Shoot is culled. */
  best?: number;
  hero?: boolean;
  /** How many Suggestions are waiting for this Photo. */
  takes?: number;
  /** Whether the background AI is on this Photo now or has it queued. */
  ai?: AiState;
  /** Closed eyes or soft focus from the eyes model. */
  eyes?: EyeFlags | null;
  onStars: (n: number) => void;
  selected: boolean;
  current: boolean;
  highlighted: boolean;
  /** Frames in this Photo's Stack, when the tile stands for it (0 otherwise). */
  stackSize: number;
  expanded: boolean;
  /** Where the tile sits in an expanded Stack: its frame number, and whether it starts or ends the run. */
  stackPos?: number;
  stackFirst?: boolean;
  stackLast?: boolean;
  /** The frame a collapsed Stack shows (its kept Pick, else its best frame). */
  stackCover?: boolean;
  culling: boolean;
  onToggleStack: () => void;
  onClick: (e: React.MouseEvent) => void;
  onSet: (v: Verdict) => void;
  /** The right-click menu's actions on this Photo (and the selection), read when it opens. */
  menu: () => TileMenu;
  /** Small enough for the grid thumbnail (a tenth of the preview's pixels to decode). */
  small?: boolean;
}

interface TileMenu {
  /** How many other Photos are selected, for "to selection" items. */
  others: number;
  onAuto: () => void;
  onApplyToSelection: () => void;
  onPaste: () => void;
  onSync: () => void;
  onCompare: () => void;
  onAsk: () => void;
  onExport: () => void;
  onRemove: () => void;
  /** Its Stack's size and whether it is expanded, for the Stack items. */
  stack: { size: number; open: boolean; any: boolean };
  onToggleStack: () => void;
  onAllStacks: (open: boolean) => void;
  /** The Photo under the menu is in the selection, so selection items act on all of it. */
  selected: boolean;
}

// A grid tile re-renders only when what it shows changed (a flag used to re-render every
// tile on screen). Its handlers are ignored here: the screen routes them through its latest
// state, so a tile that skipped a render never acts on an old selection.
const sameTile = (a: TileProps, b: TileProps) =>
  (Object.keys({ ...a, ...b }) as (keyof TileProps)[]).every((k) =>
    typeof a[k] === 'function' ? typeof b[k] === 'function' : k === 'stars' ? a.stars.stars === b.stars.stars && a.stars.you === b.stars.you : Object.is(a[k], b[k]),
  );

const Tile = memo(function Tile({ p, v, you, stars, best, hero, takes, ai, eyes, onStars, selected, current, highlighted, stackSize, expanded, stackPos, stackFirst, stackLast, stackCover, culling, onToggleStack, onClick, onSet, menu, small }: TileProps) {
  const stk = stackSize > 1 ? ['stk', expanded ? 'stk-open' : 'stk-shut', stackCover && 'stk-cover', stackFirst && 'stk-first', stackLast && 'stk-last'] : [];
  const cls = ['tile', hero && 'hero', selected && 'sel', current && 'cur', highlighted && 'hl', v === 'reject' && 'faded', ...stk].filter(Boolean).join(' ');
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <figure className={cls} onClick={onClick} onDoubleClick={() => go({ name: 'loupe', id: p.id })} data-testid={`tile-${p.id}`} data-verdict={v ?? ''}>
          <div className="ph">
            <PhotoView photo={p} size={small ? 'tile' : 'thumb'} edits style={{ width: '100%', height: '100%', borderRadius: 0 }} />
            <VerdictChip v={v} you={you} />
            <div className="tile-tr">
              {stackSize > 1 && (
                <Tip label={expanded ? `Collapse Stack (frame ${stackPos} of ${stackSize}${stackCover ? ', the cover' : ''})` : `Expand Stack of ${stackSize}`} keys="S">
                  <button
                    className="badge stack-badge"
                    data-testid="stack-badge"
                    aria-label={expanded ? 'Collapse Stack' : `Expand Stack of ${stackSize}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      onToggleStack();
                    }}
                  >
                    {(!expanded || stackCover) && <Layers size={10} strokeWidth={2.2} />}
                    {expanded ? `${stackPos}/${stackSize}` : stackSize}
                  </button>
                </Tip>
              )}
              {best != null && hero === undefined && (
                <Tip label={`Number ${best} of the Shoot's Best Photos`} keys="B">
                  <span className="best-badge" aria-label={`Best Photos number ${best}`}>
                    <Award size={10} strokeWidth={2.2} />
                    {best}
                  </span>
                </Tip>
              )}
            </div>
            {hero !== undefined && best != null && <span className="rank">#{best}</span>}
            {!!takes && v !== 'reject' && (
              <Tip label={`${takes} Suggestion${takes > 1 ? 's' : ''} to review in Edit`} keys="A accepts the first">
                <span className="takes-badge" data-testid="takes-badge">
                  <Sparkles size={10} strokeWidth={2.2} />
                  {takes}
                </span>
              </Tip>
            )}
            {!takes && ai && (
              <Tip label={ai === 'working' ? 'The AI is on this photo now' : 'Waiting for the AI: Suggestions come when its turn comes'}>
                <span className={`ai-mark ${ai}`} data-testid="ai-mark" data-ai={ai} aria-label={ai === 'working' ? 'AI working' : 'Queued for AI'} />
              </Tip>
            )}
            <EyeBadges flags={eyes ?? null} />
            {culling && <div className="shimmer" />}
            {!culling && <FlagButtons v={v} onSet={onSet} />}
          </div>
          <figcaption title={p.reason || undefined}>
            {!culling && (stars.stars > 0 || stars.you) && <Stars n={stars.stars} you={stars.you} onSet={onStars} />}
            {v && p.reason ? p.reason : culling ? 'Scoring…' : p.file}
          </figcaption>
        </figure>
      </ContextMenuTrigger>
      <ContextMenuContent className="min-w-[280px]">
        <TileMenuItems p={p} stars={stars} onStars={onStars} onSet={onSet} menu={menu} />
      </ContextMenuContent>
    </ContextMenu>
  );
}, sameTile);

/** The right-click menu, built when it opens so it reflects the selection then. */
function TileMenuItems({ p, stars, onStars, onSet, menu: menuOf }: Pick<TileProps, 'p' | 'stars' | 'onStars' | 'onSet' | 'menu'>) {
  const menu = menuOf();
  return (
    <>
      <ContextMenuItem onSelect={() => onSet('pick')}>
        <Flag size={13} /> Pick <MenuKeys keys={['P']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => onSet('reject')}>
        <X size={13} /> Reject <MenuKeys keys={['X']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => onSet('unflagged')}>
        <span style={{ width: 13 }} /> Clear flag <MenuKeys keys={['U']} />
      </ContextMenuItem>
      <div className="menu-rate" role="group" aria-label="Rate">
        <Star size={13} />
        <span>Rate</span>
        <span className="grow" />
        <Stars n={stars.stars} you={stars.you} onSet={onStars} />
      </div>
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={menu.onAuto}>
        <Wand2 size={13} /> Auto <MenuKeys keys={['⇧', 'A']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => go({ name: 'edit', id: p.id })}>
        <SlidersHorizontal size={13} /> Edit <MenuKeys keys={['D']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={menu.onApplyToSelection} disabled={menu.others === 0}>
        <Copy size={13} /> Apply this edit to {menu.others ? `${menu.others} selected` : 'selection'} <MenuKeys keys={['⌘', '⇧', 'A']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => askSettings({ mode: 'copy', from: p.id, targets: [] })}>
        <span style={{ width: 13 }} /> Copy settings… <MenuKeys keys={['⌘', '⇧', 'C']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={menu.onPaste} disabled={!hasCopiedSettings()}>
        <span style={{ width: 13 }} /> Paste settings{menu.others > 0 && menu.selected ? ` to ${menu.others + 1} Photos` : ''} <MenuKeys keys={['⌘', '⇧', 'V']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={menu.onSync} disabled={menu.others === 0}>
        <span style={{ width: 13 }} /> Sync settings to {menu.others ? `${menu.others} selected` : 'selection'}… <MenuKeys keys={['⌘', '⇧', 'S']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={menu.onCompare} disabled={menu.others === 0 && !p.stackId}>
        <Columns2 size={13} /> {menu.others === 0 && p.stackId ? 'Compare the Stack' : 'Compare with selection'} <MenuKeys keys={['C']} />
      </ContextMenuItem>
      {menu.stack.any && (
        <>
          <ContextMenuItem onSelect={menu.onToggleStack} disabled={menu.stack.size < 2}>
            <Layers size={13} /> {menu.stack.size < 2 ? 'Not in a Stack' : menu.stack.open ? `Collapse Stack of ${menu.stack.size}` : `Expand Stack of ${menu.stack.size}`} <MenuKeys keys={['S']} />
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => menu.onAllStacks(true)}>
            <span style={{ width: 13 }} /> Expand all Stacks
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => menu.onAllStacks(false)}>
            <span style={{ width: 13 }} /> Collapse all Stacks
          </ContextMenuItem>
        </>
      )}
      <ContextMenuItem onSelect={menu.onAsk}>
        <Sparkles size={13} /> Ask Agent about this photo <MenuKeys keys={['⌘', 'L']} />
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem disabled={!LIVE} onSelect={() => revealInFinder({ photo_id: Number(p.id) }).catch((e) => toast(String(e).replace(/^Error: /, ''), { id: 'argus' }))}>
        <Folder size={13} /> Show original in Finder <MenuKeys keys={['⌘', '⇧', 'R']} />
      </ContextMenuItem>
      <ContextMenuItem onSelect={menu.onExport}>
        <Download size={13} /> {menu.others > 0 && menu.selected ? `Export ${menu.others + 1} Photos…` : 'Export…'}
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => void navigator.clipboard?.writeText(p.file).then(() => toast(`Copied ${p.file}`, { id: 'argus' }))}>
        <span style={{ width: 13 }} /> Copy file name
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={menu.onRemove} data-testid="tile-remove">
        <Trash2 size={13} /> {menu.others > 0 && menu.selected ? `Remove ${menu.others + 1} Photos from Library…` : 'Remove from Library…'} <MenuKeys keys={['⌫']} />
      </ContextMenuItem>
    </>
  );
}

/** The runs of an expanded Stack's frames in one grid row: the band behind them spans these columns. */
function stackRuns(row: Photo[], pos: Record<string, { first: boolean; last: boolean }>) {
  const runs: { stackId: string; from: number; to: number; first: boolean; last: boolean }[] = [];
  row.forEach((p, j) => {
    if (!p.stackId || !pos[p.id]) return;
    const r = runs[runs.length - 1];
    if (r && r.stackId === p.stackId && r.to === j - 1) {
      r.to = j;
      r.last = pos[p.id].last;
    } else runs.push({ stackId: p.stackId, from: j, to: j, first: pos[p.id].first, last: pos[p.id].last });
  });
  return runs;
}

export function CullScreen({ onExport, onCull }: { onExport: (ids?: string[]) => void; onCull: () => void }) {
  const { state, dispatch } = useLibrary();
  // Thumbnail size: pinch on the grid, the toolbar slider, or = and − as in Lightroom's
  // Grid (⌘+ / ⌘− zoom the whole app). Remembered for this viewer.
  const [thumb, setThumb] = usePref<string>('thumb', String(THUMB_DEFAULT));
  const [filter, setFilter] = useState<Filter>('all');
  const [minStars, setMinStars] = useState(0);
  // Lightroom has no AI filter; ours narrows to what the background AI has done or not yet.
  const [aiFilter, setAiFilter] = useState<'any' | 'ready' | 'waiting'>('any');
  const [sort, setSort] = useSort();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [cur, setCur] = useState(0);
  const [sel, setSel] = useState<Set<string>>(new Set());
  // Removed Photos leave the selection too.
  useEffect(() => setSel((cur) => (cur.size && [...cur].some((id) => !PHOTO_BY_ID[id]) ? new Set([...cur].filter((id) => PHOTO_BY_ID[id])) : cur)), [state.imported]);
  const gridRef = useRef<HTMLDivElement>(null);
  const all = PHOTOS.slice(0, state.imported);
  const culled = Object.keys(state.aiVerdicts).length > 0;

  // Best Photos: the cull's ranked short list, minus any the person has since rejected.
  const bestPhotos = useMemo(
    () => (culled ? all.filter((p) => p.best != null && !(state.overrides[p.id] === 'reject')).sort((a, b) => a.best! - b.best!) : []),
    [all, culled, state.overrides],
  );
  const takes = useShootTakes();
  const openTakes = (id: string) => (takes[id] ?? []).filter((t) => t.status === 'open').length;
  const aiStatus = useAiStatus();
  const eyesOf = useEyeFlags();
  const aiOf = (id: string) => aiStatus.of(id, openTakes(id) > 0);
  const aiOk = (p: Photo) => aiFilter === 'any' || (aiFilter === 'ready' ? openTakes(p.id) > 0 : aiOf(p.id) != null);
  // A collapsed Stack shows the frame that was kept (its best Pick), else its best frame.
  const cover = useMemo(() => {
    const out: Record<string, string> = {};
    const rank = (p: Photo) => (verdictOf(state, p.id) === 'pick' ? -100 : 0) + (p.stackRank ?? 99);
    for (const p of all) if (p.stackId && (!out[p.stackId] || rank(p) < rank(PHOTO_BY_ID[out[p.stackId]]))) out[p.stackId] = p.id;
    return out;
  }, [all, state]);
  const stackCount = Object.keys(cover).length;
  const visible = useMemo(() => {
    const facts = { stars: (id: string) => starsOf(state, id).stars, verdict: (id: string) => verdictOf(state, id) };
    if (filter === 'best') {
      const best = bestPhotos.filter((p) => starsOf(state, p.id).stars >= minStars && aiOk(p));
      // Best Photos stay in rank order unless another sort is chosen.
      return sort.key === 'capture' ? best : sortPhotos(best, sort, facts, false);
    }
    const shown = all.filter((p) => {
      if (minStars && starsOf(state, p.id).stars < minStars) return false;
      if (!aiOk(p)) return false;
      if (!minStars && p.stackId && cover[p.stackId] !== p.id && !expanded.has(p.stackId)) return false;
      return filter === 'all' || (verdictOf(state, p.id) ?? 'unflagged') === filter;
    });
    // The star filter shows every frame of a Stack, so they sort one by one then.
    return sortPhotos(shown, sort, facts, !minStars);
  }, [all, bestPhotos, cover, expanded, filter, minStars, state, aiFilter, takes, aiStatus, sort]);
  const counts = useMemo(() => {
    const c = { all: all.length, pick: 0, reject: 0, unflagged: 0, best: bestPhotos.length };
    all.forEach((p) => {
      c[verdictOf(state, p.id) ?? 'unflagged']++;
    });
    return c;
  }, [all, bestPhotos, state]);
  const atLeast = useMemo(() => [0, 1, 2, 3, 4, 5].map((n) => all.filter((p) => starsOf(state, p.id).stars >= n).length), [all, state]);

  const current = visible[Math.min(cur, visible.length - 1)];
  // Lightroom: flags apply to every selected photo, or the current one.
  const targets = sel.size ? [...sel] : current ? [current.id] : [];

  const setVerdict = (ids: string[], v: Verdict) => {
    if (ids.length === 1) dispatch({ type: 'verdict/set', id: ids[0], verdict: v, by: 'you' });
    else dispatch({ type: 'batch', label: `${ids.length} ${v === 'unflagged' ? 'unflagged' : `to ${v}`}`, actions: ids.map((id) => ({ type: 'verdict/set', id, verdict: v, by: 'you' })) });
  };
  // A accepts the first take on each target as one batch.
  const acceptFirst = (ids: string[]) => {
    const firsts = ids.map((id) => (takes[id] ?? []).find((t) => t.status === 'open')).filter((t) => !!t);
    if (!firsts.length) return dispatch({ type: 'toast', text: 'No Suggestions waiting on these Photos' });
    if (!LIVE) {
      return dispatch({ type: 'batch', label: `Suggestions on ${firsts.length} Photo${firsts.length > 1 ? 's' : ''}`, actions: firsts.map((t) => ({ type: 'recipe/set', id: t.photoId, recipe: t.recipe, label: `Suggestion: ${t.title}`, by: 'ai' })) });
    }
    acceptMany(firsts)
      .then(({ batch }) =>
        toast(`Accepted the first take on ${firsts.length} Photo${firsts.length > 1 ? 's' : ''}`, {
          id: 'argus',
          action: batch == null ? undefined : { label: 'Undo', onClick: () => void undoBatch(batch).then(() => window.dispatchEvent(new Event('argus:suggestions'))) },
        }),
      )
      .catch((e) => dispatch({ type: 'toast', text: `Accept failed: ${String(e).replace(/^Error: /, '')}` }));
  };
  const setStars = (ids: string[], n: number) => {
    if (ids.length === 1) dispatch({ type: 'stars/set', id: ids[0], stars: n, by: 'you' });
    else dispatch({ type: 'batch', label: `${ids.length} to ${n} stars`, actions: ids.map((id) => ({ type: 'stars/set', id, stars: n, by: 'you' })) });
  };
  const changeSort = (s: typeof sort) => {
    setSort(s);
    setCur(0);
    setSel(new Set());
  };
  // Collapsing lands on the cover, as in Lightroom; expanding stays on the frame you were on.
  const toggleStack = (id: string) => {
    const open = expanded.has(id);
    setFollow(open ? cover[id] : (visible[cur]?.id ?? cover[id]));
    setExpanded((x) => {
      const n = new Set(x);
      if (open) n.delete(id);
      else n.add(id);
      return n;
    });
  };
  const setAllStacks = (open: boolean) => {
    setFollow(visible[cur]?.stackId && !open ? cover[visible[cur].stackId!] : (visible[cur]?.id ?? null));
    setExpanded(open ? new Set(Object.keys(cover)) : new Set());
  };
  // Each expanded Stack's frames, numbered in the order they show, and where its run starts and ends.
  const stackPos = useMemo(() => {
    const out: Record<string, { n: number; first: boolean; last: boolean }> = {};
    const seen: Record<string, number> = {};
    visible.forEach((p, i) => {
      if (!p.stackId || !expanded.has(p.stackId) || minStars || filter === 'best') return;
      seen[p.stackId] = (seen[p.stackId] ?? 0) + 1;
      out[p.id] = { n: seen[p.stackId], first: visible[i - 1]?.stackId !== p.stackId, last: visible[i + 1]?.stackId !== p.stackId };
    });
    return out;
  }, [visible, expanded, minStars, filter]);
  const move = (d: number) => {
    setSel(new Set());
    setCur((s) => Math.max(0, Math.min(visible.length - 1, s + d)));
  };
  // After a flag or star, move on to the Photo that was next, wherever it lands: under a
  // flag or star filter the flagged Photo leaves the grid, and sorted by stars or flag it
  // moves, so stepping the cursor by one would skip a Photo or jump away.
  const [follow, setFollow] = useState<string | null>(null);
  const moveOn = () => {
    const next = visible[Math.min(cur, visible.length - 1) + 1];
    if (next) setFollow(next.id);
  };
  useLayoutEffect(() => {
    if (!follow) return;
    const i = visible.findIndex((p) => p.id === follow);
    if (i >= 0) setCur(i);
    setFollow(null);
  }, [visible, follow]);

  // TanStack Virtual renders only the rows on screen. Columns follow the panel width and
  // the thumbnail size; tiles stretch to fill the row, as in Lightroom.
  const [width, setWidth] = useState(900);
  useLayoutEffect(() => {
    const el = gridRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const size = clampThumb(Number(thumb) || THUMB_DEFAULT);
  const dense = size < 110;
  const gap = dense ? 3 : 6;
  const pad = dense ? 8 : 10;
  const cols = Math.max(1, Math.floor((width - pad * 2 + gap) / (size + gap)));
  const tileW = (width - pad * 2 - gap * (cols - 1)) / cols;
  const rowH = tileW * (2 / 3) + 27 + gap;
  const small = tileW * (window.devicePixelRatio || 1) <= THUMB_EDGE;
  const rows = Math.ceil(visible.length / cols);
  const virt = useVirtualizer({ count: rows, getScrollElement: () => gridRef.current, estimateSize: () => rowH, overscan: 3, paddingStart: pad, paddingEnd: pad });
  useEffect(() => virt.measure(), [rowH, virt]);
  useEffect(() => {
    if (anchor.current) return;
    virt.scrollToIndex(Math.floor(cur / cols), { align: 'auto' });
  }, [cur, cols, virt]);

  // Zooming keeps the Photo under the pointer (or the current one, from the keyboard)
  // where it was: remember its index and height on screen, then scroll it back after
  // the grid re-flows.
  const anchor = useRef<{ index: number; frac: number; y: number } | null>(null);
  const layout = useRef({ cols, rowH, pad, gap, tileW });
  layout.current = { cols, rowH, pad, gap, tileW };
  const resize = (next: number, at?: { x: number; y: number }) => {
    const el = gridRef.current;
    const n = clampThumb(Math.round(next));
    if (!el || n === size) return;
    const { cols: c, rowH: h, pad: p, gap: g, tileW: w } = layout.current;
    if (at) {
      const top = el.scrollTop + at.y - p;
      const row = Math.max(0, Math.floor(top / h));
      const col = Math.max(0, Math.min(c - 1, Math.floor((at.x - p) / (w + g))));
      anchor.current = { index: Math.min(visible.length - 1, row * c + col), frac: top / h - row, y: at.y };
    } else {
      const row = Math.floor(cur / c);
      anchor.current = { index: cur, frac: 0.5, y: p + row * h + h / 2 - el.scrollTop };
    }
    setThumb(String(n));
  };
  useLayoutEffect(() => {
    const el = gridRef.current;
    const a = anchor.current;
    if (!el || !a) return;
    anchor.current = null;
    el.scrollTop = pad + (Math.floor(a.index / cols) + a.frac) * rowH - a.y;
  }, [cols, rowH, pad]);
  // Trackpad pinch: Chromium and Firefox send ctrl+wheel; WebKit (the Mac app) sends
  // gesture events. Both scale the size smoothly.
  const sizeRef = useRef(size);
  sizeRef.current = size;
  const resizeRef = useRef(resize);
  resizeRef.current = resize;
  useEffect(() => {
    const el = gridRef.current;
    if (!el) return;
    const point = (e: { clientX: number; clientY: number }) => {
      const r = el.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    const wheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      resizeRef.current(sizeRef.current * Math.exp(-e.deltaY * 0.01), point(e));
    };
    let start = 0;
    type Gesture = Event & { scale: number; clientX: number; clientY: number };
    const gestureStart = (e: Event) => {
      e.preventDefault();
      start = sizeRef.current;
    };
    const gestureChange = (e: Event) => {
      e.preventDefault();
      const g = e as Gesture;
      resizeRef.current(start * g.scale, point(g));
    };
    el.addEventListener('wheel', wheel, { passive: false });
    el.addEventListener('gesturestart', gestureStart);
    el.addEventListener('gesturechange', gestureChange);
    return () => {
      el.removeEventListener('wheel', wheel);
      el.removeEventListener('gesturestart', gestureStart);
      el.removeEventListener('gesturechange', gestureChange);
    };
  }, []);
  useHotkeys(['equal', 'shift+equal'], () => resize(size * 1.25), [size, cur, visible]);
  useHotkeys('minus', () => resize(size / 1.25), [size, cur, visible]);

  useHotkeys('right', () => move(1), { preventDefault: true }, [visible]);
  useHotkeys('left', () => move(-1), { preventDefault: true }, [visible]);
  useHotkeys('down', () => move(cols), { preventDefault: true }, [visible, cols]);
  useHotkeys('up', () => move(-cols), { preventDefault: true }, [visible, cols]);
  useHotkeys(
    ['p', 'x', 'u'],
    (e) => {
      setVerdict(targets, VERDICT_KEYS[e.key.toLowerCase()]);
      // design.md: flagging jumps to the next Photo. A multi-selection stays put.
      if (!sel.size) moveOn();
    },
    [targets, visible, sel],
  );
  // Lightroom: 0-5 set stars (0 clears); pressing the AI's number confirms it. Like flags,
  // rating moves on to the next Photo unless several are selected.
  useHotkeys(
    STAR_KEYS,
    (e) => {
      setStars(targets, Number(e.key));
      if (!sel.size) moveOn();
    },
    [targets, visible, sel],
  );
  useHotkeys(
    'b',
    () => {
      if (!bestPhotos.length) return dispatch({ type: 'toast', text: 'Cull the Shoot first: the AI picks its Best Photos' });
      setFilter((f) => (f === 'best' ? 'all' : 'best'));
      setCur(0);
      setSel(new Set());
    },
    [bestPhotos],
  );
  useHotkeys(['space', 'enter', 'e'], () => current && go({ name: 'loupe', id: current.id }), { preventDefault: true }, [current]);
  useHotkeys('d', () => current && go({ name: 'edit', id: current.id }), [current]);
  useHotkeys('s', () => current?.stackId && toggleStack(current.stackId), [current]);
  useHotkeys('shift+s', () => changeSort(toggleStars(sort)), [sort]);
  // N (Lightroom's Survey key) culls the Shoot's bursts one Stack at a time, from the current Photo's.
  const openBursts = () => (stackCount ? go({ name: 'bursts', id: current?.stackId }) : dispatch({ type: 'toast', text: 'No Stacks yet: bursts stack once Import has looked at the Photos, or after an AI cull' }));
  useHotkeys('n', openBursts, [current, stackCount]);
  useHotkeys(
    'c',
    () => {
      const ids = sel.size > 1 ? [...sel] : current ? stackOf(current).map((x) => x.id) : [];
      if (ids.length > 1) go({ name: 'compare', ids });
      else dispatch({ type: 'toast', text: 'Select two or more photos, or a Stack, to compare' });
    },
    [sel, current],
  );
  useHotkeys('a', () => acceptFirst(targets), [targets, takes]);
  useHotkeys('mod+a', () => setSel(new Set(visible.map((p) => p.id))), { preventDefault: true }, [visible]);
  useHotkeys('escape', () => setSel(new Set()), ESC_OUTSIDE_DIALOGS);
  // Lightroom: Delete removes the selection (or the current Photo) from the Library, after asking.
  useHotkeys(['backspace', 'delete'], () => targets.length && askRemove({ photos: targets }), { preventDefault: true }, [targets]);
  // L sets a Look on the Shoot's Picks; say why when it can't.
  useHotkeys('l', () => (inCollection() ? toast('Open a Shoot to set its Look', { id: 'argus' }) : counts.pick ? go({ name: 'look' }) : toast('Pick some Photos first', { id: 'argus', description: 'A Look is set on the Picks of a Shoot (P picks).' })), [counts.pick]);
  // Lightroom's Auto on the selection, or on the whole Shoot before culling starts.
  const autoIds = sel.size > 1 ? [...sel] : all.map((p) => p.id);
  const autoAll = () => runAuto(autoIds, (id) => recipeOf(state, id), dispatch);
  useHotkeys('shift+a', autoAll, [autoIds, state]);
  useHotkeys('g', () => go({ name: 'cull' }));

  // The right-click menu acts on the Photo under it, and on the selection where it says so.
  const menuFor = (p: Photo): TileMenu => {
    const others = [...sel].filter((id) => id !== p.id);
    return {
      others: others.length,
      onAuto: () => {
        const ids = sel.has(p.id) ? [...sel] : [p.id];
        if (ids.length > 1) runAuto(ids, (id) => recipeOf(state, id), dispatch);
        else dispatch(autoAction(ids, (id) => recipeOf(state, id)));
      },
      onApplyToSelection: () => copyEdit(p.id, others),
      onPaste: () => pasteCopied(sel.has(p.id) && sel.size > 1 ? [...sel] : [p.id]),
      onSync: () => askSettings({ mode: 'sync', from: p.id, targets: others }),
      onCompare: () => go({ name: 'compare', ids: others.length ? [p.id, ...others] : stackOf(p).map((x) => x.id) }),
      onAsk: () => {
        const i = visible.findIndex((x) => x.id === p.id);
        if (i >= 0) setCur(i);
        setSel(new Set());
        requestAnimationFrame(() => document.getElementById('ask-cull')?.focus());
      },
      onExport: () => onExport(sel.has(p.id) && sel.size > 1 ? [...sel] : [p.id]),
      selected: sel.has(p.id),
      onRemove: () => askRemove({ photos: sel.has(p.id) ? [...sel] : [p.id] }),
      stack: { size: p.stackId ? STACK_BY_ID[p.stackId].photoIds.length : 0, open: !!p.stackId && expanded.has(p.stackId), any: stackCount > 0 },
      onToggleStack: () => p.stackId && toggleStack(p.stackId),
      onAllStacks: setAllStacks,
    };
  };
  // "Apply this edit": the whole edit onto the others, as one batch (⇧⌘S asks which groups).
  const copyEdit = (from: string, to: string[]) => pasteSettings(state, dispatch, from, to, ALL_GROUPS);
  const pasteCopied = (to: string[]) => pasteClip(state, dispatch, to);
  useSettingsKeys(current?.id ?? null, [...sel]);
  useHotkeys('mod+shift+a', () => current && copyEdit(current.id, [...sel].filter((id) => id !== current.id)), { preventDefault: true }, [current, sel, state]);
  useHotkeys('mod+l', () => current && menuFor(current).onAsk(), { preventDefault: true }, [current, visible]);
  useHotkeys('mod+shift+r', () => current && LIVE && void revealInFinder({ photo_id: Number(current.id) }).catch(() => {}), { preventDefault: true }, [current]);


  const clickTile = (i: number, p: Photo, e: React.MouseEvent) => {
    if (e.metaKey || e.ctrlKey) {
      setSel((s) => {
        const n = new Set(s.size ? s : current ? [current.id] : []);
        if (n.has(p.id)) n.delete(p.id);
        else n.add(p.id);
        return n;
      });
    } else if (e.shiftKey) {
      const [a, b] = [Math.min(cur, i), Math.max(cur, i)];
      setSel(new Set(visible.slice(a, b + 1).map((x) => x.id)));
      return;
    } else setSel(new Set());
    setCur(i);
  };
  // What the (memoised) tiles call: always this render's state and selection.
  const tileActs = {
    stars: (id: string, n: number) => setStars(sel.has(id) ? [...sel] : [id], n),
    flag: (id: string, v: Verdict) => setVerdict(sel.has(id) ? [...sel] : [id], v),
    click: (p: Photo, e: React.MouseEvent) => clickTile(visible.indexOf(p), p, e),
    toggleStack,
    menuFor,
  };
  const act = useRef(tileActs);
  act.current = tileActs;

  return (
    <>
      <main className="panel main">
        <header className="panel-head toolbar cull-head">
          <h1 title={SHOOT.name}>{SHOOT.name}</h1>
          <ViewSwitch view="grid" id={current?.id} compareIds={sel.size > 1 ? [...sel] : current?.stackId ? STACK_BY_ID[current.stackId].photoIds : undefined} />
          {state.importing && <ImportProgress />}
          <span className="grow" />
          <Seg
            quiet
            label="Filter by flag"
            className="filter-seg"
            value={filter}
            onChange={(f) => {
              setFilter(f);
              setCur(0);
              setSel(new Set());
            }}
            items={(['all', 'pick', 'reject', 'unflagged', 'best'] as Filter[]).map((f) => ({
              value: f,
              title: { all: 'All', pick: 'Picks', reject: 'Rejects', unflagged: 'Unflagged', best: 'Best Photos (B)' }[f],
              label: (
                <>
                  {f === 'all' ? 'All' : f === 'pick' ? <Flag size={11} /> : f === 'reject' ? <X size={12} /> : f === 'unflagged' ? <Square size={11} /> : <Award size={11} />}
                  <span className="n">{culled || f === 'all' || f === 'unflagged' ? counts[f] : '–'}</span>
                </>
              ),
            }))}
          />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" aria-label="Filter by stars" data-testid="stars-filter" className="stars-trigger">
                <Star size={12} fill={minStars ? 'currentColor' : 'none'} /> {minStars ? `${minStars}${minStars < 5 ? '+' : ''}` : 'Any'}
                {aiFilter !== 'any' && <Sparkles size={11} className="ai-icon" />} <ChevronDown size={11} />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {[5, 4, 3, 2, 1, 0].map((n) => (
                <DropdownMenuItem
                  key={n}
                  onSelect={() => {
                    setMinStars(n);
                    setCur(0);
                    setSel(new Set());
                  }}
                >
                  <span style={{ width: 14 }}>{n === minStars && <Check size={12} />}</span>
                  <span className="grow">{n === 0 ? 'Any rating' : n === 5 ? '★★★★★ only' : `${'★'.repeat(n)} and up`}</span>
                  <span className="n mono faint">{atLeast[n]}</span>
                </DropdownMenuItem>
              ))}
              <div className="menu-sep" />
              <div className="menu-label">Background AI</div>
              {(
                [
                  ['any', 'Any', all.length],
                  ['ready', 'Suggestions ready', all.filter((p) => openTakes(p.id) > 0).length],
                  ['waiting', 'Waiting for AI', all.filter((p) => aiOf(p.id) != null).length],
                ] as const
              ).map(([k, label, n]) => (
                <DropdownMenuItem
                  key={k}
                  data-testid={`ai-filter-${k}`}
                  onSelect={() => {
                    setAiFilter(k);
                    setCur(0);
                    setSel(new Set());
                  }}
                >
                  <span style={{ width: 14 }}>{k === aiFilter && <Check size={12} />}</span>
                  <span className="grow">{label}</span>
                  <span className="n mono faint">{n}</span>
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <SortMenu sort={sort} onChange={changeSort} rank={filter === 'best'} />
          <Tip label="Thumbnail size: pinch, or = and −">
            <div className="thumb-size" data-testid="thumb-size">
              <Grid3x3 size={12} />
              <Slider aria-label="Thumbnail size" min={THUMB_MIN} max={THUMB_MAX} step={1} value={[size]} onValueChange={([v]) => resize(v)} />
              <Grid2x2 size={13} className="wide-only" />
            </div>
          </Tip>
          <Tip label={sel.size > 1 ? 'Lightroom-style Auto on the selected Photos' : 'Lightroom-style Auto on every Photo in the Shoot'} keys="⇧A">
            <Button variant="ghost" onClick={autoAll} disabled={!autoIds.length} data-testid="auto-all">
              <Wand2 size={13} /> <span className="mid-only">{sel.size > 1 ? `Auto ${sel.size}` : 'Auto all'}</span>
            </Button>
          </Tip>
          {!inCollection() && (
          <Tip label={culled ? 'Cull the whole Shoot again: Verdicts, stars, Stacks and Best Photos' : 'Cull the whole Shoot: Verdicts, stars, Stacks and Best Photos'}>
            <Button variant="ghost" onClick={onCull} disabled={!all.length || state.culling} data-testid="ai-cull">
              <Sparkles size={13} className="ai-icon" /> <span className="mid-only">{state.culling ? 'Culling…' : 'AI cull'}</span>
            </Button>
          </Tip>
          )}
        </header>
        {filter === 'best' && (
          <BestPhotos
            photos={visible}
            total={all.length}
            render={(p, i) => (
              <Tile
                key={p.id}
                p={p}
                v={verdictOf(state, p.id)}
                you={p.id in state.overrides}
                stars={starsOf(state, p.id)}
                takes={openTakes(p.id)}
                ai={aiOf(p.id)}
                eyes={eyesOf(p.id)}
                best={p.best}
                hero={i === 0}
                onStars={(n) => act.current.stars(p.id, n)}
                selected={sel.has(p.id)}
                current={i === cur}
                highlighted={state.highlight.includes(p.id)}
                stackSize={0}
                expanded={false}
                culling={state.culling}
                onToggleStack={() => {}}
                onClick={(e) => act.current.click(p, e)}
                onSet={(v) => act.current.flag(p.id, v)}
                menu={() => act.current.menuFor(p)}
                small={small}
              />
            )}
          />
        )}
        <div ref={gridRef} className={`scroll grid-v ${dense ? 'dense' : ''}`} hidden={filter === 'best'}>
          <div style={{ height: virt.getTotalSize(), position: 'relative' }}>
            {virt.getVirtualItems().map((row) => (
              <div
                key={row.key}
                className="grid-row"
                style={{ position: 'absolute', top: 0, left: pad, right: pad, transform: `translateY(${row.start}px)`, gap, gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, ['--g' as string]: `${gap}px` }}
              >
                {stackRuns(visible.slice(row.index * cols, row.index * cols + cols), stackPos).map((r) => (
                  <Tip key={`band-${r.stackId}`} label="Collapse Stack" keys="S">
                    <button
                      className="stack-band"
                      data-testid="stack-band"
                      aria-label="Collapse Stack"
                      onClick={() => act.current.toggleStack(r.stackId)}
                      style={{ left: r.from * (tileW + gap) - gap / 2, width: (r.to - r.from + 1) * (tileW + gap), top: -gap / 2, height: rowH }}
                    />
                  </Tip>
                ))}
                {visible.slice(row.index * cols, row.index * cols + cols).map((p, j) => {
                  const i = row.index * cols + j;
                  return (
              <Tile
                key={p.id}
                p={p}
                v={verdictOf(state, p.id)}
                you={p.id in state.overrides}
                stars={starsOf(state, p.id)}
                takes={openTakes(p.id)}
                ai={aiOf(p.id)}
                eyes={eyesOf(p.id)}
                best={culled ? p.best : undefined}
                onStars={(n) => act.current.stars(p.id, n)}
                selected={sel.has(p.id)}
                current={i === cur}
                highlighted={state.highlight.includes(p.id)}
                stackSize={p.stackId && (cover[p.stackId] === p.id || stackPos[p.id]) ? STACK_BY_ID[p.stackId].photoIds.length : 0}
                expanded={!!stackPos[p.id]}
                stackPos={stackPos[p.id]?.n}
                stackFirst={stackPos[p.id]?.first ?? true}
                stackLast={stackPos[p.id]?.last ?? true}
                stackCover={!!p.stackId && cover[p.stackId] === p.id}
                culling={state.culling}
                onToggleStack={() => act.current.toggleStack(p.stackId!)}
                onClick={(e) => act.current.click(p, e)}
                onSet={(v) => act.current.flag(p.id, v)}
                menu={() => act.current.menuFor(p)}
                small={small}
              />
                  );
                })}
              </div>
            ))}
          </div>
          {!visible.length &&
            (PHOTOS.length === 0 ? (
              <div className="faint row" style={{ padding: 24, gap: 12 }}>
                No photos yet.
                <Button variant="default" onClick={() => go({ name: 'import' })} data-testid="empty-import">
                  Import a Shoot
                </Button>
              </div>
            ) : (
              <div className="faint" style={{ padding: 24 }}>Nothing here yet.</div>
            ))}
        </div>
        {/* Lightroom's grid info line: what the cursor is on, then the keys. */}
        <footer className="panel-foot cull-foot">
          {sel.size > 1 ? (
            <span style={{ color: 'var(--text)' }}>{sel.size} selected</span>
          ) : current ? (
            <span className="foot-info" data-testid="cull-info">
              <span className="mono" style={{ color: 'var(--text)' }}>{current.file}</span>
              <span className="mono">{current.exif}</span>
              {current.reason && <span>{current.reason}</span>}
            </span>
          ) : null}
          <span className="grow" />
          <span className="foot-keys">
            <span><Kbd quiet>P</Kbd> Pick</span>
            <span><Kbd quiet>X</Kbd> Reject</span>
            <span><Kbd quiet>U</Kbd> Clear</span>
            <span><Kbd quiet>1-5</Kbd> Stars</span>
            <span><Kbd quiet>C</Kbd> Compare</span>
            <span><Kbd quiet>N</Kbd> Stacks</span>
            <span><Kbd quiet>D</Kbd> Edit</span>
            <span><Kbd quiet>⌘</Kbd><Kbd quiet>Z</Kbd> Undo</span>
          </span>
        </footer>
      </main>
      <Agent
        ctx="cull"
        chips={[`@ ${SHOOT.name}`, culled ? `${counts.pick} Picks` : `${state.imported} photos`, ...(sel.size > 1 ? [`${sel.size} selected`] : current ? [current.file] : [])]}
        placeholder="Cull, fix or explain anything…"
        selected={current}
        onReview={(ids) => {
          setFilter('all');
          dispatch({ type: 'highlight', ids });
          setSel(new Set(ids));
        }}
      />
    </>
  );
}

/** The Shoot's Best Photos, ranked: #1 large, the rest in rank order, each with its reason. */
function BestPhotos({ photos, total, render }: { photos: Photo[]; total: number; render: (p: Photo, i: number) => React.ReactNode }) {
  return (
    <div className="scroll" style={{ flex: 1 }} data-testid="best-photos">
      <div className="best-summary">
        <span>
          {photos.length
            ? `${photos.length} of ${total}, about one in ${Math.max(1, Math.round(total / photos.length))} and never two from one Stack, ranked by ${aiName()}.`
            : 'No Best Photos left: cull the Shoot to have them chosen.'}
        </span>
      </div>
      <div className="best-view">{photos.map(render)}</div>
    </div>
  );
}

/** The views of this Shoot, as icons with their Lightroom keys: Grid, Loupe, Compare, Edit. */
/** An Import filling this Shoot, in the header: count, size, time left and Cancel. */
function ImportProgress() {
  const { state } = useLibrary();
  const done = state.importDone ?? state.imported;
  const total = state.importTotal ?? Math.max(done, PHOTOS.length);
  const left = eta(state.importEta ?? null);
  const bytes = importingBytes();
  const gb = bytes ? bytes / 1e9 : null;
  const copying = (state.importLabel ?? 'Importing').startsWith('Importing');
  return (
    <div className="head-import" data-testid="import-strip">
      <Download size={12} className="ai-icon" />
      <span className="head-import-label">{copying ? 'Importing' : state.importLabel}</span>
      <span className="mono faint head-import-meta">
        {done} / {total}
        {copying && gb && total > 0 ? ` · ${((gb * done) / total).toFixed(1)} of ${gb.toFixed(1)} GB` : ''}
        {left ? ` · ${left.replace(/^about /, '')} left` : ''}
      </span>
      <span className="head-import-bar" role="progressbar" aria-label="Import" aria-valuenow={total ? Math.round((100 * done) / total) : undefined}>
        <span style={{ width: `${total ? (100 * done) / total : 0}%` }} />
      </span>
      {LIVE && copying && (
        <button className="import-link" onClick={() => void call('cancel_import').then(() => toast('Import stopped: the photos already in stay', { id: 'argus' }))} data-testid="import-cancel-running">
          Cancel
        </button>
      )}
    </div>
  );
}
