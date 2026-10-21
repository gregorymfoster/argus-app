// Import is a focused dialog over the grid (Greg's pick). Pressing Import
// starts reading and opens the Shoot at once; thumbnails fill in behind.

import { useCallback, useEffect, useRef, useState } from 'react';
import { PHOTOS, SHOOT } from '../data/dummy';
import { useLibrary } from '../store/library';
import { go } from '../router';
import { noSpaces, shootFolder } from '../domain/paths';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { ProgressBar, eta, useEngineTask, useTimedEstimate } from '../ui/Progress';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';
import { Check, ChevronDown, ChevronRight, Folder, HardDrive, Plus, RotateCw, Sparkles, X } from 'lucide-react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Kbd, MenuKeys, Seg } from '../ui/kit';
import { usePref } from '../store/variants';
import { bytes, libraryInfo, tidy } from '../data/libraries';
import { LIVE, PROVIDERS, chosenProvider, estimateImport, formatShootName, importSeconds, type ImportEstimate, findCards, liveImport, scanFolder, suggestName, thumbnail, type Card, type NameSuggestion, type Scan, type ScannedPhoto } from '../data/live';
import { pickFolder } from '../data/backend';
import { whileShown } from '../data/poll';

const AI_NAME_OFF = 'AI naming is off in Settings';

export function useImportRunner() {
  const { dispatch } = useLibrary();
  return () => {
    dispatch({ type: 'reset' });
    dispatch({ type: 'import/start' });
    let n = 0;
    const tick = () => {
      n += 4;
      dispatch({ type: 'import/progress', n });
      if (n < PHOTOS.length) setTimeout(tick, 70);
    };
    setTimeout(tick, 120);
  };
}

type ImportProps = { open: boolean; autoCull: boolean; setAutoCull: (v: boolean) => void };

export function ImportDialog(props: ImportProps) {
  return LIVE ? <LiveImportDialog {...props} /> : <DemoImportDialog {...props} />;
}

/**
 * The real import. Argus looks for a memory card (a mounted volume with a DCIM folder) and
 * scans it, or a folder Greg picks. Every photo found shows in a grid, already-imported
 * ones unticked; the local model suggests a Shoot name. A card's photos are copied into
 * the Library; a folder can be copied or left in place. Every wait shows a progress bar.
 */
