// Everything running right now, in one list (Greg, 2026-11-02): an Import, an Export, an AI
// cull or Auto the person started, and the Queue's job (Suggestions, previews, backups,
// learning from edits). The Status bar, the Background work panel and Work mode's Running
// card all read this, so a piece of work shows the same way wherever you look.

import { Download, Image, Images, Sparkles, Upload, X, FileOutput } from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';
import { toast } from 'sonner';
import { call } from '../data/backend';
import { LIVE } from '../data/live';
import { plain, type QueueStatus } from '../data/suggestions';
import { lastWake, sleptNote, useAppWork, workEta } from '../data/work';
import { useLibrary } from '../store/library';
import { eta } from './Progress';

export interface Running {
  key: string;
  kind: string;
  label: string;
  /** What it's on now, or what it waits for. */
  detail?: string;
  done?: number;
  /** Unset or 0: size unknown, the bar runs without a fill. */
  total?: number;
  eta_s: number | null;
  stop?: () => void;
  testid: string;
}

/** Engine tasks outside the Queue that the person started and watches, by `progress` kind. */
export const FOREGROUND: Record<string, string> = { cull: 'AI cull', auto: 'Auto', rank_presets: 'Presets' };

export function runningIcon(kind: string, size = 12, className?: string): ReactNode {
  const p = { size, className };
  if (kind === 'import') return <Upload {...p} />;
  if (kind === 'export') return <Download {...p} />;
  if (kind === 'photos') return <Images {...p} />;
  if (kind === 'data') return <FileOutput {...p} />;
  if (kind === 'render_previews') return <Image {...p} />;
  return <Sparkles {...p} />;
}

/** Re-renders each second while anything runs, so ETAs count down between steps. */
function useTick(on: boolean) {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!on) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [on]);
}

/**
 * What's running, the work the person waits on first: Import, the window's own work
 * (Export, Apple Photos), AI culls and Auto, then the Queue's job. `queue: false` leaves the
 * Queue's job out for callers that draw it their own way (the Status bar's "+N queued").
 */
export function useRunning(q: QueueStatus | null, { queue = true } = {}): Running[] {
  const { state } = useLibrary();
  const app = useAppWork();
  useTick(app.length > 0);
  const out: Running[] = [];
  if (state.importing && state.importTotal) {
    const copying = (state.importLabel ?? 'Importing').startsWith('Importing');
    out.push({
      key: 'import',
      kind: 'import',
      label: copying ? 'Importing' : (state.importLabel ?? 'Importing'),
      done: state.importDone ?? state.imported,
      total: state.importTotal,
      eta_s: state.importEta ?? null,
      stop: LIVE && copying ? () => void call('cancel_import').then(() => toast('Import stopped: the photos already in stay', { id: 'argus' })) : undefined,
      testid: 'status-import',
    });
  }
  for (const w of app) {
    const slept = sleptNote(w.slept);
    const detail = [w.detail, slept].filter(Boolean).join(' · ') || undefined;
    out.push({ key: `app-${w.id}`, kind: w.kind, label: w.label, detail, done: w.done, total: w.total, eta_s: workEta(w), stop: w.stop, testid: `status-${w.kind}` });
  }
  for (const t of q?.tasks ?? []) {
    if (!FOREGROUND[t.kind]) continue;
    out.push({
      key: `task-${t.kind}`,
      kind: t.kind,
      label: t.kind === 'auto' ? t.label : `${FOREGROUND[t.kind]} · ${t.label}`,
      detail: t.waiting ? `Waiting: ${plain(t.waiting)}` : undefined,
      done: t.done,
      total: t.total,
      eta_s: t.waiting ? null : t.eta_s,
      stop: LIVE ? () => void call('cancel_task', { kind: t.kind }) : undefined,
      testid: 'status-task',
    });
  }
  const r = q?.running;
  if (queue && r) {
    out.push({
      key: `job-${r.id}`,
      kind: r.kind,
      label: r.label,
      detail: r.waiting ? `Waiting: ${plain(r.waiting)}` : r.stage && r.stage !== r.label ? r.stage : (sleptOver(r.started_at) ?? undefined),
      done: r.done,
      total: r.total,
      eta_s: r.waiting || q?.paused ? null : (q?.eta_s ?? null),
      stop: LIVE ? () => void call('cancel_job', { job_id: r.id }) : undefined,
      testid: 'running-job',
    });
  }
  return out;
}

/** The note for a Queue job that was running when the Mac last slept, for 10 minutes after. */
function sleptOver(startedAt: string | null | undefined): string | null {
  const w = lastWake();
  if (!w || !startedAt || Date.now() - w.at > 10 * 60_000) return null;
  return Date.parse(startedAt) < w.at - w.ms ? sleptNote(w.ms) : null;
}

/** One row of Work mode's Running card: dot, label, bar, count, time left, Stop. */
export function RunningRow({ w }: { w: Running }) {
  const sized = (w.total ?? 0) > 0;
  return (
    <div className="wm-job wm-run" data-testid="work-running" data-kind={w.kind}>
      <span className="wm-dot on" />
      <span className="trunc" title={w.detail ? `${w.label} · ${w.detail}` : w.label}>
        {w.label}
        {w.detail && <span className="faint"> · {w.detail}</span>}
      </span>
      <span className={`job-bar ${sized ? '' : 'indeterminate'}`}>
        <span style={sized ? { width: `${Math.min(100, (100 * (w.done ?? 0)) / w.total!)}%` } : undefined} />
      </span>
      <span className="mono faint wm-num">{sized ? `${w.done}/${w.total}` : ''}</span>
      <span className="faint wm-num">{eta(w.eta_s) ?? ''}</span>
      {w.stop ? (
        <button className="job-x" title="Stop" aria-label={`Stop ${w.label}`} onClick={w.stop} data-testid="work-running-stop">
          <X size={11} />
        </button>
      ) : (
        <span />
      )}
    </div>
  );
}
