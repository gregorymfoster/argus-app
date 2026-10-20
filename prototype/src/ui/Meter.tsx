// The Resource meter (engine/src/meter.rs): what the AI is taking from the Mac, as one
// quiet line under Background work, and a panel with each group's CPU, memory, CPU power and
// GPU over the last 10 minutes and since launch (Greg, 2026-10-20).

import { useState } from 'react';
import { Gauge } from 'lucide-react';
import { usePoll } from '../data/poll';
import { LIVE } from '../data/live';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';

type Key = 'local_model' | 'claude' | 'argus';
interface Rates {
  cpu_pct: number;
  mem_bytes: number;
  watts: number | null;
  gpu_pct: number | null;
  procs: number;
}
type Groups = Record<Key, Rates>;
interface Used {
  cpu_s: number;
  energy_j: number;
  gpu_s: number;
}
export interface Meter {
  now: Groups;
  history: (Groups & { at: number })[];
  totals: Record<Key, Used>;
  mem_total_bytes: number;
  energy: boolean;
  log: string | null;
}

const NAME: Record<Key, string> = { local_model: 'Local model', claude: 'Claude', argus: 'Argus' };
const zero: Rates = { cpu_pct: 0, mem_bytes: 0, watts: null, gpu_pct: null, procs: 0 };

/** Local model and Claude together: "the AI". */
function ai(g: Groups): Rates {
  const a = g.local_model ?? zero;
  const b = g.claude ?? zero;
  const add = (x: number | null, y: number | null) => (x == null && y == null ? null : (x ?? 0) + (y ?? 0));
  return { cpu_pct: a.cpu_pct + b.cpu_pct, mem_bytes: a.mem_bytes + b.mem_bytes, watts: add(a.watts, b.watts), gpu_pct: add(a.gpu_pct, b.gpu_pct), procs: a.procs + b.procs };
}

