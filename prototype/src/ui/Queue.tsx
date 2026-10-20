// Background work (culls, Suggestions, previews): a line and a bar in the sidebar, a chip
// where it's worth seeing, and a panel with what's running, waiting and done today, each
// with an honest ETA and its own Cancel, plus Pause all and the Compute level.

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { ArrowUpToLine, BatteryMedium, Check, Coffee, GripVertical, Image, Pause, Play, RotateCcw, ScanEye, Sparkles, X } from 'lucide-react';
import { call } from '../data/backend';
import { whileShown } from '../data/poll';
import { toastFailure } from '@/lib/utils';
import { LIVE } from '../data/live';
import { plain, useQueue, type QueueStatus } from '../data/suggestions';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Button } from '@/components/ui/button';
import { openSettings } from '../settings/Settings';
import { useLibrary } from '../store/library';
import { Kbd, Seg } from './kit';

/** A job as `list_jobs` returns it. */
interface Job {
  id: number;
  kind: string;
  label: string;
  priority: number;
  done: number;
  total: number;
  started_at: string | null;
  finished_at: string | null;
}

/** The queue as the panel reads it: `progress` plus the settings `set_queue` keeps. */
export type Queue = QueueStatus & { boost?: 'idle' | 'overnight' | null; overnight?: boolean; idle_boost_min?: number; running: (QueueStatus['running'] & Partial<Job>) | null };

export const COMPUTE_LABEL = { gentle: 'Gentle', balanced: 'Balanced', full: 'Full' } as const;
export const COMPUTE_NOTE = {
  gentle: 'Gentle runs models at background priority and backs off while you work.',
  balanced: 'Balanced shares the Mac: steady progress, and it stays usable.',
  full: 'Full runs as fast as the Mac allows. Best when you step away.',
} as const;

// engine/src/jobs.rs: the open Photo's Suggestions run at PRIORITY_FOCUS.
const PRIORITY_FOCUS = 100;

/** "6 s", "3 m 48 s", "1 h 4 m". */
export function dur(s: number): string {
  const n = Math.max(1, Math.round(s));
  if (n < 60) return `${n} s`;
  if (n < 3600) return `${Math.floor(n / 60)} m ${String(n % 60).padStart(2, '0')} s`;
  return `${Math.floor(n / 3600)} h ${Math.round((n % 3600) / 60)} m`;
}

/** "about 9 min", "20 s". */
function about(s: number | null | undefined): string | null {
  if (s == null || !isFinite(s)) return null;
  return s < 90 ? `${Math.max(1, Math.round(s / 5) * 5)} s` : `about ${Math.round(s / 60)} min`;
}

const seconds = (a: string | null, b: string | null) => (a && b ? (Date.parse(b) - Date.parse(a)) / 1000 : null);

/** What the queue is doing, in a few words: never just "waiting". */
export function queueLine(q: QueueStatus): string {
  if (q.running) return q.running.label;
  if (q.paused) return 'Paused';
  if (q.battery_paused) return 'Paused on battery';
  if (q.thermal_paused) return 'Paused while the Mac is hot';
  if (q.waiting_for_idle && q.queued > 0) return 'AI waits for you to step away';
  // Between jobs: Gentle and Balanced rest a few seconds so the Mac stays yours.
  return q.compute === 'full' ? 'Starting the next job' : `Resting between jobs · ${COMPUTE_LABEL[q.compute]}`;
}

/** The level it runs at now, and why when that's above its own. */
function levelNow(q: Queue): string {
  if (q.boost === 'overnight') return 'Full overnight';
  if (q.boost === 'idle') return 'Full while idle';
  return COMPUTE_LABEL[q.compute];
}

// The demo has no engine: a queue partway through a Shoot's Suggestions.
const DEMO_QUEUE: Queue = { running: { id: 2, kind: 'suggest_shoot', label: 'Suggestions · Tahoe, Oct 12', done: 38, total: 100, priority: 20, started_at: null, finished_at: null }, queued: 2, failed: 0, done_today: 2, eta_s: 660, compute: 'gentle', paused: false, boost: null };
const DEMO_JOBS: Record<string, Job[]> = {
  queued: [
    { id: 3, kind: 'cull_shoot', label: 'AI cull · Birthday dinner', priority: 50, done: 0, total: 0, started_at: null, finished_at: null },
    { id: 4, kind: 'render_previews', label: 'Previews · Lisbon', priority: 60, done: 0, total: 0, started_at: null, finished_at: null },
    { id: 5, kind: 'analyze', label: 'Analysing photos · Lisbon', priority: 55, done: 0, total: 0, started_at: null, finished_at: null },
  ],
  done: [
    { id: 1, kind: 'import', label: 'Import · 112 photos', priority: 0, done: 0, total: 0, started_at: '2026-10-18T09:00:00Z', finished_at: '2026-10-18T09:03:48Z' },
    { id: 0, kind: 'cull_shoot', label: 'AI cull · Tahoe, Oct 12', priority: 0, done: 0, total: 0, started_at: '2026-10-18T09:04:00Z', finished_at: '2026-10-18T09:10:12Z' },
  ],
};

