// The Status bar (Greg, 2026-11-01): one thin bar along the bottom of the window in every
// mode, like Cursor's. Left, always in the same place: what the AI is using on this Mac, then
// the work running behind the screen (Import, Export, an AI cull or Auto, the Queue). Middle:
// the screen's own footer (file info and key hints), which each screen hands in through
// `StatusFoot`. Right: the app update. Each item opens the panel it opened from the sidebar.
// Mockups: canvas section "One status bar in every mode".

import { createContext, Fragment, useContext, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { ArrowDownToLine, ArrowUpCircle, Download, Sparkles, Upload } from 'lucide-react';
import { MeterLine } from './Meter';
import { QueuePopover, queueLine, useQueueOrDemo } from './Queue';
import { useExportProgress } from './Export';
import { eta } from './Progress';
import { useLibrary } from '../store/library';
import { plain, type QueueStatus } from '../data/suggestions';
import { restartToUpdate, useUpdate } from '../data/updates';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { UpdateNotes } from './Updates';

const Slot = createContext<HTMLElement | null>(null);

/** Gives screens the bar's middle, where their footer goes. */
export function StatusSlotProvider({ children }: { children: (setSlot: (el: HTMLElement | null) => void) => ReactNode }) {
  const [slot, setSlot] = useState<HTMLElement | null>(null);
  return <Slot.Provider value={slot}>{children(setSlot)}</Slot.Provider>;
}

/** A screen's footer: file info, counts and key hints, shown in the middle of the Status bar. */
export function StatusFoot({ className = '', children }: { className?: string; children: ReactNode }) {
  const slot = useContext(Slot);
  if (!slot) return null;
  return createPortal(<div className={`status-foot ${className}`}>{children}</div>, slot);
}

/** Engine tasks outside the Queue that the person started and watches, by `progress` kind. */
const FOREGROUND: Record<string, string> = { cull: 'AI cull', auto: 'Auto', rank_presets: 'Presets' };

/** One running piece of work: icon, label, done of total, a short bar and time left. */
function Work({ q, icon, label, done, total, eta_s, testid, title, more }: { q: QueueStatus; icon: ReactNode; label: string; done?: number; total?: number; eta_s?: number | null; testid: string; title?: string; more?: number }) {
  const left = eta(eta_s ?? null);
  const sized = total != null && total > 0;
  return (
    <QueuePopover q={q}>
      <button className="sb-item live" data-testid={testid} title={title ?? label}>
        {icon}
        <span className="sb-label">{label}</span>
        {sized && (
          <span className="mono sb-count">
            {done}/{total}
          </span>
        )}
        {(sized || total === 0) && (
          <span className={`job-bar sb-bar ${sized ? '' : 'indeterminate'}`}>
            <span style={sized ? { width: `${(100 * (done ?? 0)) / total}%` } : undefined} />
          </span>
        )}
        {left && <span className="sb-eta">{left}</span>}
        {more ? <span className="sb-more">+{more} queued</span> : null}
      </button>
    </QueuePopover>
  );
}

/** Everything running behind the screen, what the person is waiting on first. */
function WorkItems() {
  const q = useQueueOrDemo();
  const { state } = useLibrary();
  const exporting = useExportProgress();
  if (!q) return null;
  const items: [string, ReactNode][] = [];
  if (state.importing && state.importTotal) {
    const label = (state.importLabel ?? 'Importing').startsWith('Importing') ? 'Importing' : (state.importLabel ?? 'Importing');
    items.push(['import', <Work q={q} icon={<Upload size={12} className="sb-icon" />} label={label} done={state.importDone ?? state.imported} total={state.importTotal} eta_s={state.importEta ?? null} testid="status-import" />]);
  }
  if (exporting) items.push(['export', <Work q={q} icon={<Download size={12} className="sb-icon on" />} label="Exporting" done={exporting.done} total={exporting.total} eta_s={exporting.eta_s} testid="status-export" />]);
  // Work this window started outside the Queue (an AI cull from the chat, Auto all): the
  // person is watching it, and the Queue itself may be idle (Greg, 2026-10-21).
  for (const t of q.tasks ?? []) {
    if (!FOREGROUND[t.kind]) continue;
    const label = t.waiting ? `${FOREGROUND[t.kind]} · waiting: ${plain(t.waiting)}` : t.kind === 'auto' ? t.label : `${FOREGROUND[t.kind]} · ${t.label}`;
    items.push([`task-${t.kind}`, <Work q={q} icon={<Sparkles size={12} className="sb-icon on" />} label={label} done={t.done} total={t.total} eta_s={t.waiting ? null : t.eta_s} testid="status-task" />]);
  }
  // The Queue (Suggestions, culls, previews, Keywords): only while it has something to say.
  const r = q.running;
  if (r || q.queued > 0 || q.failed > 0) {
    const label = q.paused ? 'Background work paused' : r ? r.label : q.queued ? queueLine(q) : `${q.failed} failed, open to retry`;
    const more = r || q.paused ? q.queued : 0;
    items.push(['queue', <Work q={q} icon={<Sparkles size={12} className="sb-icon" />} label={label} done={r?.done} total={r && r.total > 0 ? r.total : undefined} eta_s={r && !r.waiting && !q.paused ? q.eta_s : null} more={more} testid="queue-progress" title={r?.waiting ? `Waiting: ${plain(r.waiting)}` : 'Background work'} />]);
  }
  return (
    <>
      {items.map(([key, node]) => (
        <Fragment key={key}>
          <span className="sb-sep" />
          {node}
        </Fragment>
      ))}
    </>
  );
}

/** The update, on the right: downloading, then Restart to update. */
function UpdateItem() {
  const s = useUpdate();
  if (s?.state === 'downloading' && s.version) {
    const pct = s.total ? Math.round((100 * s.done) / s.total) : null;
    return (
      <span className="sb-item" data-testid="status-update" title={`Argus ${s.version} is downloading. Keep working.`}>
        <ArrowDownToLine size={12} className="sb-icon" />
        <span className="sb-label">Downloading {s.version}</span>
        <span className={`job-bar sb-bar ${pct == null ? 'indeterminate' : ''}`}>
          <span style={pct == null ? undefined : { width: `${pct}%` }} />
        </span>
      </span>
    );
  }
  if (s?.state !== 'ready') return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button className="sb-item sb-update" onClick={() => void restartToUpdate()} data-testid="update-restart">
          <ArrowUpCircle size={12} />
          <span className="sb-label">Restart to update</span>
          <span className="mono sb-version">{s.version}</span>
        </button>
      </TooltipTrigger>
      <TooltipContent side="top" align="end" className="max-w-[300px]">
        <UpdateNotes s={s} />
      </TooltipContent>
    </Tooltip>
  );
}

/** The bar itself; `slot` receives the middle, where `StatusFoot` renders. */
export function StatusBar({ slot }: { slot: (el: HTMLElement | null) => void }) {
  return (
    <div className="status-bar" data-testid="status-bar">
      <MeterLine />
      <WorkItems />
      <div className="status-mid" ref={slot} />
      <UpdateItem />
    </div>
  );
}
