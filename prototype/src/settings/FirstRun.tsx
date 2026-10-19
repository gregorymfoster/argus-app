// Before the app opens on a Library: first launch ("where should the Library live?"),
// or the remembered Library is on a disk that isn't connected. Lightroom asks the same
// when it can't find its catalog (ADR 0011).

import { useState } from 'react';
import { Aperture } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { pickFolder } from '../data/backend';
import { createLibrary, openLibrary, reloadOnLibrary, tidy, type LibraryInfo } from '../data/libraries';

export function FirstRun({ info }: { info: LibraryInfo }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lost = info.current != null && !info.exists;

  async function go(f: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await f();
      reloadOnLibrary();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setBusy(false);
    }
  }
  const pickThen = (title: string, f: (dir: string) => Promise<unknown>) =>
    go(async () => {
      const dir = await pickFolder(title);
      if (!dir) throw new Error('No folder chosen');
      await f(dir);
    });

  return (
    <div className="first-run" data-testid="first-run">
      <div className="first-run-card">
        <div className="brand">
          <Aperture size={18} strokeWidth={1.6} />
          Argus
        </div>
        {lost ? (
          <>
            <h1>Your Library can't be found</h1>
            <p className="muted">
              It was at <span className="mono">{tidy(info.current)}</span>. If it's on an external disk, connect it and try again.
            </p>
            <div className="first-run-actions">
              <Button size="lg" variant="default" onClick={() => location.reload()} disabled={busy}>
                Try again
              </Button>
              <Button size="lg" onClick={() => pickThen('Where is the Library?', openLibrary)} disabled={busy}>
                Locate Library…
              </Button>
              <Button size="lg" variant="ghost" onClick={() => pickThen('Where should the new Library go?', createLibrary)} disabled={busy}>
                New Library…
              </Button>
            </div>
          </>
        ) : (
          <>
            <h1>Where should Argus keep your Library?</h1>
            <p className="muted">
              One folder holds everything Argus makes: photos copied from cards, previews, edits and backups. You can move it to another disk or Mac later.
            </p>
            <div className="first-run-path mono" data-testid="suggested-library">
              {tidy(info.suggested)}
            </div>
            <div className="first-run-actions">
              <Button size="lg" variant="default" onClick={() => go(() => createLibrary(info.suggested))} disabled={busy} data-testid="create-suggested">
                Create Library here
              </Button>
              <Button size="lg" onClick={() => pickThen('Where should the Library go?', createLibrary)} disabled={busy}>
                Choose another folder…
              </Button>
              <Button size="lg" variant="ghost" onClick={() => pickThen('Open a Library', openLibrary)} disabled={busy}>
                Open existing Library…
              </Button>
            </div>
            {info.recent.length > 0 && <p className="faint">Recent: {info.recent.map((r) => tidy(r.path)).join(', ')}</p>}
          </>
        )}
        {error && <p className="lib-error">{error}</p>}
      </div>
    </div>
  );
}
