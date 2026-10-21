// Waiting is never a bare spinner (docs/design.md): a bar, a count and an ETA when the
// engine knows how far along it is, and an honest estimate when it only knows how long
// such a step usually takes. Reuse these anywhere the app waits.

import { useEffect, useRef, useState } from 'react';
import { usePoll } from '../data/poll';

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

/** "about 40s", "about 3 min". */
export function eta(seconds: number | null | undefined): string | null {
  if (seconds == null || !isFinite(seconds)) return null;
  if (seconds < 5) return 'a few seconds';
  if (seconds < 90) return `about ${Math.round(seconds / 5) * 5}s`;
  return `about ${Math.round(seconds / 60)} min`;
}

/**
 * A progress bar. With `done`/`total` it fills to match; without, it shows a calm moving
 * sheen (still never a spinner). `eta` is seconds left. `count={false}` leaves out "n of
 * total" where the units mean nothing to the person (model tiles) or space is tight.
 */
export function ProgressBar({ label, done, total, eta: left, testId, count = true }: { label: string; done?: number; total?: number; eta?: number | null; testId?: string; count?: boolean }) {
  const known = total != null && total > 0 && done != null;
  const pct = known ? Math.min(100, (done! / total!) * 100) : 0;
  const leftText = eta(left);
  return (
    <div className="progress" role="progressbar" aria-label={label} aria-valuenow={known ? Math.round(pct) : undefined} aria-valuemin={0} aria-valuemax={100} data-testid={testId}>
      <div className="progress-head">
        <span>{label}</span>
        <span className="mono faint">
          {known && count ? `${done} of ${total}` : ''}
          {known && count && leftText ? ' · ' : ''}
          {leftText ? `${leftText} left` : ''}
        </span>
      </div>
      <div className={`progress-track ${known ? '' : 'indeterminate'}`}>
        <div style={known ? { width: `${pct}%` } : undefined} />
      </div>
    </div>
  );
}

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

