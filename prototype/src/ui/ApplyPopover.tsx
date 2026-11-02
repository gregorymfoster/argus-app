// The Apply popover (Greg, 2026-11-02: "I love the pop-over 'Apply to all' kind of experience.
// I think this should be my average experience when applying edits to everything"). Every edit
// that reaches more than one Photo opens this: each Photo drawn as it will look, a tick per
// Photo to leave it out, Return applies to the ticked ones as one Undo, Esc changes nothing.
// See "Applying to many Photos" in docs/design.md.
//
// This file is the shared frame (header, note with a real progress bar, tile grid, foot with
// Cancel and Apply). Apply look (ui/ApplyLook.tsx), Crop & straighten (ui/Framing.tsx) and
// Paste, Sync and Auto (ui/ApplyEdit.tsx) fill it with their own tiles and words.

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Check } from 'lucide-react';
import { Popover as P } from 'radix-ui';
import { PHOTOS } from '../data/dummy';
import { ProgressBar } from './Progress';
import { Kbd, Seg } from './kit';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Button } from '@/components/ui/button';
import { markEscape } from '@/components/ui/dialog';

export type Scope = 'picks' | 'all';

export interface ApplyTile {
  id: string;
  /** The engine's render of the Photo as it will look. */
  image?: string;
  /** Drawn while there is no `image` (the demo, a held \, a frame not drawn yet). */
  fallback: ReactNode;
  badge?: ReactNode;
  /** The badge asks for a look (shown in the Shoot color). */
  warn?: boolean;
  /** Not a choice: the source of the edit, or a frame that is always kept as it is. */
  lock?: 'source' | 'kept';
  /** Starts left out (edited by hand); a click still ticks it. Undefined while not known yet
   * (the engine hasn't drawn it): the tile starts ticked and is settled once it is known. */
  startOff?: boolean;
  /** Words for a screen reader after the file name. */
  says?: string;
}

export interface ApplyProps {
  /** Prefix of the test ids: `${testId}`, `${testId}-tile`, `${testId}-go`, `${testId}-reading`. */
  testId: string;
  title: ReactNode;
  /** Quiet words after the title: the Shoot, or where the edit comes from. */
  context?: ReactNode;
  /** Extra controls in the header, before the scope. */
  controls?: ReactNode;
  scope?: { value: Scope; onChange: (s: Scope) => void; picks: number; all: number };
  note: ReactNode;
  /** The first pass: drawn so far of how many, with an ETA; null once all are drawn. */
  reading: { label: string; done: number; total: number; eta: number | null } | null;
  /** Shown in the note's corner once every tile is drawn. */
  idle?: ReactNode;
  tiles: ApplyTile[];
  /** Shown in the grid when there are no tiles and nothing is being drawn. */
  empty?: ReactNode;
  /** The foot's words for `n` ticked Photos. */
  foot: (n: number) => ReactNode;
  /** Quiet words before Cancel: an error, or frames not drawn. */
  footNote?: ReactNode;
  /** Apply the edit to the ticked Photos. A promise keeps the popover open, counting up, until it settles. */
  onApply: (ids: string[]) => void | Promise<unknown>;
  /** Apply waits while this is true (proposals still being read). */
  wait?: boolean;
  onClose: () => void;
}

/** The popover around a trigger (a header button). */
export function ApplyPopover({ open, onOpenChange, testId, children, body }: { open: boolean; onOpenChange: (on: boolean) => void; testId: string; children: ReactNode; body: () => ReactNode }) {
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="end" className="apply-look w-[min(880px,calc(100vw-48px))] p-0" onOpenAutoFocus={(e) => e.preventDefault()} onEscapeKeyDown={markEscape} data-testid={testId}>
        {open && body()}
      </PopoverContent>
    </Popover>
  );
}

/** The popover at a point (opened by a key or a menu, with no button of its own). */
export function ApplyPopoverAt({ at, onClose, testId, children }: { at: DOMRect | null; onClose: () => void; testId: string; children: ReactNode }) {
  const rect = useRef<DOMRect>(new DOMRect());
  if (at) rect.current = at;
  return (
    <Popover open={!!at} onOpenChange={(o) => !o && onClose()}>
      <P.Anchor virtualRef={{ current: { getBoundingClientRect: () => rect.current } }} />
      <PopoverContent align="end" className="apply-look w-[min(880px,calc(100vw-48px))] p-0" onOpenAutoFocus={(e) => e.preventDefault()} onEscapeKeyDown={markEscape} data-testid={testId}>
        {at && children}
      </PopoverContent>
    </Popover>
  );
}

/** Where a popover opened by a key sits: under the toolbar, at the window's right. */
export const toolbarCorner = () => new DOMRect(window.innerWidth - 16, 44, 0, 0);

const fileOf = (id: string) => PHOTOS.find((p) => p.id === id)?.file ?? id;

