// Settings › AI › Search: search by meaning (engine `meaning_model`). Its model is a one-time
// 407 MB download into the models folder; until then ⌘K finds Photos by words alone. While
// it downloads the row is a bar with MB and an ETA from the recent rate.

import { useEffect, useRef, useState } from 'react';
import { Check, Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { toastFailure } from '@/lib/utils';
import { call } from '../data/backend';
import { LIVE } from '../data/live';
import { bytes } from '../data/libraries';
import { refreshPolls, usePoll } from '../data/poll';
import { ProgressBar } from '../ui/Progress';
import { Row } from './Row';

export interface MeaningStatus {
  installed: boolean;
  bytes: number;
  have_bytes: number;
  folder: string;
}

const DEMO: MeaningStatus = { installed: false, bytes: 407_000_000, have_bytes: 0, folder: '~/Library/Application Support/Argus/models' };
const MB = 1 << 20;

export function MeaningRow() {
  const [started, setStarted] = useState(false);
  const [demo, setDemo] = useState(DEMO);
  const live = usePoll<MeaningStatus>(LIVE ? 'meaning_model' : null, {}, started ? 1000 : 15000);
  const s = LIVE ? live : demo;
  const busy = !!s && !s.installed && (started || (s.have_bytes > 0 && s.have_bytes < s.bytes));
  const eta = useEta(busy ? s?.have_bytes : undefined, s?.bytes);

  // Dummy data: a pretend download, so the prototype shows the bar.
  useEffect(() => {
    if (LIVE || !started || demo.installed) return;
    const t = setInterval(() => setDemo((d) => (d.have_bytes + 9 * MB >= d.bytes ? { ...d, have_bytes: d.bytes, installed: true } : { ...d, have_bytes: d.have_bytes + 9 * MB })), 250);
    return () => clearInterval(t);
  }, [started, demo.installed]);
  useEffect(() => {
    if (s?.installed) setStarted(false);
  }, [s?.installed]);

  const start = async () => {
    setStarted(true);
    if (!LIVE) return;
    try {
      await call('meaning_model', { download: true });
      refreshPolls('meaning_model');
    } catch (e) {
      setStarted(false);
      toastFailure("Couldn't start the download")(e);
    }
  };

  if (!s) return null;
  if (busy)
    return (
      <div className="set-row" data-testid="settings-meaning">
        <div className="set-text">
          <ProgressBar label={`Downloading search by meaning · ${bytes(s.have_bytes)} of ${bytes(s.bytes)}`} done={Math.floor(s.have_bytes / MB)} total={Math.ceil(s.bytes / MB)} eta={eta} count={false} testId="meaning-progress" />
        </div>
      </div>
    );
  return (
    <Row title="Search by meaning" note={s.installed ? 'Installed. ⌘K finds photos that look like your words, even with no Keywords. Runs on this Mac.' : `Find photos that look like your words, even with no Keywords. A one-time ${bytes(s.bytes)} download; it runs on this Mac and nothing leaves it.`}>
      {s.installed ? (
        <span className="set-status" data-testid="settings-meaning">
          <Check size={13} /> Installed
        </span>
      ) : (
        <Button size="sm" onClick={() => void start()} data-testid="settings-meaning">
          <Download size={13} /> Download
        </Button>
      )}
    </Row>
  );
}

/** Seconds left at the rate of the last half minute or so. */
function useEta(have: number | undefined, total: number | undefined): number | null {
  const seen = useRef<[number, number][]>([]);
  if (have == null || !total) {
    seen.current = [];
    return null;
  }
  const now = Date.now();
  const last = seen.current[seen.current.length - 1];
  if (!last || last[1] !== have) seen.current.push([now, have]);
  seen.current = seen.current.filter(([t]) => now - t < 30_000);
  const first = seen.current[0];
  if (!first || first === seen.current[seen.current.length - 1]) return null;
  const rate = (have - first[1]) / ((now - first[0]) / 1000);
  return rate > 0 ? (total - have) / rate : null;
}
