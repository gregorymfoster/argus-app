// The pieces of the v2 one-line header (Design pass v2): the icon view switch, Lightroom's
// G / E / C / D, and a small meter for the background queue. Loupe, Compare and Look share
// them; any screen's header can.

import type { ReactNode } from 'react';
import { Columns2, Download, Layers, LayoutGrid, Library as LibraryIcon, SlidersHorizontal, Sparkles, Square, type LucideIcon } from 'lucide-react';
import { go } from '../router';
import { useLibrary } from '../store/library';
import { cullProgress, editTarget, isEdited, picksOf, type Mode } from '../data/modes';
import { useQueue } from '../data/suggestions';
import { QueuePopover, queueLine } from './Queue';
import { Kbd, Tip } from './kit';

export type View = 'grid' | 'loupe' | 'compare' | 'edit';

/** Grid, Loupe, Compare and Edit for `id`; Compare needs `compareIds` (a Stack or a selection). */
export function ViewSwitch({ view, id, compareIds }: { view: View; id?: string; compareIds?: string[] }) {
  const items: { v: View; label: string; key: string; icon: ReactNode; to?: () => void }[] = [
    { v: 'grid', label: 'Grid', key: 'G', icon: <LayoutGrid size={12} />, to: () => go({ name: 'cull' }) },
    { v: 'loupe', label: 'Loupe', key: 'E', icon: <Square size={12} />, to: id ? () => go({ name: 'loupe', id }) : undefined },
    { v: 'compare', label: 'Compare', key: 'C', icon: <Columns2 size={12} />, to: compareIds && compareIds.length > 1 ? () => go({ name: 'compare', ids: compareIds }) : undefined },
    { v: 'edit', label: 'Edit', key: 'D', icon: <SlidersHorizontal size={12} />, to: id ? () => go({ name: 'edit', id }) : undefined },
  ];
  return (
    <span className="view-switch" role="group" aria-label="View">
      {items.map((i) => (
        <Tip key={i.v} label={i.label} keys={i.key}>
          <button className={i.v === view ? 'on' : ''} aria-label={`${i.label} (${i.key})`} aria-pressed={i.v === view} disabled={!i.to && i.v !== view} onClick={() => i.v !== view && i.to?.()}>
            {i.icon}
          </button>
        </Tip>
      ))}
    </span>
  );
}

/** Go to a mode, as ⌘1 to ⌘4 and the mode switch do. `current` is the Photo in hand. */
export function goMode(mode: Mode, state: ReturnType<typeof useLibrary>['state'], dispatch: ReturnType<typeof useLibrary>['dispatch'], current?: string) {
  if (mode === 'library') return go({ name: 'cull' });
  if (mode === 'cull') return go(current ? { name: 'moments', id: current } : { name: 'moments' });
  if (mode === 'export') return go({ name: 'export' });
  const id = editTarget(state, current);
  if (id) return go({ name: 'edit', id });
  dispatch({ type: 'toast', text: 'No Picks yet: keep some in Cull (⌘2) first' });
}

/** Library ⌘1 · Cull ⌘2 · Edit ⌘3 · Export ⌘4, with each pass's progress: where every header starts
 *  (as Lightroom Classic's Library and Develop). `current` is the Photo in hand, if any. */
export function ModeSwitch({ mode, current }: { mode: Mode; current?: string }) {
  const { state, dispatch } = useLibrary();
  const cull = cullProgress(state);
  const picks = picksOf(state);
  const edited = picks.filter((p) => isEdited(state, p)).length;
  const items: { m: Mode; label: string; Icon: LucideIcon; keys: string; count?: string; tip: string }[] = [
    { m: 'library', label: 'Library', Icon: LibraryIcon, keys: '⌘1', tip: 'Browse the Shoot: grid, Loupe, Compare' },
    { m: 'cull', label: 'Cull', Icon: Layers, keys: '⌘2', count: cull.total ? `${cull.done}/${cull.total}` : undefined, tip: 'Keep the best of each moment, side by side' },
    { m: 'edit', label: 'Edit', Icon: SlidersHorizontal, keys: '⌘3', count: picks.length ? `${edited}/${picks.length}` : undefined, tip: 'Edit the Picks' },
    { m: 'export', label: 'Export', Icon: Download, keys: '⌘4', tip: 'Export the Picks' },
  ];
  return (
    <span className="mode-switch" role="group" aria-label="Mode" data-testid="mode-switch">
      {items.map((i) => (
        <Tip key={i.m} label={i.tip} keys={i.keys}>
          <button className={i.m === mode ? 'on' : ''} aria-pressed={i.m === mode} aria-label={i.label} onClick={() => i.m !== mode && goMode(i.m, state, dispatch, current)} data-testid={`mode-${i.m}`}>
            <i.Icon size={12} className="mode-icon" />
            <span className="mode-label">{i.label}</span>
            {i.count && <span className="mono count">{i.count}</span>}
          </button>
        </Tip>
      ))}
    </span>
  );
}

/** The queue at a glance: a sparkle, done of total and a short bar. Click for the queue. */
export function QueueMeter() {
  const q = useQueue();
  if (!q || (!q.running && q.queued === 0)) return null;
  const r = q.running;
  const pct = r && r.total > 0 ? Math.round((100 * r.done) / r.total) : null;
  return (
    <QueuePopover q={q}>
      <button className="queue-meter" data-testid="queue" title={queueLine(q)} aria-label="Background work">
        <Sparkles size={12} />
        {r && r.total > 0 ? <span className="mono">{r.done}/{r.total}</span> : <span className="mono">+{q.queued}</span>}
        {pct != null && (
          <span className="bar">
            <i style={{ width: `${pct}%` }} />
          </span>
        )}
      </button>
    </QueuePopover>
  );
}

/** The mode switch (in the Library), then a hairline: where Loupe, Compare and Look headers
 *  start. G still goes back to the grid. */
export function BackToGrid({ current }: { current?: string }) {
  return (
    <>
      <ModeSwitch mode="library" current={current} />
      <span className="vsep" />
    </>
  );
}

/** A row of key hints: ["Space", "100%"], ["← →", "Move"]… */
export function Hints({ items }: { items: [keys: string, label: string][] }) {
  return (
    <span className="hints">
      {items.map(([keys, label], i) => (
        <span key={label} className="hint">
          {i > 0 && <span className="dot">·</span>}
          {keys.split(' ').map((k) => (
            <Kbd key={k} quiet>{k}</Kbd>
          ))}
          <span>{label}</span>
        </span>
      ))}
    </span>
  );
}