const gb = (b: number) => (b >= 10e9 ? `${Math.round(b / 1e9)} GB` : b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.round(b / 1e6)} MB`);
const pct = (p: number) => `${p < 10 ? p.toFixed(1) : Math.round(p)}%`;
const watts = (w: number | null) => (w == null ? '–' : `${w < 10 ? w.toFixed(1) : Math.round(w)} W`);
const wh = (j: number) => (j >= 3600 ? `${(j / 3600).toFixed(1)} Wh` : `${Math.round(j)} J`);
const mins = (s: number) => (s >= 60 ? `${Math.round(s / 60)} min` : `${Math.round(s)} s`);

/** Busy enough to call working: a few % of a core or of the GPU. */
const busy = (r: Rates) => r.cpu_pct > 5 || (r.gpu_pct ?? 0) > 3;

// The demo: a model runner partway through a cull for 10 minutes.
function demo(): Meter {
  const t0 = Date.now() / 1000;
  const at = (i: number) => t0 - (299 - i) * 2;
  const g = (i: number): Groups => {
    const w = Math.sin(i / 9) * 0.18 + (i > 240 ? 0.1 : 0);
    const on = i > 30;
    return {
      local_model: on ? { cpu_pct: 118 * (1 + w), mem_bytes: 21.4e9, watts: 1.4 * (1 + w), gpu_pct: 84 * (1 + w / 2), procs: 2 } : { cpu_pct: 0.4, mem_bytes: 21.4e9, watts: 0.2, gpu_pct: 0, procs: 2 },
      claude: zero,
      argus: { cpu_pct: 3 + Math.abs(Math.sin(i / 4)) * 6, mem_bytes: 412e6, watts: 0.6, gpu_pct: 1.2, procs: 1 },
    };
  };
  const history = Array.from({ length: 300 }, (_, i) => ({ ...g(i), at: at(i) }));
  return {
    now: g(299),
    history,
    totals: { local_model: { cpu_s: 734, energy_j: 18_900, gpu_s: 512 }, claude: { cpu_s: 0, energy_j: 0, gpu_s: 0 }, argus: { cpu_s: 41, energy_j: 380, gpu_s: 6 } },
    mem_total_bytes: 64e9,
    energy: true,
    log: '~/Library/Logs/Argus/debug.jsonl',
  };
}

/** The meter: every 2 s while the panel is open (the engine samples faster while someone
 *  looks), every 10 s for the sidebar line with only the points its sparkline draws. */
export function useMeter(open: boolean): Meter | null {
  const m = usePoll<Meter>(LIVE ? 'resource_usage' : null, open ? { watch: true } : { watch: false, points: 90 }, open ? 2000 : 10000);
  const [d] = useState(() => (LIVE ? null : demo()));
  return LIVE ? (m ?? null) : d;
}

/** A tiny line of one number over time. */
function Spark({ values, max, w = 64, h = 14 }: { values: number[]; max: number; w?: number; h?: number }) {
  if (values.length < 2) return <svg width={w} height={h} className="meter-spark" aria-hidden="true" />;
  const top = Math.max(max, 1e-6);
  const pts = values.map((v, i) => `${((i / (values.length - 1)) * w).toFixed(1)},${(h - 1 - (Math.min(v, top) / top) * (h - 2)).toFixed(1)}`).join(' ');
  return (
    <svg width={w} height={h} className="meter-spark" aria-hidden="true">
      <polyline points={`0,${h} ${pts} ${w},${h}`} className="area" />
      <polyline points={pts} className="line" />
    </svg>
  );
}

/** What a sparkline shows: GPU where the Mac reports it (a local model runs there), else CPU. */
const metric = (m: Meter) => (m.energy ? (r: Rates) => r.gpu_pct ?? 0 : (r: Rates) => r.cpu_pct);

/** One quiet line at the foot of the sidebar; opens the panel. */
export function MeterLine() {
  const [open, setOpen] = useState(false);
  const m = useMeter(open);
  if (!m) return null;
  const a = ai(m.now);
  const on = busy(a);
  const f = metric(m);
  const series = m.history.slice(-90).map((p) => f(ai(p)));
  return (
    <MeterPanel m={m} open={open} onOpenChange={setOpen}>
      <button className={`meter-line ${on ? 'on' : ''}`} data-testid="meter" title="What the AI is using on this Mac">
        <Gauge size={12} className="meter-icon" />
        {on ? (
          // Room for two numbers: CPU, and GPU where the Mac reports it (memory is in the panel).
          <span className="mono meter-nums" title={`${pct(a.cpu_pct)} CPU · ${a.gpu_pct != null ? `${pct(a.gpu_pct)} GPU · ` : ''}${gb(a.mem_bytes)}`}>
            AI {pct(a.cpu_pct)} · {a.gpu_pct != null ? `GPU ${pct(a.gpu_pct)}` : gb(a.mem_bytes)}
          </span>
        ) : (
          <span className="mono meter-nums faint">AI idle{a.mem_bytes > 0 ? ` · ${gb(a.mem_bytes)} loaded` : ''}</span>
        )}
        <span className="grow" />
        <Spark values={series} max={Math.max(...series, 50)} w={28} h={12} />
      </button>
    </MeterPanel>
  );
}

function MeterPanel({ m, open, onOpenChange, children }: { m: Meter; open: boolean; onOpenChange: (open: boolean) => void; children: React.ReactNode }) {
  const f = metric(m);
  const keys: Key[] = ['local_model', 'claude', 'argus'];
  const peak = Math.max(...m.history.flatMap((p) => keys.map((k) => f(p[k] ?? zero))), 50);
  const aiUsed = (k: keyof Used) => m.totals.local_model[k] + m.totals.claude[k];
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="start" side="top" className="meter-panel w-[400px] p-0" data-testid="meter-panel">
        <div className="qp-head">
          <span className="qp-title">AI usage</span>
          <span className="mono faint">last 10 min · {m.energy ? 'GPU' : 'CPU'}</span>
        </div>
        <table className="meter-table">
          <thead>
            <tr>
              <th />
              <th>CPU</th>
              <th>Memory</th>
              {m.energy && <th title="CPU energy only: macOS doesn’t split GPU power by process">CPU W</th>}
              {m.energy && <th>GPU</th>}
              <th />
            </tr>
          </thead>
          <tbody>
            {keys.map((k) => {
              const r = m.now[k] ?? zero;
              return (
                <tr key={k} className={busy(r) ? 'on' : ''} data-testid={`meter-${k}`}>
                  <td className="name">{NAME[k]}</td>
                  <td className="mono">{r.procs ? pct(r.cpu_pct) : '–'}</td>
                  <td className="mono">{r.procs ? gb(r.mem_bytes) : '–'}</td>
                  {m.energy && <td className="mono">{r.procs ? watts(r.watts) : '–'}</td>}
                  {m.energy && <td className="mono">{r.procs && r.gpu_pct != null ? pct(r.gpu_pct) : '–'}</td>}
                  <td>
                    <Spark values={m.history.map((p) => f(p[k] ?? zero))} max={peak} w={72} h={14} />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <p className="qp-note meter-foot">
          Since launch the AI used {mins(aiUsed('cpu_s'))} of CPU
          {m.energy && `, ${mins(aiUsed('gpu_s'))} of GPU and ${wh(aiUsed('energy_j'))} of CPU energy (macOS doesn’t split GPU power by process)`}. CPU is per core (400% is four cores); memory is of {gb(m.mem_total_bytes)}.
        </p>
        {m.log && (
          <p className="qp-note meter-foot mono" title="Every command, job and model request, with timings">
            Debug log: {m.log.replace(/^\/Users\/[^/]+/, '~')}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}
