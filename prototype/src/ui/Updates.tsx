// "Restart to update", like Cursor and Claude (Greg, 2026-10-18): the release installs in
// the background, then a small button in the sidebar footer restarts into it, and a toast
// says so once on screens without the sidebar. Check for Updates… (menu, ⌘K, Settings)
// reports what it found. See src/data/updates.ts.

import { useEffect, useRef } from 'react';
import { ArrowUpCircle } from 'lucide-react';
import { toast } from 'sonner';
import { onMenuCheck, restartToUpdate, useUpdate, type UpdateStatus } from '../data/updates';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

/** The release notes, first few lines. */
function Notes({ s }: { s: UpdateStatus }) {
  const lines = (s.notes ?? '').split('\n').map((l) => l.trim()).filter(Boolean).slice(0, 6);
  return (
    <div className="update-notes">
      <div>
        Argus {s.version} <span className="faint">· you have {s.current}</span>
      </div>
      {lines.map((l, i) => (
        <div key={i} className="muted">
          {l}
        </div>
      ))}
    </div>
  );
}

/** Sidebar footer: shows only once a release is installed and waiting for a restart. */
export function UpdateButton() {
  const s = useUpdate();
  if (s?.state !== 'ready') return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button className="update-button" onClick={() => void restartToUpdate()} data-testid="update-restart">
          <ArrowUpCircle size={13} />
          Restart to update
          <span className="grow" />
          <span className="mono faint">{s.version}</span>
        </button>
      </TooltipTrigger>
      <TooltipContent side="right" className="max-w-[300px]">
        <Notes s={s} />
      </TooltipContent>
    </Tooltip>
  );
}

/** Toasts: once when a release is ready, and the outcome of a Check for Updates… by hand. */
export function useUpdateToasts(asked: { current: boolean }) {
  const s = useUpdate();
  const shown = useRef<string | null>(null);
  useEffect(() => onMenuCheck(() => (asked.current = true)), [asked]);
  useEffect(() => {
    if (!s) return;
    const restart = { label: 'Restart', onClick: () => void restartToUpdate() };
    if (s.state === 'ready' && shown.current !== s.version) {
      shown.current = s.version;
      asked.current = false;
      toast(`Argus ${s.version} is ready`, { id: 'update', description: 'Restart to update. Your work is saved.', action: restart, duration: 10000 });
      return;
    }
    if (!asked.current) return;
    if (s.state === 'checking') toast('Checking for updates…', { id: 'update' });
    else if (s.state === 'downloading') toast(`Downloading Argus ${s.version}`, { id: 'update', description: 'Keep working; a Restart button appears when it’s ready.' });
    else if (s.state === 'current') toast(`Argus ${s.current} is the newest version`, { id: 'update' });
    else if (s.state === 'error') toast('Couldn’t check for updates', { id: 'update', description: s.error ?? undefined });
    else if (s.state === 'off') toast('This build doesn’t update itself', { id: 'update', description: 'Dev and local builds have no update feed. Install from the download page.' });
    if (s.state !== 'checking' && s.state !== 'downloading') asked.current = false;
  }, [s, asked]);
}