function LiveImportDialog({ open, autoCull, setAutoCull }: ImportProps) {
  const [cards, setCards] = useState<Card[] | null>(null);
  const [folder, setFolder] = useState<string | null>(null);
  const [scan, setScan] = useState<Scan | null>(null);
  const [scanning, setScanning] = useState(false);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [name, setName] = useState('');
  const [typed, setTyped] = useState(false);
  const [naming, setNaming] = useState<'idle' | 'busy' | NameSuggestion>('idle');
  const [askAgain, setAskAgain] = useState(0);
  // Which photos the grid shows, like Lightroom's New Photos / All Photos (Greg's default: New).
  const [show, setShow] = useState<'new' | 'all'>('new');
  // Settings › Import sets where each choice starts; changing it here is for this Import only.
  const [showPref] = usePref<'new' | 'all'>('import-show', 'new');
  const [rawPref] = usePref<'1' | '0'>('import-raw-only', '1');
  const [copyPref] = usePref<'1' | '0'>('import-copy', '0');
  const [aiNamePref] = usePref<'1' | '0'>('import-ai-name', '1');
  // Menus here are controlled: the dialog sees Esc first (Radix keeps two layer stacks),
  // so it closes the open menu instead of itself.
  const [menu, setMenu] = useState<'source' | 'show' | null>(null);
  const menuProps = (m: 'source' | 'show') => ({ open: menu === m, onOpenChange: (o: boolean) => setMenu(o ? m : null) });
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  // RAW+JPEG frames are one Photo either way; RAW only (Greg's default) leaves the JPEG behind.
  const [rawOnly, setRawOnly] = useState(true);
  // Lightroom's Auto on every new Photo (off until Auto is scored against Greg's edits).
  const [autoToneDefault] = usePref<'1' | '0'>('import-auto', '0');
  const [autoTone, setAutoTone] = useState(autoToneDefault === '1');
  // Edit Suggestions after the Import (Settings › AI and Compute sets the default).
  const [suggestPref, setSuggest] = usePref<'1' | '0'>('import-suggest', '1');
  const suggest = suggestPref === '1';
  const [focused, setFocused] = useState<string | null>(null);
  const [thumb, setThumb] = useState(150);
  const [recent] = useState(recentFolders);
  const [libraryRoot, setLibraryRoot] = useState<string | null>(null);
  useEffect(() => {
    if (open) void libraryInfo().then((i) => setLibraryRoot(i.current), () => {});
  }, [open]);
  const typedRef = useRef(typed);
  typedRef.current = typed;
  // Cards always copy (they get unplugged); a folder can stay where it is, like Lightroom's Add.
  const [copyFolder, setCopyFolder] = useState(false);
  const [estimate, setEstimate] = useState<ImportEstimate | null>(null);
  const [savedAs, setSavedAs] = useState<string | null>(null);
  const isCard = !!folder && !!cards?.some((c) => c.path === folder);
  const copy = isCard || copyFolder;
  useEffect(() => {
    if (!open) return;
    setShow(showPref);
    setRawOnly(rawPref === '1');
    setCopyFolder(copyPref === '1');
    setAutoTone(autoToneDefault === '1');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  const scanTask = useEngineTask('scan', scanning);
  const importTask = useEngineTask('import', busy);
  const nameTime = useTimedEstimate('suggest-name', naming === 'busy' && !!scan, 35);

  const choose = useCallback(async (path: string) => {
    setFolder(path);
    setScan(null);
    setNote(null);
    // A new source gets its own name, unless Greg typed one.
    if (!typedRef.current) setName('');
    setNaming('busy');
    setScanning(true);
    try {
      const s = await scanFolder(path);
      setScan(s);
      setChosen(new Set(s.photos.filter((p) => !p.imported).map((p) => p.path)));
      if (s.photos.length === 0) setNote('No photos found there.');
    } catch (e) {
      setNote(String(e));
      setNaming('idle');
    } finally {
      setScanning(false);
    }
  }, []);

  // Look for cards while nothing is chosen, so plugging one in picks it up.
  useEffect(() => {
    if (!open || folder) return;
    let live = true;
    const look = () =>
      findCards()
        .then((found) => {
          if (!live) return;
          setCards(found);
          if (found.length > 0) choose(found[0].path);
        })
        .catch(() => live && setCards([]));
    look();
    const stop = whileShown(look, 3000);
    return () => {
      live = false;
      stop();
    };
  }, [open, folder, choose]);

  // Name the Shoot from what's new on the card, unless Greg has typed one.
  useEffect(() => {
    if (!scan) return;
    if (scan.photos.length === 0) return setNaming('idle');
    const fresh = scan.photos.filter((p) => !p.imported);
    // AI naming off: the date names it, and Name with AI still asks once.
    if (aiNamePref === '0' && askAgain === 0) return setNaming({ name: '', by: 'date', note: AI_NAME_OFF });
    let live = true;
    setNaming('busy');
    suggestName(fresh.length ? fresh : scan.photos)
      .then((s) => {
        if (!live) return;
        setNaming(s);
        setName((cur) => (typedRef.current && cur.trim() ? cur : s.name));
      })
      .catch(() => live && setNaming('idle'));
    return () => {
      live = false;
    };
  }, [scan, askAgain]);

  const photos = scan?.photos ?? [];
  const fresh = photos.filter((p) => !p.imported);
  const visible = show === 'new' ? fresh : photos;
  const files = visible.filter((p) => chosen.has(p.path)).map((p) => p.path);
  const setMany = (list: ScannedPhoto[], on: boolean) =>
    setChosen((cur) => {
      const next = new Set(cur);
      for (const p of list) on ? next.add(p.path) : next.delete(p.path);
      return next;
    });
  const setAll = (on: boolean) => setMany(visible, on);
  const firstCapture = photos.find((p) => p.captured_at)?.captured_at ?? null;

  // How much room the import takes on the Library's disk, against what's free.
  const filesKey = files.join('\n');
  useEffect(() => {
    if (!folder || files.length === 0) return setEstimate(null);
    let live = true;
    const t = setTimeout(() => {
      estimateImport(folder, files, copy, rawOnly)
        .then((e) => live && setEstimate(e))
        .catch(() => live && setEstimate(null));
    }, 250);
    return () => {
      live = false;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [folder, filesKey, copy, rawOnly]);

  // The name as it will be saved: the Shoot's date first, no spaces (engine naming rules).
  useEffect(() => {
    if (!name.trim()) return setSavedAs(null);
    let live = true;
    const t = setTimeout(() => {
      formatShootName(name, firstCapture)
        .then((n) => live && setSavedAs(n))
        .catch(() => live && setSavedAs(null));
    }, 200);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [name, firstCapture]);

  const pickOther = async () => {
    const picked = await pickFolder();
    if (picked) choose(picked);
  };
  const toggle = (path: string) =>
    setChosen((cur) => {
      const next = new Set(cur);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  const run = async () => {
    if (!folder || chosen.size === 0) return;
    setBusy(true);
    setNote(null);
    try {
      // Format what's in the field now, not the preview, which may lag a keystroke behind.
      const shoot = name.trim() ? await formatShootName(name, firstCapture).catch(() => name.trim()) : null;
      const r = await liveImport(folder, shoot, autoCull, files, rawOnly, copy, estimate?.original_bytes, autoTone, suggest);
      if (r.shoot_id == null) setNote(`Nothing new: all ${r.duplicates} photos are already in the Library.`);
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusy(false);
    }
  };
  const tooBig = !!estimate && estimate.free_bytes != null && estimate.needed_bytes > estimate.free_bytes;
  const card = cards?.find((c) => c.path === folder);
  const camera = scan?.photos[0]?.camera;
  const sourceName = folder ? (card ? card.name : tidy(folder)) : cards === null ? 'Looking for a memory card…' : 'No memory card';
  const sourceMeta = folder ? [camera, scan ? `${scan.photos.length} photos` : scanning ? 'reading…' : null].filter(Boolean).join(' · ') : cards === null ? '' : 'choose a folder';
  const groups = byDay(visible);
  const model = PROVIDERS.find((x) => x.id === chosenProvider());
  const namedBy = typeof naming === 'object' && naming.by === 'ai' && !typed ? `Named by ${modelName(model?.model ?? model?.label ?? 'AI')} from ${Math.min(4, fresh.length || photos.length)} photos` : null;
  const into = savedAs && libraryRoot ? `into ${tidy(libraryRoot)}/originals/${savedAs}/` : null;
  const canImport = !!folder && files.length > 0 && !busy && !scanning && !tooBig;

  // Remember folders Greg imports from, for the From menu.
  useEffect(() => {
    if (!scan || !folder || isCard) return;
    rememberFolder(folder, scan.photos.length);
  }, [scan, folder, isCard]);

  useHotkeys('mod+a', () => setAll(true), { enabled: open && !busy, preventDefault: true }, [visible]);
  useHotkeys('mod+o', () => void pickOther(), { enabled: open && !busy, preventDefault: true });
  useHotkeys('enter', () => canImport && void run(), { enabled: open && !menu, preventDefault: true }, [canImport, files, name]);
  useHotkeys('space', () => focused && toggle(focused), { enabled: open && !busy, preventDefault: true }, [focused]);

  // Pinch (or ⌃-scroll) on the grid resizes the thumbnails, as in the Cull grid.
  const gridRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = gridRef.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      setThumb((t) => Math.max(96, Math.min(320, t * Math.exp(-e.deltaY * 0.01))));
    };
    el.addEventListener('wheel', wheel, { passive: false });
    return () => el.removeEventListener('wheel', wheel);
  }, [photos.length]);

  return (
    <Dialog open={open} onOpenChange={(o) => !o && !busy && go({ name: 'cull' })}>
      {/* Cancel, ✕ and Esc close it; a stray click outside (the Mac's window-focus click) doesn't. */}
      <DialogContent
        className="import-v2 top-[6vh] h-[min(830px,88vh)] w-[min(1200px,calc(100%-48px))] gap-0 p-0"
        closable={false}
        data-testid="import-dialog"
        onPointerDownOutside={(e) => e.preventDefault()}
        onInteractOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => {
          if (!menu) return;
          e.preventDefault();
          setMenu(null);
        }}
      >
        <header className="imp-head">
          <DialogTitle className="imp-title">Import</DialogTitle>
          <DialogDescription className="sr-only">{copy ? 'Copies the photos you tick into your Library; the originals are never changed.' : 'Reads these photos where they are and never changes them.'}</DialogDescription>
          <span className="muted">From</span>
          <DropdownMenu {...menuProps('source')}>
            <DropdownMenuTrigger asChild>
              <button className="import-pick imp-source" disabled={busy} data-testid="import-source">
                {card ? <HardDrive size={13} className="ai-icon" /> : <Folder size={13} />}
                <span className="imp-source-name">{sourceName}</span>
                {sourceMeta && <span className="mono">{sourceMeta}</span>}
                <ChevronDown size={12} />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="min-w-[320px]" data-testid="import-from">
              <div className="menu-label">Cards</div>
              {(cards ?? []).length === 0 && <div className="menu-empty">No memory card found</div>}
              {(cards ?? []).map((c) => (
                <DropdownMenuItem key={c.path} onSelect={() => choose(c.path)} data-testid="import-card">
                  <HardDrive size={13} /> {c.name}
                  {c.path === folder && scan && <span className="mono faint">{[camera, `${scan.photos.length} photos`].filter(Boolean).join(' · ')}</span>}
                  <span className="grow" />
                  {c.path === folder && <Check size={13} />}
                </DropdownMenuItem>
              ))}
              {recent.length > 0 && <div className="menu-label">Recent folders</div>}
              {recent.map((r) => (
                <DropdownMenuItem key={r.path} onSelect={() => choose(r.path)} data-testid="import-recent">
                  <Folder size={13} /> <span className="mono imp-recent">{tidy(r.path)}</span>
                  <span className="grow" />
                  <span className="mono faint">{r.count}</span>
                </DropdownMenuItem>
              ))}
              <div className="menu-sep" />
              <DropdownMenuItem onSelect={pickOther} data-testid="import-folder">
                <Plus size={13} /> Choose folder… <MenuKeys keys={['⌘', 'O']} />
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          {folder && (
            <>
              <ChevronRight size={13} className="faint" />
              {isCard ? (
                <span className="muted">Copy into Library</span>
              ) : (
                <Seg
                  label="Copy or leave in place"
                  value={copyFolder ? 'copy' : 'add'}
                  onChange={(v) => setCopyFolder(v === 'copy')}
                  items={[
                    { value: 'add', label: 'Leave in place' },
                    { value: 'copy', label: 'Copy into Library' },
                  ]}
                />
              )}
            </>
          )}
          <span className="grow" />
          {photos.length > 0 && (
            <DropdownMenu {...menuProps('show')}>
              <DropdownMenuTrigger asChild>
                <button className="import-pick" disabled={busy} data-testid="import-show">
                  {show === 'new' ? 'New photos' : 'All photos'} <span className="mono faint">{show === 'new' ? fresh.length : photos.length}</span>
                  <ChevronDown size={12} />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setShow('new')} data-testid="import-show-new">
                  New photos <span className="faint">not in the Library yet</span> <span className="grow" /> <span className="mono faint">{fresh.length}</span>
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setShow('all')} data-testid="import-show-all">
                  All photos <span className="grow" /> <span className="mono faint">{photos.length}</span>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
          {photos.some((p) => p.jpeg) && (
            <label className="imp-chip" title={rawOnly ? "The camera's JPEGs stay on the card" : "Each RAW keeps the camera's JPEG beside it"}>
              <Checkbox checked={rawOnly} onCheckedChange={(c) => setRawOnly(c === true)} disabled={busy} data-testid="import-raw-only" />
              RAW only
            </label>
          )}
          <button className="imp-close" aria-label="Close" onClick={() => !busy && go({ name: 'cull' })} disabled={busy}>
            <X size={14} />
          </button>
        </header>

        <div className="imp-body" ref={gridRef}>
          {scanning && (
            <div className="imp-pad">
              <ProgressBar label="Finding photos" done={scanTask?.done} total={scanTask?.total} eta={scanTask?.eta_s} testId="import-scan-progress" />
            </div>
          )}
          {!folder && !scanning && (
            <div className="imp-empty">
              <span className="muted">{cards === null ? 'Looking for a memory card…' : 'No memory card found.'}</span>
              <Button onClick={pickOther} data-testid="import-folder-empty">
                <Folder size={13} /> Choose a folder <MenuKeys keys={['⌘', 'O']} />
              </Button>
            </div>
          )}
          {note && (
            <div className="imp-pad muted" role="status">
              {note}
            </div>
          )}
          {photos.length > 0 && visible.length === 0 && (
            <div className="imp-pad muted" data-testid="import-none-new">
              Everything here is already in the Library.{' '}
              <button className="import-link" style={{ color: 'var(--pick)' }} onClick={() => setShow('all')}>
                Show all {photos.length}
              </button>
            </div>
          )}
          {groups.map((g, gi) => {
            const ticked = g.photos.filter((p) => chosen.has(p.path)).length;
            return (
              <section key={g.day} className="imp-group">
                <div className="imp-group-head">
                  <Checkbox
                    checked={ticked === g.photos.length ? true : ticked ? 'indeterminate' : false}
                    onCheckedChange={(c) => setMany(g.photos, c === true)}
                    disabled={busy}
                    aria-label={`Tick every photo from ${g.label}`}
                  />
                  <span className="imp-day">{g.label}</span>
                  <span className="mono faint">
                    {ticked} of {g.photos.length}
                  </span>
                  {gi === 0 && (
                    <>
                      <span className="grow" />
                      <span className="imp-keys" data-testid="import-counts">
                        <span className="muted">
                          {files.length} of {visible.length} ticked
                          {scan!.imported > 0 && show === 'new' ? ` · ${scan!.imported} already in, hidden` : ''}
                        </span>
                        <span>
                          <Kbd quiet>⌘</Kbd>
                          <Kbd quiet>A</Kbd>{' '}
                          <button className="import-link" onClick={() => setAll(true)} disabled={busy} data-testid="import-check-all">
                            all
                          </button>
                          {' · '}
                          <button className="import-link" onClick={() => setAll(false)} disabled={busy} data-testid="import-uncheck-all">
                            none
                          </button>
                        </span>
                        <span>
                          <Kbd quiet>space</Kbd> toggle
                        </span>
                        <span>pinch to resize</span>
                      </span>
                    </>
                  )}
                </div>
                <div className="import-grid" data-testid="import-grid" style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${Math.round(thumb)}px, 1fr))` }}>
                  {g.photos.map((p) => (
                    <ScanTile key={p.path} photo={p} on={chosen.has(p.path)} onToggle={() => toggle(p.path)} onFocus={() => setFocused(p.path)} disabled={busy} />
                  ))}
                </div>
              </section>
            );
          })}
        </div>

        <footer className="imp-foot">
          <div className="imp-row">
            <label htmlFor="shoot" className="imp-label">
              Shoot
            </label>
            <div className="imp-name">
              <Sparkles size={12} className="ai-icon" />
              <input
                id="shoot"
                className="mono"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setTyped(true);
                }}
                placeholder={naming === 'busy' ? 'Naming from the photos…' : 'e.g. Boat Tour'}
                disabled={busy}
                spellCheck={false}
                data-testid="import-name"
              />
            </div>
            {naming === 'busy' && scan ? (
              <div style={{ width: 240 }}>
                <ProgressBar label="Naming from the photos" eta={nameTime.eta} done={nameTime.done} total={nameTime.total} testId="import-name-progress" />
              </div>
            ) : (
              <span className="imp-hint" data-testid="import-name-hint">
                {namedBy ?? (savedAs && savedAs !== name.trim() ? <>Saved as <span className="mono">{savedAs}</span></> : null)}
                {typeof naming === 'object' && naming.by === 'date' && !typed && (
                  <span title={naming.note ?? undefined} data-testid="import-name-fallback">
                    Date only{naming.note && naming.note !== AI_NAME_OFF ? ` (${naming.note})` : ''}
                  </span>
                )}
                {typeof naming === 'object' && !typed && (
                  <button className="import-link imp-again" onClick={() => setAskAgain((n) => n + 1)} disabled={busy} data-testid="import-name-retry">
                    <RotateCw size={11} /> {naming.note === AI_NAME_OFF ? 'Name with AI' : 'Ask again'}
                  </button>
                )}
              </span>
            )}
            <span className="grow" />
            {into && copy && <span className="mono faint imp-into" title={into}>{into}</span>}
          </div>
          <div className="imp-row">
            <label className="imp-check">
              <Checkbox checked={autoCull} onCheckedChange={(c) => setAutoCull(c === true)} disabled={busy} data-testid="import-auto-cull" />
              AI cull after import
            </label>
            <label className="imp-check">
              <Checkbox checked={suggest} onCheckedChange={(c) => setSuggest(c === true ? '1' : '0')} disabled={busy} data-testid="import-suggest" />
              Suggest edits
            </label>
            <label className="imp-check" title="Lightroom's Auto on each new photo, before the cull">
              <Checkbox checked={autoTone} onCheckedChange={(c) => setAutoTone(c === true)} disabled={busy} data-testid="import-auto-tone" />
              Auto tone
            </label>
            <span className="grow" />
            {busy ? (
              <div style={{ width: 320 }}>
                <ProgressBar label={copy ? 'Copying and building previews' : 'Building previews'} done={importTask?.done} total={importTask?.total} eta={importTask?.eta_s} testId="import-progress" />
              </div>
            ) : (
              estimate && (
                <span className={`mono imp-space ${tooBig ? 'too-big' : ''}`} data-testid="import-space">
                  {tooBig ? 'Not enough room: ' : ''}
                  {estimate.photos} photos · {bytes(estimate.needed_bytes)} · {eta(importSeconds(estimate.original_bytes, copy))}
                  {estimate.free_bytes != null && (
                    <>
                      <span className="imp-disk" title={`${bytes(estimate.needed_bytes)} of ${bytes(estimate.free_bytes)} free`}>
                        <span style={{ width: `${Math.min(100, Math.max(2, (100 * estimate.needed_bytes) / estimate.free_bytes))}%` }} />
                      </span>
                      {bytes(estimate.free_bytes)} free
                    </>
                  )}
                </span>
              )
            )}
            <Button variant="ghost" onClick={() => go({ name: 'cull' })} disabled={busy} data-testid="import-cancel">
              Cancel <Kbd quiet>esc</Kbd>
            </Button>
            <Button variant="default" onClick={run} disabled={!canImport} data-testid="import-start">
              {busy ? 'Importing…' : `Import ${files.length}`} {!busy && <Kbd>↵</Kbd>}
            </Button>
          </div>
        </footer>
      </DialogContent>
    </Dialog>
  );
}

/** Photos grouped by the day they were taken, newest day first (undated last). */
function byDay(photos: ScannedPhoto[]): { day: string; label: string; photos: ScannedPhoto[] }[] {
  const days = new Map<string, ScannedPhoto[]>();
  for (const p of photos) {
    const d = p.captured_at?.slice(0, 10) ?? '';
    days.set(d, [...(days.get(d) ?? []), p]);
  }
  return [...days]
    .sort(([a], [b]) => (a && b ? b.localeCompare(a) : a ? -1 : 1))
    .map(([day, ps]) => ({ day, photos: ps, label: day ? new Date(`${day}T12:00:00`).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' }) : 'No date' }));
}

/** "qwen3.8:27b" as people say it: "Qwen 3.8". */
function modelName(m: string): string {
  const q = /^([a-z]+)(\d[\d.]*)/i.exec(m);
  return q ? `${q[1].charAt(0).toUpperCase()}${q[1].slice(1)} ${q[2]}` : m;
}

// Folders imported from lately, newest first, for the From menu (this viewer only).
const RECENT_KEY = 'argus:import-recent';
function recentFolders(): { path: string; count: number }[] {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]');
  } catch {
    return [];
  }
}
function rememberFolder(path: string, count: number) {
  try {
    const next = [{ path, count }, ...recentFolders().filter((r) => r.path !== path)].slice(0, 5);
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    // No storage: the menu just has no recent folders.
  }
}

/** One file on the card: its embedded preview once it scrolls into view, and a tick. */
function ScanTile({ photo, on, onToggle, onFocus, disabled }: { photo: ScannedPhoto; on: boolean; onToggle: () => void; onFocus: () => void; disabled: boolean }) {
  const ref = useRef<HTMLButtonElement>(null);
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let live = true;
    const io = new IntersectionObserver((entries) => {
      if (!entries.some((e) => e.isIntersecting)) return;
      io.disconnect();
      queueThumb(photo.path).then((s) => live && setSrc(s), () => {});
    });
    io.observe(el);
    return () => {
      live = false;
      io.disconnect();
    };
  }, [photo.path]);
  return (
    <button
      ref={ref}
      className={`import-tile ${on ? 'on' : ''} ${photo.imported ? 'imported' : ''}`}
      onClick={() => {
        onFocus();
        onToggle();
      }}
      onMouseEnter={onFocus}
      disabled={disabled}
      aria-pressed={on}
      title={photo.file_name}
      data-testid="import-tile"
      data-imported={photo.imported || undefined}
    >
      <span className="import-ph">
        {src ? <img src={src} alt="" draggable={false} /> : <span className="import-tile-blank" />}
        <span className="import-tick">{on && <Check size={11} strokeWidth={3} />}</span>
        {photo.imported ? <span className="import-tag">Imported</span> : photo.jpeg && <span className="import-tag">RAW+JPEG</span>}
      </span>
      <span className="import-name mono">{photo.file_name}</span>
    </button>
  );
}

// Thumbnails come from the card itself, so only a few are read at once.
const THUMB_SLOTS = 4;
let thumbsInFlight = 0;
const thumbQueue: (() => void)[] = [];
function queueThumb(path: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const go = () => {
      thumbsInFlight++;
      thumbnail(path)
        .then(resolve, reject)
        .finally(() => {
          thumbsInFlight--;
          thumbQueue.shift()?.();
        });
    };
    if (thumbsInFlight < THUMB_SLOTS) go();
    else thumbQueue.push(go);
  });
}

function DemoImportDialog({ open, autoCull, setAutoCull }: ImportProps) {
  const start = useImportRunner();
  const [source, setSource] = useState<'card' | 'folder'>('card');
  const [name, setName] = useState(SHOOT.name);
  const [dest, setDest] = useState<string | null>(null);
  const folder = dest ?? shootFolder('2026-10-12', name);

  return (
    <Dialog open={open} onOpenChange={(o) => !o && go({ name: 'cull' })}>
      <DialogContent className="w-[min(600px,calc(100%-32px))]" data-testid="import-dialog">
        <div>
          <DialogTitle>Import a Shoot</DialogTitle>
          <DialogDescription className="mt-1.5">Argus reads your originals in place and never changes them.</DialogDescription>
        </div>
        <div role="group" aria-label="Source" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <button className={`source ${source === 'card' ? 'on' : ''}`} onClick={() => setSource('card')} aria-pressed={source === 'card'}>
            <div style={{ fontWeight: 500 }}>Memory card</div>
            <div className="mono" style={{ fontSize: 12, color: source === 'card' ? 'var(--text)' : 'var(--text-2)', marginTop: 4 }}>
              SONY A7IV · 112 photos
            </div>
          </button>
          <button className={`source ${source === 'folder' ? 'on' : ''}`} onClick={() => setSource('folder')} aria-pressed={source === 'folder'}>
            <div style={{ fontWeight: 500 }}>Folder</div>
            <div className="mono" style={{ fontSize: 12, color: 'var(--text-2)', marginTop: 4 }}>{source === 'folder' ? '~/Desktop/tahoe-raw (112)' : 'Choose a folder on disk'}</div>
          </button>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '140px 1fr', rowGap: 12, alignItems: 'center' }}>
          <label htmlFor="shoot" className="muted">
            Shoot name
          </label>
          <input id="shoot" className="field" value={name} onChange={(e) => setName(e.target.value)} />
          <label htmlFor="copy" className="muted">
            Copy originals to
          </label>
          <input id="copy" className="field mono" style={{ fontSize: 13 }} value={folder} onChange={(e) => setDest(noSpaces(e.target.value))} spellCheck={false} />
        </div>
        <label className="row" style={{ gap: 8, color: 'var(--text-2)', fontSize: 13 }}>
          <Checkbox checked={autoCull} onCheckedChange={(c) => setAutoCull(c === true)} />
          Cull with Claude as soon as previews are ready
        </label>
        <div className="row" style={{ justifyContent: 'flex-end', gap: 10 }}>
          <Button size="lg" onClick={() => go({ name: 'cull' })}>
            Cancel
          </Button>
          <Button
            size="lg"
            variant="default"
            onClick={() => {
              start();
              go({ name: 'cull' });
            }}
          >
            Import 112 photos
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
