// Settings › Library: where the Library lives (open, new, move, recent), what it takes on
// disk, clearing the cache, preview size, and finding missing originals. Lightroom's
// Catalog Settings and File Handling, in one page (ADR 0011).

import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { pickFolder } from '../data/backend';
import { LIVE } from '../data/live';
import {
  bytes,
  clearCache,
  createLibrary,
  forgetLibrary,
  libraryInfo,
  moveLibrary,
  openLibrary,
  rebuildPreviews,
  relinkOriginals,
  reloadOnLibrary,
  setLibrarySettings,
  storage,
  tidy,
  type LibraryInfo,
  type Storage,
} from '../data/libraries';
import { Seg } from '../ui/kit';
import { RemovedSection } from './RemovedSection';

const SIZES = [
  { value: '1024', label: '1024' },
  { value: '2048', label: '2048' },
  { value: '4096', label: '4096' },
];
const QUALITIES = [
  { value: '60', label: 'Low' },
  { value: '75', label: 'Medium' },
  { value: '85', label: 'High' },
];

function demoOnly() {
  toast('The prototype has one sample Library');
}

export function LibrarySection() {
  const [info, setInfo] = useState<LibraryInfo | null>(null);
  const [disk, setDisk] = useState<Storage | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [edge, setEdge] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [i, s] = await Promise.all([libraryInfo(), storage()]);
      setInfo(i);
      setDisk(s);
      setEdge((e) => e ?? s.settings.preview_long_edge);
    } catch (e) {
      setError(String(e));
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function run(label: string, f: () => Promise<void>) {
    setBusy(label);
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(null);
    }
  }

  const open = () =>
    LIVE
      ? run('Opening', async () => {
          const dir = await pickFolder('Open a Library');
          if (!dir) return;
          await openLibrary(dir);
          reloadOnLibrary();
        })
      : demoOnly();
  const create = () =>
    LIVE
      ? run('Creating', async () => {
          const dir = await pickFolder('Where should the new Library go?');
          if (!dir) return;
          await createLibrary(dir);
          reloadOnLibrary();
        })
      : demoOnly();
  const move = () =>
    LIVE
      ? run('Moving the Library', async () => {
          const dir = await pickFolder('Move the Library to…');
          if (!dir) return;
          const r = await moveLibrary(dir);
          toast(`Library moved to ${tidy(r.moved.to)}`);
          reloadOnLibrary();
        })
      : demoOnly();
  const switchTo = (path: string) => (LIVE ? run('Opening', async () => (await openLibrary(path), reloadOnLibrary())) : demoOnly());
  const relink = () =>
    LIVE
      ? run('Looking for originals', async () => {
          const dir = await pickFolder('Where are the missing originals?');
          if (!dir) return;
          const r = await relinkOriginals(dir);
          toast(r.relinked ? `Found ${r.relinked} originals` : 'No missing originals in that folder');
          await refresh();
        })
      : demoOnly();

  if (!info || !disk) return <p className="muted">{error ?? 'Reading the Library…'}</p>;

  const s = disk.settings;
  const parts = [
    { label: 'Originals in the Library', u: disk.originals_in_library, color: 'var(--text)' },
    { label: 'Previews', u: disk.previews, color: 'var(--pick)' },
    { label: 'Cache', u: disk.cache, color: 'var(--shoot)' },
    { label: 'Edits and backups', u: { bytes: disk.database.bytes + disk.backups.bytes, files: 0 }, color: 'var(--done)' },
  ];
  const whole = disk.total.bytes + (disk.free_bytes ?? 0) || 1;
  const others = info.recent.filter((r) => r.path !== info.current);
  const previewsChanged = edge != null && edge !== s.preview_long_edge;

  return (
    <div className="lib-settings" data-testid="library-settings">
      <h3>Location</h3>
      <div className="lib-path">
        <span className="mono" data-testid="library-path" title={info.current ?? ''}>
          {tidy(info.current)}
        </span>
        <span className="grow" />
        <Button size="sm" onClick={open} disabled={!!busy}>
          Open…
        </Button>
        <Button size="sm" onClick={create} disabled={!!busy}>
          New…
        </Button>
        <Button size="sm" onClick={move} disabled={!!busy} data-testid="move-library">
          Move…
        </Button>
      </div>
      <p className="faint lib-note">Everything Argus makes lives in this folder. Move it to another disk or Mac and it opens there.</p>
      {others.length > 0 && (
        <div className="lib-recent">
          {others.map((r) => (
            <div key={r.path} className="lib-row">
              <span className={`mono ${r.exists ? '' : 'faint'}`}>{tidy(r.path)}</span>
              {!r.exists && <span className="faint">not connected</span>}
              <span className="grow" />
              {r.exists && (
                <Button size="sm" variant="ghost" onClick={() => switchTo(r.path)} disabled={!!busy}>
                  Switch
                </Button>
              )}
              <Button size="sm" variant="ghost" onClick={() => void forgetLibrary(r.path).then(setInfo)} disabled={!!busy}>
                Forget
              </Button>
            </div>
          ))}
        </div>
      )}

      <h3>Disk</h3>
      <div className="lib-bar" role="img" aria-label="Disk use">
        {parts.map((p) => (
          <span key={p.label} style={{ width: `${(p.u.bytes / whole) * 100}%`, background: p.color }} />
        ))}
      </div>
      <div className="lib-grid">
        {parts.map((p) => (
          <div key={p.label} style={{ display: 'contents' }}>
            <span className="lib-swatch" style={{ background: p.color }} />
            <span>{p.label}</span>
            <span className="mono">{bytes(p.u.bytes)}</span>
          </div>
        ))}
        <span className="lib-swatch lib-free" />
        <span className="muted">Free on this disk</span>
        <span className="mono muted" data-testid="free-space">
          {bytes(disk.free_bytes)}
        </span>
      </div>
      {disk.originals_elsewhere.files > 0 && (
        <p className="faint lib-note">
          {disk.originals_elsewhere.files} originals ({bytes(disk.originals_elsewhere.bytes)}) are read from folders outside the Library and aren't counted.
        </p>
      )}
      {disk.missing > 0 && (
        <div className="lib-row lib-warn">
          <span>
            {disk.missing} originals can't be found. Their previews still work; edits and exports need the originals.
          </span>
          <span className="grow" />
          <Button size="sm" onClick={relink} disabled={!!busy}>
            Locate…
          </Button>
        </div>
      )}
      <div className="lib-row">
        <span>Cache</span>
        <span className="faint">Rebuildable files Argus can delete any time</span>
        <span className="grow" />
        <Button
          size="sm"
          onClick={() =>
            run('Clearing', async () => {
              const freed = await clearCache();
              toast(`Freed ${bytes(freed.bytes)}`);
              await refresh();
            })
          }
          disabled={!!busy || disk.cache.bytes === 0}
          data-testid="clear-cache"
        >
          Clear {bytes(disk.cache.bytes)}
        </Button>
      </div>

      <h3>Previews</h3>
      <div className="lib-row">
        <span>Size</span>
        <span className="grow" />
        <Seg
          label="Preview size"
          value={String(edge ?? s.preview_long_edge)}
          onChange={(v) => setEdge(Number(v))}
          items={SIZES}
        />
      </div>
      <div className="lib-row">
        <span>Quality</span>
        <span className="grow" />
        <Seg
          label="Preview quality"
          value={String(QUALITIES.reduce((a, q) => (Math.abs(+q.value - s.preview_quality) < Math.abs(+a.value - s.preview_quality) ? q : a)).value)}
          onChange={(v) => void run('Saving', async () => (await setLibrarySettings({ preview_quality: Number(v) }), await refresh()))}
          items={QUALITIES}
        />
      </div>
      {previewsChanged ? (
        <div className="lib-row lib-warn">
          <span>
            New imports use {edge} px. Rebuilding the {disk.photos} existing previews takes about {Math.max(1, Math.round(disk.photos / 60))} min.
          </span>
          <span className="grow" />
          <Button size="sm" variant="ghost" onClick={() => setEdge(s.preview_long_edge)}>
            Cancel
          </Button>
          <Button
            size="sm"
            onClick={() =>
              run(`Rebuilding ${disk.photos} previews`, async () => {
                await setLibrarySettings({ preview_long_edge: edge! });
                if (LIVE) await rebuildPreviews();
                await refresh();
              })
            }
            disabled={!!busy}
          >
            Apply and rebuild
          </Button>
        </div>
      ) : (
        <p className="faint lib-note">Smaller previews save disk; larger ones look sharper when zoomed. About {bytes(disk.previews.files ? disk.previews.bytes / disk.previews.files : 0)} each now.</p>
      )}
      <RemovedSection />
      {busy && (
        <div className="lib-busy" role="status">
          <span className="lib-progress" />
          {busy}…
        </div>
      )}
      {error && <p className="lib-error">{error}</p>}
    </div>
  );
}
