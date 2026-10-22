// Waiting is never a bare spinner (docs/design.md): a bar, a count and an ETA when the
// engine knows how far along it is, and an honest estimate when it only knows how long
// such a step usually takes. Reuse these anywhere the app waits.

import { useEffect, useRef, useState } from 'react';
import { usePoll } from '../data/poll';
import { call } from '../data/backend';

export interface EngineTask {
  kind: string;
  label: string;
  done: number;
  total: number;
  elapsed_s: number;
  eta_s: number | null;
  /** What it waits on instead of working ("`argus cull` is using the model"). */
  waiting?: string | null;
}

/** "a few seconds", "~40 s", "~3 min", "~2 h 34 min". */
export function eta(seconds: number | null | undefined): string | null {
  if (seconds == null || !isFinite(seconds)) return null;
  if (seconds < 5) return 'a few seconds';
  if (seconds < 60) return `~${Math.max(5, Math.round(seconds / 5) * 5)} s`;
  if (seconds < 3600) return `~${Math.round(seconds / 60)} min`;
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds - h * 3600) / 60);
  return m ? `~${h} h ${m} min` : `~${h} h`;
}

/**
 * One progress row, the same everywhere the app waits (Greg, 2026-10-21): what it's doing on
 * the left (one line, ellipsized), "3 of 266 · ~2 h 34 min" on the right in tabular figures,
 * Cancel tucked in after it, and a thin bar underneath. With `done`/`total` the bar fills (a
 * soft sheen runs over the filled part so a slow job still looks alive); without, the sheen
 * crosses the whole track (still never a spinner). `eta` is seconds left. `count={false}`
 * leaves out "n of total" where the units mean nothing to the person (model tiles).
 */
export function ProgressBar({ label, done, total, eta: left, testId, count = true, onCancel }: { label: string; done?: number; total?: number; eta?: number | null; testId?: string; count?: boolean; onCancel?: () => void }) {
  const known = total != null && total > 0 && done != null;
  const pct = known ? Math.min(100, (done! / total!) * 100) : 0;
  const leftText = eta(left);
  const meta = [known && count ? `${done} of ${total}` : '', leftText ? `${leftText}${leftText === 'a few seconds' ? '' : ' left'}` : ''].filter(Boolean).join(' · ');
  return (
    <div className="progress" role="progressbar" aria-label={label} aria-valuenow={known ? Math.round(pct) : undefined} aria-valuemin={0} aria-valuemax={100} data-testid={testId}>
      <div className="progress-head">
        <span className="progress-label" title={label}>
          {label}
        </span>
        {meta && <span className="progress-meta">{meta}</span>}
        {onCancel && (
          <button className="progress-cancel" onClick={onCancel} data-testid={testId ? `${testId}-cancel` : undefined}>
            Cancel
          </button>
        )}
      </div>
      <div className={`progress-track ${known ? '' : 'indeterminate'}`}>
        <div style={known ? { width: `${pct}%` } : undefined} />
      </div>
    </div>
  );
}

/**
 * The engine task of this kind as a progress row, while it runs; `label` until it shows up
 * (or once it has ended and the caller is still reading the results back). With `cancel`,
 * Cancel asks the engine to stop it (`cancel_task`) and the row says it is stopping.
 */
export function TaskProgress({ kind, label, testId, cancel }: { kind: string; label: string; testId?: string; cancel?: boolean }) {
  const task = useEngineTask(kind, true);
  const [stopping, setStopping] = useState(false);
  const onCancel =
    cancel && task && !stopping
      ? () => {
          setStopping(true);
          void call('cancel_task', { kind }).catch(() => setStopping(false));
        }
      : undefined;
  if (!task) return <ProgressBar label={label} testId={testId} />;
  if (stopping) return <ProgressBar label="Stopping" done={task.done} total={task.total || undefined} testId={testId} />;
  // Queued behind another model user: say on whom, with no bar or ETA that pretends to move.
  if (task.waiting) return <ProgressBar label={`Waiting: ${task.waiting.replace(/`/g, '')}`} testId={testId} onCancel={onCancel} />;
  return <ProgressBar label={task.label} done={task.done} total={task.total || undefined} eta={task.eta_s} testId={testId} onCancel={onCancel} />;
}

/** The engine's error for work the person stopped (engine `progress::STOPPED`). */
export const wasStopped = (e: unknown) => /(^|: )stopped$/.test(String(e));

/** The engine's running task of this kind (`progress` command), polled while `active`. */
export function useEngineTask(kind: string, active: boolean): EngineTask | null {
  const r = usePoll<{ tasks: EngineTask[] }>(active ? 'progress' : null, {}, 400);
  return (active && r?.tasks.find((t) => t.kind === kind)) || null;
}

/**
 * For a step the engine can't measure (one model request): fill against how long it took
 * last time on this machine, never quite reaching the end, and remember the real time.
 */
export function useTimedEstimate(key: string, active: boolean, fallbackS: number) {
  const storeKey = `argus:took:${key}`;
  const expected = useRef(fallbackS);
  const started = useRef(0);
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (!active) return;
    try {
      expected.current = Number(localStorage.getItem(storeKey)) || fallbackS;
    } catch {
      expected.current = fallbackS;
    }
    started.current = performance.now();
    setElapsed(0);
    const t = setInterval(() => setElapsed((performance.now() - started.current) / 1000), 250);
    return () => {
      clearInterval(t);
      const took = (performance.now() - started.current) / 1000;
      if (took > 1)
        try {
          localStorage.setItem(storeKey, String(Math.round(took)));
        } catch {
          // no storage: the next estimate uses the fallback
        }
    };
  }, [active, storeKey, fallbackS]);
  const exp = expected.current;
  return { done: Math.min(95, Math.round((elapsed / exp) * 100)), total: 100, eta: Math.max(exp - elapsed, 1) };
}