function useQueueOrDemo(): Queue | null {
  const q = useQueue() as Queue | null;
  return LIVE ? q : DEMO_QUEUE;
}

const listJobs = (status: 'queued' | 'done', limit: number): Promise<Job[]> =>
  LIVE ? call<Job[]>('list_jobs', { status, limit }).catch(() => []) : Promise.resolve(DEMO_JOBS[status]);

/** The Ollama model and whether it's in memory, for the panel's last line. */
function useModelLine(open: boolean): string | null {
  const [line, setLine] = useState<string | null>(null);
  useEffect(() => {
    if (!open) return;
    if (!LIVE) return setLine('Claude · demo');
    call<{ model: string; models: { name: string; loaded: boolean }[]; error?: string }>('models')
      .then((m) => setLine(m.error ? `Ollama not running · ${m.model}` : `Ollama · ${m.model}${m.models.find((x) => x.name === m.model)?.loaded ? ' loaded' : ''}`))
      .catch(() => setLine(null));
  }, [open]);
  return line;
}

const kindIcon = (kind: string) => (kind === 'render_previews' || kind === 'import' ? <Image size={12} /> : kind === 'analyze' ? <ScanEye size={12} /> : <Sparkles size={12} />);
/** A second line for jobs that never touch the LLM, so it's clear they're cheap. */
const kindSub = (kind: string) => (kind === 'render_previews' ? 'no model needed' : kind === 'analyze' ? 'faces, eyes and sky on this Mac, no LLM' : null);

/** A waiting job's drag to reorder (like Lightroom's queue) and Run next. */
interface JobMove {
  dragging: boolean;
  over: boolean;
  onDragStart: () => void;
  onDragOver: () => void;
  onDrop: () => void;
  onDragEnd: () => void;
  onTop?: () => void;
}

function JobRow({ job, eta, sub, focus, progress, onCancel, move }: { job: Job; eta: string | null; sub?: string | null; focus?: boolean; progress?: number; onCancel?: () => void; move?: JobMove }) {
  return (
    <li
      className={`job ${move ? 'movable' : ''} ${move?.dragging ? 'dragging' : ''} ${move?.over ? 'drop-over' : ''}`}
      data-testid="queue-job"
      draggable={!!move}
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', String(job.id));
        move?.onDragStart();
      }}
      onDragOver={(e) => {
        if (!move) return;
        e.preventDefault();
        move.onDragOver();
      }}
      onDrop={(e) => {
        e.preventDefault();
        move?.onDrop();
      }}
      onDragEnd={move?.onDragEnd}
    >
      <div className="job-head">
        {move && (
          <span className="job-grip" aria-hidden="true">
            <GripVertical size={11} />
          </span>
        )}
        <span className="job-icon">{kindIcon(job.kind)}</span>
        <span className="job-label">{job.label}</span>
        {focus && <span className="job-tag">your photo, first</span>}
        <span className="grow" />
        {eta && <span className="mono faint">{eta}</span>}
        {move?.onTop && (
          <button className="job-x" title="Run next" aria-label={`Run ${job.label} next`} onClick={move.onTop}>
            <ArrowUpToLine size={11} />
          </button>
        )}
        {onCancel && (
          <button className="job-x" title="Cancel this job" aria-label={`Cancel ${job.label}`} onClick={onCancel}>
            <X size={11} />
          </button>
        )}
      </div>
      {sub && <div className="job-sub">{sub}</div>}
      {progress != null && (
        <div className="job-bar">
          <span style={{ width: `${Math.min(100, progress * 100)}%` }} />
        </div>
      )}
    </li>
  );
}