export function ApplyBody(props: ApplyProps) {
  const { testId, tiles, onApply, onClose, wait } = props;
  // Left out by the person. Hand-edited Photos start out, once, when they first show.
  const [off, setOff] = useState<Set<string>>(new Set());
  const seen = useRef(new Set<string>());
  useEffect(() => {
    const fresh = tiles.filter((t) => !seen.current.has(t.id) && t.startOff !== undefined);
    if (!fresh.length) return;
    for (const t of fresh) seen.current.add(t.id);
    const out = fresh.filter((t) => t.startOff).map((t) => t.id);
    if (out.length) setOff((s) => new Set([...s, ...out]));
  }, [tiles]);
  const targets = tiles.filter((t) => !t.lock && !off.has(t.id)).map((t) => t.id);

  // Applying: since when, so a long one counts up instead of sitting silent.
  const [busy, setBusy] = useState<number | null>(null);
  const [now, setNow] = useState(0);
  useEffect(() => {
    if (busy == null) return;
    const t = setInterval(() => setNow(performance.now()), 1000);
    return () => clearInterval(t);
  }, [busy]);
  const busyS = busy == null ? 0 : Math.max(0, Math.round((now - busy) / 1000));

  const apply = () => {
    if (!targets.length || busy != null || wait) return;
    const r = onApply(targets);
    if (!(r instanceof Promise)) return;
    setBusy(performance.now());
    setNow(performance.now());
    void r.finally(() => setBusy(null));
  };
  // While it is open the popover owns the keyboard, as a dialog would: Return applies, Esc
  // closes, \ shows befores and Tab moves between its controls; the screen behind (Cull's flag
  // and arrow keys, Edit's sliders) hears nothing.
  const applyRef = useRef(apply);
  applyRef.current = apply;
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || e.key === 'Tab' || e.key === '\\') return;
      if ((e.target as HTMLElement | null)?.closest?.('[role="menu"]')) return;
      e.stopPropagation();
      if (e.key === 'Enter') {
        e.preventDefault();
        applyRef.current();
      }
    };
    window.addEventListener('keydown', on, true);
    return () => window.removeEventListener('keydown', on, true);
  }, []);

  const toggle = (id: string) =>
    setOff((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  return (
    <div className="apply-look-body">
      <div className="apply-look-head">
        <span className="apply-look-title">{props.title}</span>
        {props.context && <span className="faint mono trunc">{props.context}</span>}
        <span className="grow" />
        {props.controls}
        {props.scope && props.scope.picks > 0 && (
          <Seg
            quiet
            label="Which Photos"
            value={props.scope.value}
            onChange={props.scope.onChange}
            items={[
              { value: 'picks', label: <>Picks <span className="mono faint">{props.scope.picks}</span></> },
              { value: 'all', label: <>All but Rejects <span className="mono faint">{props.scope.all}</span></> },
            ]}
          />
        )}
      </div>
      <div className="apply-look-note">
        <span>{props.note}</span>
        <span className="grow" />
        {props.reading ? (
          <span className="apply-look-bar">
            <ProgressBar label={props.reading.label} done={props.reading.done} total={props.reading.total} eta={props.reading.eta} testId={`${testId}-reading`} />
          </span>
        ) : (
          props.idle
        )}
      </div>
      <div className="apply-look-grid">
        {tiles.map((t) => {
          const out = !t.lock && off.has(t.id);
          return (
            <button key={t.id} className={`apply-tile${t.lock === 'source' ? ' hero' : ''}${out || t.lock === 'kept' ? ' off' : ''}`} onClick={() => !t.lock && toggle(t.id)} disabled={!!t.lock} aria-pressed={t.lock ? undefined : !out} aria-label={`${fileOf(t.id)}${t.says ? `, ${t.says}` : ''}${out ? ', left out' : ''}`} data-testid={`${testId}-tile`} data-drawn={t.image ? 'yes' : undefined}>
              <span className="apply-well">
                {t.image ? <img src={t.image} alt="" draggable={false} /> : t.fallback}
                {!t.lock && <span className={`apply-tick${out ? '' : ' on'}`}>{!out && <Check size={9} strokeWidth={3} />}</span>}
                {t.badge && <span className={`look-badge${t.lock === 'source' ? ' hero' : t.warn ? ' check' : ''}`}>{t.badge}</span>}
              </span>
              <span className="apply-cap mono">{fileOf(t.id)}</span>
            </button>
          );
        })}
        {!props.reading && !tiles.length && props.empty}
      </div>
      <div className="apply-look-foot">
        <span className="trunc">{props.foot(targets.length)}</span>
        <span className="grow" />
        {props.footNote}
        {busy != null && busyS >= 3 && <span className="faint mono" data-testid={`${testId}-busy`}>Applying · {busyS} s</span>}
        <Button variant="ghost" size="sm" onClick={onClose}>
          Cancel <Kbd quiet>esc</Kbd>
        </Button>
        <Button size="sm" onClick={apply} disabled={!targets.length || busy != null || !!wait} data-testid={`${testId}-go`}>
          Apply to {targets.length} <Kbd>↵</Kbd>
        </Button>
      </div>
    </div>
  );
}
