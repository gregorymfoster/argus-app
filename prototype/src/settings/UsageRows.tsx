// Settings › General › Training data: Argus records each Photo's start, the AI's takes and what
// the person kept and exported, in the Library only (engine usage_log, docs/usage-log.md).
// Export writes it as bench cases for bench/score.py.

import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';
import { call, pickFolder } from '../data/backend';
import { LIVE } from '../data/live';
import { Row } from './Row';

type Summary = { recording: boolean; events: number; photos: number; takes: number; exports: number; since: string | null };
const DEMO: Summary = { recording: true, events: 412, photos: 96, takes: 188, exports: 31, since: '2026-10-18T09:12:00Z' };

export function UsageRows() {
  const [s, setS] = useState<Summary | null>(LIVE ? null : DEMO);
  const [confirm, setConfirm] = useState(false);
  const run = useCallback(async (args: Record<string, unknown> = {}) => {
    if (!LIVE) return setS((p) => ({ ...(p ?? DEMO), ...(args.recording !== undefined ? { recording: !!args.recording } : {}), ...(args.clear ? { events: 0, photos: 0, takes: 0, exports: 0, since: null } : {}) }));
    try {
      setS(await call<Summary>('usage_log', args));
    } catch (e) {
      toast.error(`Training data: ${(e as Error).message}`);
    }
  }, []);
  useEffect(() => void run(), [run]);
  const exportBench = async () => {
    const dir = await pickFolder('Export training data to…');
    if (!dir || !LIVE) return;
    const t = toast.loading('Exporting training data…');
    try {
      const r = await call<{ edit_cases: number; cull_shoots: number }>('export_usage_bench', { folder: dir });
      toast.success(`Exported ${r.edit_cases} edit cases and ${r.cull_shoots} culled Shoots`, { id: t });
    } catch (e) {
      toast.error(`Export failed: ${(e as Error).message}`, { id: t });
    }
  };
  const since = s?.since ? new Date(s.since).toLocaleDateString() : null;
  return (
    <>
      <h3>Training data</h3>
      <Row title="Record my edits to improve Argus" note="Where each Photo started, the AI's takes, which one you kept and what you exported. Stays in this Library; nothing is uploaded.">
        <Switch checked={s?.recording ?? true} onCheckedChange={(c) => void run({ recording: c })} aria-label="Record my edits to improve Argus" data-testid="settings-record-usage" />
      </Row>
      <Row title="Recorded" note={s ? `${s.photos} Photos, ${s.takes} AI takes, ${s.exports} exports${since ? ` since ${since}` : ''}.` : 'Reading…'}>
        <button className="import-pick" onClick={() => void exportBench()} disabled={!s?.events} data-testid="settings-usage-export">
          Export…
        </button>
        <button
          className="import-pick"
          onClick={() => (confirm ? (setConfirm(false), void run({ clear: true })) : setConfirm(true))}
          onBlur={() => setConfirm(false)}
          disabled={!s?.events}
          data-testid="settings-usage-clear"
        >
          {confirm ? `Clear ${s?.events ?? 0} events?` : 'Clear'}
        </button>
      </Row>
    </>
  );
}