export function QueuePopover({ q, children }: { q: QueueStatus; children: ReactNode }) {
  const queue = q as Queue;
  const [open, setOpen] = useState(false);
  const [waiting, setWaiting] = useState<Job[]>([]);
  const [done, setDone] = useState<Job[]>([]);
  const [, bump] = useState(0);
  const model = useModelLine(open);
  // Drag to reorder: the job picked up and the one it's over. Polling waits while you drag.
  const [drag, setDrag] = useState<{ id: number; over: number | null } | null>(null);
  const dragging = useRef(false);
  dragging.current = !!drag;
  const refresh = () => {
    void listJobs('queued', 50).then((j) => !dragging.current && setWaiting([...j].sort((a, b) => b.priority - a.priority || a.id - b.id)));
    void listJobs('done', 40).then(setDone);
  };
  useEffect(() => {
    if (!open) return;
    refresh();
    return whileShown(refresh, 3000);
  }, [open]);
  const set = (args: Record<string, unknown>) => (LIVE ? call('set_queue', args).then(() => bump((n) => n + 1), toastFailure('Couldn’t change Background work')) : Promise.resolve());
  const cancel = (id: number) => (LIVE ? call('cancel_job', { job_id: id }).then(refresh) : undefined);
  const reorder = (ids: number[]) => {
    setWaiting((cur) => ids.map((id) => cur.find((j) => j.id === id)!).filter(Boolean));
    if (LIVE) void call('reorder_jobs', { job_ids: ids }).then(refresh, refresh);
    // Dummy data: keep the new order, as the engine would.
    else ids.forEach((id, i) => DEMO_JOBS.queued.forEach((j) => j.id === id && j.priority < PRIORITY_FOCUS && (j.priority = 90 - i)));
  };
  // The open Photo's work runs first whatever the order; the rest move below it.
  const firstMovable = waiting.find((j) => j.priority < PRIORITY_FOCUS);
  const moveTo = (id: number, before: number) => {
    const ids = waiting.map((j) => j.id).filter((x) => x !== id);
    const pinned = waiting.filter((j) => j.priority >= PRIORITY_FOCUS).length;
    ids.splice(Math.max(pinned, ids.indexOf(before)), 0, id);
    reorder(ids);
  };
  const retry = () => (LIVE ? call('retry_failed').then(() => (refresh(), bump((n) => n + 1))) : undefined);

  // Seconds per job of each kind, from what finished lately: the honest basis for an ETA.
  const avg: Record<string, number> = {};
  const n: Record<string, number> = {};
  for (const j of done) {
    const s = seconds(j.started_at, j.finished_at);
    if (s == null) continue;
    avg[j.kind] = ((avg[j.kind] ?? 0) * (n[j.kind] ?? 0) + s) / ((n[j.kind] ?? 0) + 1);
    n[j.kind] = (n[j.kind] ?? 0) + 1;
  }
  const r = queue.running;
  const elapsed = r?.started_at ? (Date.now() - Date.parse(r.started_at)) / 1000 : null;
  const runningLeft = r?.waiting ? null : r && r.total > 0 && r.done > 0 && elapsed != null ? (elapsed / r.done) * (r.total - r.done) : r && avg[r.kind] != null && elapsed != null ? Math.max(0, avg[r.kind] - elapsed) : null;
  const today = new Date().toISOString().slice(0, 10);
  const doneToday = done.filter((j) => (j.finished_at ?? '').startsWith(today) || !LIVE).slice(0, 4);
  const jobs = (r ? 1 : 0) + queue.queued;
  const left = queue.eta_s != null ? queue.eta_s + (runningLeft ?? 0) : runningLeft;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="start" side="top" className="queue-panel w-[400px] p-0" data-testid="queue-popover">
        <div className="qp-head">
          <span className="qp-title">Background work</span>
          <span className="mono faint">
            {jobs} {jobs === 1 ? 'job' : 'jobs'}
          </span>
          {queue.failed > 0 && (
            <button className="qp-retry mono" onClick={() => void retry()} title="Retry failed jobs" aria-label="Retry failed jobs" data-testid="queue-retry">
              {queue.failed} failed <RotateCcw size={11} />
            </button>
          )}
          <span className="grow" />
          <Button size="sm" onClick={() => void set({ paused: !queue.paused })} data-testid="queue-pause">
            {queue.paused ? <Play size={12} /> : <Pause size={12} />} {queue.paused ? 'Resume' : 'Pause'}
          </Button>
        </div>
        <div className="qp-body">
          <Seg label="Compute" className="seg-wide" value={queue.compute} onChange={(v) => void set({ compute: v })} items={(['gentle', 'balanced', 'full'] as const).map((c) => ({ value: c, label: COMPUTE_LABEL[c] }))} />
          <p className="qp-note" data-testid="queue-note">
            {COMPUTE_NOTE[queue.compute]}
            {queue.paused ? ' Paused: nothing new starts until you resume.' : queue.battery_paused ? '' : left != null ? ` About ${about(left)?.replace(/^about /, '')} left at this pace.` : ''}
            {queue.boost && ` Running at Full now, ${queue.boost === 'idle' ? 'while the Mac is idle' : 'overnight'}.`}
          </p>
          {queue.battery_paused && !queue.paused && (
            <div className="qp-battery" data-testid="queue-battery">
              <BatteryMedium size={12} />
              <span className="grow">On battery: waiting for power. The photo you’re on still runs.</span>
              <Button size="sm" onClick={() => void set({ run_anyway: true })} data-testid="queue-run-anyway">
                Run anyway
              </Button>
            </div>
          )}
          {queue.thermal_paused && !queue.paused && (
            <p className="qp-note" data-testid="queue-thermal">
              The Mac is running hot: background work waits for it to cool. The photo you’re on still runs.
            </p>
          )}
          {queue.waiting_for_idle && !queue.paused && queue.queued > 0 && (
            <div className="qp-battery" data-testid="queue-idle">
              <Coffee size={12} />
              <span className="grow">{queue.waiting_for_overnight ? 'Culls and big batches wait for overnight, so the Mac stays cool.' : 'Culls and Suggestions wait until you step away, or overnight, so the Mac stays yours.'}</span>
              <Button size="sm" onClick={() => void set({ run_now: true })} data-testid="queue-run-now">
                Run now
              </Button>
            </div>
          )}
          {queue.run_now && !queue.paused && (
            <p className="qp-note" data-testid="queue-idle">
              Running AI work now, until the queue is empty.
            </p>
          )}
          {queue.model_busy && (!r || r.waiting) && (
            <p className="qp-note" data-testid="queue-model-busy">
              Model busy: {plain(queue.model_busy)}. AI work here waits its turn.
            </p>
          )}
          {queue.on_battery && queue.run_anyway && !queue.paused && (
            <p className="qp-note" data-testid="queue-battery">
              Running on battery until you plug in.
            </p>
          )}
        </div>
        {r && (
          <section className="qp-section">
            <div className="qp-label">Running</div>
            <ul>
              <JobRow
                job={r as Job}
                eta={about(runningLeft)}
                focus={(r.priority ?? 0) >= PRIORITY_FOCUS}
                sub={r.waiting ? `Waiting: ${plain(r.waiting)}` : [r.total > 0 ? `${r.done} / ${r.total}` : null, r.stage && r.stage !== r.label ? r.stage : null, (r.priority ?? 0) >= PRIORITY_FOCUS ? 'at Full while it’s open' : levelNow(queue)].filter(Boolean).join(' · ')}
                progress={r.total > 0 ? r.done / r.total : undefined}
                onCancel={() => cancel(r.id)}
              />
            </ul>
          </section>
        )}
        {waiting.length > 0 && (
          <section className="qp-section">
            <div className="qp-label">Waiting</div>
            <ul data-testid="queue-jobs">
              {waiting.slice(0, 6).map((j) => (
                <JobRow
                  key={j.id}
                  job={j}
                  eta={about(avg[j.kind])}
                  focus={j.priority >= PRIORITY_FOCUS}
                  sub={kindSub(j.kind)}
                  onCancel={() => cancel(j.id)}
                  move={
                    waiting.length > 1 && j.priority < PRIORITY_FOCUS
                      ? {
                          dragging: drag?.id === j.id,
                          over: !!drag && drag.over === j.id && drag.id !== j.id,
                          onDragStart: () => setDrag({ id: j.id, over: null }),
                          onDragOver: () => drag && drag.over !== j.id && setDrag({ ...drag, over: j.id }),
                          onDrop: () => {
                            if (drag && drag.id !== j.id) moveTo(drag.id, j.id);
                            setDrag(null);
                          },
                          onDragEnd: () => setDrag(null),
                          onTop: firstMovable && firstMovable.id !== j.id ? () => moveTo(j.id, firstMovable.id) : undefined,
                        }
                      : undefined
                  }
                />
              ))}
              {waiting.length > 6 && <li className="faint qp-more">and {waiting.length - 6} more</li>}
            </ul>
          </section>
        )}
        {!r && !waiting.length && <div className="qp-empty muted">{queue.paused ? 'Paused, and nothing waiting.' : 'Nothing running. New Shoots get a cull and Suggestions here.'}</div>}
        {doneToday.length > 0 && (
          <section className="qp-section">
            <div className="qp-label">Done today</div>
            <ul>
              {doneToday.map((j) => {
                const s = seconds(j.started_at, j.finished_at);
                return (
                  <li key={j.id} className="job done">
                    <div className="job-head">
                      <Check size={12} className="job-ok" />
                      <span className="job-label">{j.label}</span>
                      <span className="grow" />
                      {s != null && <span className="mono faint">{dur(s)}</span>}
                    </div>
                  </li>
                );
              })}
            </ul>
          </section>
        )}
        <div className="qp-foot">
          {model && (
            <>
              <span className="status-dot" />
              <span className="mono">{model}</span>
            </>
          )}
          <span className="grow" />
          <button
            className="import-link"
            onClick={() => {
              setOpen(false);
              openSettings('ai');
            }}
          >
            Settings <Kbd>⌘,</Kbd>
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** The compact chip: what's running, how far, how many wait. Click for the panel. */
export function QueueChip() {
  const q = useQueue();
  if (!q || (!q.running && q.queued === 0)) return null;
  const r = q.running;
  return (
    <QueuePopover q={q}>
      <button className="queue-chip" data-testid="queue" title={queueLine(q)}>
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{queueLine(q)}</span>
        {r && r.total > 0 && (
          <span className="bar">
            <span style={{ width: `${Math.round((100 * r.done) / r.total)}%` }} />
          </span>
        )}
        {q.queued > 0 && <span className="mono faint">+{q.queued}</span>}
      </button>
    </QueuePopover>
  );
}

/** The queue at a glance at the foot of the sidebar: what runs, a bar, time left and the level. */
export function QueueProgress() {
  const q = useQueueOrDemo();
  const { state } = useLibrary();
  if (!q) return null;
  // An Import filling the open Shoot comes first: it's what the person is waiting on.
  if (state.importing && state.importTotal) {
    const done = state.importDone ?? state.imported;
    const left = about(state.importEta);
    const copying = (state.importLabel ?? 'Importing').startsWith('Importing');
    return (
      <QueuePopover q={q}>
        <button className="queue-side" data-testid="queue-progress" title="Background work">
          <span className="qs-line">
            <Sparkles size={12} className="qs-icon" />
            <span className="qs-label">{copying ? 'Import · copying' : state.importLabel}</span>
            <span className="grow" />
            <span className="mono faint">
              {done}/{state.importTotal}
            </span>
          </span>
          <span className="job-bar">
            <span style={{ width: `${(100 * done) / state.importTotal}%` }} />
          </span>
          <span className="qs-meta">
            {left ?? 'working'} · {levelNow(q)}
          </span>
        </button>
      </QueuePopover>
    );
  }
  const r = q.running;
  const idle = !r && q.queued === 0;
  const left = r?.waiting ? null : about(q.eta_s);
  const held = q.battery_paused ? 'wait for power' : q.thermal_paused ? 'wait for the Mac to cool' : q.waiting_for_idle ? 'wait for you to step away' : null;
  return (
    <QueuePopover q={q}>
      <button className="queue-side" data-testid="queue-progress" title={r?.waiting ? `Waiting: ${plain(r.waiting)}` : 'Background work'}>
        <span className="qs-line">
          <Sparkles size={12} className="qs-icon" />
          <span className="qs-label">{q.paused ? 'Background work paused' : idle ? 'Background work' : r ? r.label : queueLine(q)}</span>
          <span className="grow" />
          {r && r.total > 0 && (
            <span className="mono faint">
              {r.done}/{r.total}
            </span>
          )}
        </span>
        <span className="job-bar">
          <span style={{ width: r && r.total > 0 ? `${(100 * r.done) / r.total}%` : '0%' }} />
        </span>
        <span className="qs-meta">
          {idle ? (q.failed ? `${q.failed} failed, open to retry` : 'nothing waiting') : q.paused ? (q.queued ? `${q.queued} waiting, resume to run` : 'resume to run') : r?.waiting ? 'waiting for the model' : held && !r ? `${q.queued} ${held}` : left ? `${left}${q.queued ? ` · +${q.queued}` : ''}` : q.queued ? `+${q.queued} waiting` : 'working'} · {levelNow(q)}
        </span>
      </button>
    </QueuePopover>
  );
}
