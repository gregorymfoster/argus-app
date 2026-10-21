// The pieces of the v2 one-line header (Design pass v2): the icon view switch, Lightroom's
// G / E / C / D, and a small meter for the background queue. Loupe, Compare and Look share
// them; any screen's header can.

import type { ReactNode } from 'react';
import { ArrowLeft, Columns2, Layers, LayoutGrid, SlidersHorizontal, Sparkles, Square } from 'lucide-react';
import { go } from '../router';
import { STACKS } from '../data/dummy';
import { useQueue } from '../data/suggestions';
import { QueuePopover, queueLine } from './Queue';
import { Kbd, Tip } from './kit';
import { Button } from '@/components/ui/button';

export type View = 'grid' | 'loupe' | 'compare' | 'bursts' | 'edit';

/** Grid, Loupe, Compare, Stacks (burst culling) and Edit for `id`; Compare needs `compareIds` (a Stack or a selection). */
export function ViewSwitch({ view, id, compareIds }: { view: View; id?: string; compareIds?: string[] }) {
  const items: { v: View; label: string; key: string; icon: ReactNode; to?: () => void }[] = [
    { v: 'grid', label: 'Grid', key: 'G', icon: <LayoutGrid size={12} />, to: () => go({ name: 'cull' }) },
    { v: 'loupe', label: 'Loupe', key: 'E', icon: <Square size={12} />, to: id ? () => go({ name: 'loupe', id }) : undefined },
    { v: 'compare', label: 'Compare', key: 'C', icon: <Columns2 size={12} />, to: compareIds && compareIds.length > 1 ? () => go({ name: 'compare', ids: compareIds }) : undefined },
    { v: 'bursts', label: 'Stacks: keep the best of each burst', key: 'N', icon: <Layers size={12} />, to: STACKS.length ? () => go({ name: 'bursts' }) : undefined },
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

/** ← Grid G, then a hairline: where every v2 header starts. */
export function BackToGrid() {
  return (
    <>
      <Button variant="ghost" size="sm" onClick={() => go({ name: 'cull' })}>
        <ArrowLeft size={13} /> Grid <Kbd quiet>G</Kbd>
      </Button>
      <span className="vsep" />
    </>
  );
}

/** A row of key hints: ["Z", "100%"], ["← →", "Move"]… */
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
