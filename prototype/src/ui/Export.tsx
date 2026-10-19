// Export, laid out like Lightroom's Export dialog (Greg: follow modern Lightroom flows):
// presets down the left (built in, plus your own), then Location, File naming, Image and
// Metadata, and a footer saying how much, how long and how much room is left. Export then
// runs in the background with a progress toast and an ETA, so the dialog never blocks.
// Live, it calls the engine's `export_photos` one Photo at a time, so progress, the ETA
// and Stop are exact. With dummy data it pretends.

import { useEffect, useState, useSyncExternalStore } from 'react';
import { toast } from 'sonner';
import { Download, FolderOpen, Layers, Plus, X } from 'lucide-react';
import { PHOTOS, SHOOT } from '../data/dummy';
import { LIVE } from '../data/live';
import { call, pickFolder } from '../data/backend';
import { useLibrary, verdictOf } from '../store/library';
import type { Photo } from '../domain/types';
import { Kbd } from './kit';
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Slider } from '@/components/ui/slider';
import { Select } from '@/components/ui/select';

type Format = 'jpeg' | 'tiff' | 'original';
type Naming = 'shoot' | 'sequence' | 'original';
type Size = 'full' | '4096' | '2048' | '1080';

interface Settings {
  folder: string;
  shootFolder: boolean;
  naming: Naming;
  format: Format;
  quality: number;
  size: Size;
  reveal: boolean;
}

interface Preset {
  id: string;
  label: string;
  set: Partial<Settings>;
}

const BUILT_IN: Preset[] = [
  { id: 'full', label: 'Full size JPEG', set: { format: 'jpeg', size: 'full', quality: 92 } },
  { id: 'web', label: 'Web · 2048 px', set: { format: 'jpeg', size: '2048', quality: 85 } },
  { id: 'social', label: 'Social · 1080 px', set: { format: 'jpeg', size: '1080', quality: 85 } },
  { id: 'print', label: 'Print · TIFF', set: { format: 'tiff', size: 'full' } },
  { id: 'lightroom', label: 'To Lightroom · RAW + XMP', set: { format: 'original' } },
];
// What a preset carries: file settings, never where the files go.
const PRESET_KEYS: (keyof Settings)[] = ['format', 'size', 'quality', 'naming'];

const DEFAULTS: Settings = { folder: '~/Pictures/Argus/Exports', shootFolder: true, naming: 'shoot', format: 'jpeg', quality: 92, size: 'full', reveal: false };
const KEY = 'argus-export';
const PRESETS_KEY = 'argus-export-presets';

function read<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    // No storage: the defaults come back next time.
  }
}

const load = (): Settings => ({ ...DEFAULTS, ...read<Partial<Settings>>(KEY, {}) });

/** Where exports go and what happens after, as Settings › Export shows them. */
export type ExportPlace = Pick<Settings, 'folder' | 'shootFolder' | 'reveal'>;
export const exportPlace = (): ExportPlace => {
  const s = load();
  return { folder: s.folder, shootFolder: s.shootFolder, reveal: s.reveal };
};
export const setExportPlace = (patch: Partial<ExportPlace>) => write(KEY, { ...load(), ...patch });
/** Your own presets, saved from the Export dialog. */
export const savedExportPresets = (): { id: string; label: string; summary: string }[] =>
  read<Preset[]>(PRESETS_KEY, []).map((p) => ({ id: p.id, label: p.label, summary: summary(p.set) }));
export const deleteExportPreset = (id: string) => write(PRESETS_KEY, read<Preset[]>(PRESETS_KEY, []).filter((p) => p.id !== id));

function summary(s: Partial<Settings>): string {
  const format = s.format === 'tiff' ? 'TIFF' : s.format === 'original' ? 'Original' : `JPEG${s.quality ? ` ${s.quality}` : ''}`;
  const size = !s.size || s.size === 'full' ? 'full size' : `${s.size} px`;
  return s.format === 'original' ? format : `${format} · ${size}`;
}

function matches(p: Preset, s: Settings) {
  return Object.entries(p.set).every(([k, v]) => s[k as keyof Settings] === v);
}

const stem = (file: string) => file.replace(/\.[^.]+$/, '').replace(/\s+/g, '-');
/** The Shoot's folder name, as `engine::naming` makes it (no spaces, ever). */
const shootFolder = () => SHOOT.name.replace(/[^\p{L}\p{N}_-]+/gu, '-').replace(/^-+|-+$/g, '');

/** What the first file will be called, as the engine names it (docs/naming.md). */
function example(s: Settings, file: string, total: number): string {
  const ext = s.format === 'jpeg' ? 'jpg' : s.format === 'tiff' ? 'tif' : (file.split('.').pop() ?? '');
  const shoot = shootFolder();
  const name = s.naming === 'original' ? stem(file) : s.naming === 'sequence' ? `${shoot}_${'1'.padStart(Math.max(3, String(total).length), '0')}` : `${shoot}_${stem(file)}`;
  return `${name}.${ext}`;
}

/** Rough size and time per Photo, for the footer, before anything has run. A7R-size frames. */
function estimate(s: Settings): { bytes: number | null; secs: number } {
  const px = s.size === 'full' ? 42e6 : (Number(s.size) ** 2 * 2) / 3;
  if (s.format === 'original') return { bytes: null, secs: 0.3 };
  if (s.format === 'tiff') return { bytes: px * 3, secs: 1.6 };
  return { bytes: px * (0.1 + ((s.quality - 50) / 50) * 0.35), secs: s.size === 'full' ? 1.3 : 0.6 };
}

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${Math.round(n / 1e6)} MB`;
  return `${Math.max(1, Math.round(n / 1e3))} KB`;
}

function duration(secs: number): string {
  const s = Math.max(1, Math.ceil(secs));
  return s < 60 ? `${s} s` : `${Math.ceil(s / 60)} min`;
}

// The running export lives outside the dialog, so closing it doesn't stop anything.
interface Run {
  done: number;
  total: number;
  started: number;
  current?: string;
  failed: { file: string; error: string }[];
  finished: boolean;
  cancelled: boolean;
  folder?: string;
}

let run: Run | null = null;
let stop = false;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());
const subscribe = (l: () => void) => (listeners.add(l), () => listeners.delete(l));
const useRun = () => useSyncExternalStore(subscribe, () => run);

function ExportToast() {
  const r = useRun();
  const [, tick] = useState(0);
  // Re-render each second so the ETA counts down between Photos.
  useEffect(() => {
    if (!r || r.finished) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [r]);
  if (!r) return null;
  const elapsed = Date.now() - r.started;
  const left = r.done > 0 ? ((elapsed / r.done) * (r.total - r.done)) / 1000 : null;
  const ok = r.done - r.failed.length;
  return (
    <div className="export-toast" aria-live="polite">
      <div className="bar">
        <div style={{ width: `${(r.done / Math.max(1, r.total)) * 100}%`, background: r.finished && !r.failed.length ? 'var(--done)' : undefined }} />
      </div>
      <div className="faint mono" style={{ fontSize: 11.5 }} data-testid="export-status">
        {!r.finished && `${r.done} of ${r.total} · ${left == null ? 'estimating…' : `about ${duration(left)} left`}${r.current ? ` · ${r.current}` : ''}`}
        {r.finished && `${r.cancelled ? 'Stopped. ' : 'Done. '}${ok} ${ok === 1 ? 'file' : 'files'} in ${r.folder ?? 'the export folder'}`}
      </div>
      {r.failed.slice(0, 3).map((f) => (
        <div key={f.file} className="mono" style={{ fontSize: 11.5, color: 'var(--reject)' }}>
          {f.file}: {f.error}
        </div>
      ))}
      {r.failed.length > 3 && <div className="faint mono">and {r.failed.length - 3} more</div>}
    </div>
  );
}

function showToast() {
  const r = run!;
  const title = r.finished ? (r.cancelled ? 'Export stopped' : r.failed.length ? `Exported with ${r.failed.length} failed` : `Exported ${r.done - r.failed.length}`) : `Exporting ${r.total}`;
  const folder = r.folder;
  toast(title, {
    id: 'export',
    description: <ExportToast />,
    duration: r.finished ? 10000 : Infinity,
    action: r.finished
      ? LIVE && folder
        ? { label: 'Show in Finder', onClick: () => void reveal(folder) }
        : undefined
      : {
          label: 'Stop',
          onClick: (e) => {
            e.preventDefault();
            stop = true;
          },
        },
  });
}

async function reveal(folder: string) {
  try {
    await call('reveal', { path: folder });
  } catch (e) {
    toast(`Couldn't show the folder: ${String(e).replace(/^Error: /, '')}`, { id: 'export-reveal' });
  }
}

async function startExport(photos: Photo[], s: Settings) {
  if (run && !run.finished) return;
  stop = false;
  const total = photos.length;
  const r: Run = { done: 0, total, started: Date.now(), failed: [], finished: false, cancelled: false, folder: `${s.folder}${s.shootFolder ? `/${shootFolder()}` : ''}` };
  const update = () => {
    run = { ...r };
    emit();
  };
  update();
  showToast();
  for (const [i, p] of photos.entries()) {
    if (stop) {
      r.cancelled = true;
      break;
    }
    r.current = p.file;
    update();
    if (LIVE) {
      try {
        const out = await call<{ exported: { path: string }[]; failed: { error: string }[] }>('export_photos', {
          photo_ids: [Number(p.id)],
          folder: s.folder,
          shoot_folder: s.shootFolder,
          format: s.format,
          naming: s.naming,
          ...(s.format === 'jpeg' ? { quality: s.quality } : {}),
          ...(s.size === 'full' || s.format === 'original' ? {} : { long_edge: Number(s.size) }),
          sequence_start: i + 1,
          sequence_total: total,
        });
        if (out.failed.length) r.failed.push({ file: p.file, error: out.failed[0].error });
        const path = out.exported[0]?.path;
        if (path) r.folder = path.slice(0, path.lastIndexOf('/'));
      } catch (e) {
        r.failed.push({ file: p.file, error: String((e as Error).message ?? e) });
      }
    } else {
      await new Promise((res) => setTimeout(res, 90));
    }
    r.done = i + 1;
    update();
  }
  r.finished = true;
  r.current = undefined;
  update();
  showToast();
  if (LIVE && s.reveal && r.folder && r.done > r.failed.length) void reveal(r.folder);
}

/** `ids`: the Photos to export (a selection); the Shoot's Picks when omitted. */
export function ExportDialog({ open, onOpenChange, ids }: { open: boolean; onOpenChange: (o: boolean) => void; ids?: string[] }) {
  const { state } = useLibrary();
  const photos = ids ? PHOTOS.filter((p) => ids.includes(p.id)) : PHOTOS.slice(0, state.imported).filter((p) => verdictOf(state, p.id) === 'pick');
  const [s, setS] = useState<Settings>(load);
  const [mine, setMine] = useState<Preset[]>(() => read<Preset[]>(PRESETS_KEY, []));
  const [naming, setNaming] = useState<string | null>(null);
  const [free, setFree] = useState<number | null>(null);
  const running = !!useRun() && !run?.finished;
  useEffect(() => {
    if (!open) return;
    setS(load());
    setNaming(null);
    if (!LIVE) return setFree(106.7e9);
    // The Library's disk: shown only when the export goes to the same internal disk.
    call<{ free_bytes?: number | null; library?: string }>('library_storage')
      .then((d) => setFree(d.free_bytes != null && !String(d.library ?? '').startsWith('/Volumes/') ? d.free_bytes : null))
      .catch(() => setFree(null));
  }, [open]);

  const set = (patch: Partial<Settings>) =>
    setS((cur) => {
      const next = { ...cur, ...patch };
      write(KEY, next);
      return next;
    });
  const presets = [...BUILT_IN, ...mine];
  const current = presets.find((p) => matches(p, s))?.id ?? null;
  const savePreset = (label: string) => {
    const set: Partial<Settings> = {};
    for (const k of PRESET_KEYS) (set as Record<string, unknown>)[k] = s[k];
    const next = [...mine.filter((p) => p.label !== label), { id: `mine-${Date.now().toString(36)}`, label, set }];
    setMine(next);
    write(PRESETS_KEY, next);
    setNaming(null);
  };
  const removePreset = (id: string) => {
    const next = mine.filter((p) => p.id !== id);
    setMine(next);
    write(PRESETS_KEY, next);
  };

  const n = photos.length;
  const est = estimate(s);
  const total = est.bytes != null ? est.bytes * n : null;
  const where = `${s.folder}${s.shootFolder ? `/${shootFolder()}` : ''}`;
  const go = () => {
    if (!n || running) return;
    onOpenChange(false);
    void startExport(photos, s);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="export-v2" data-testid="export-dialog" onKeyDown={(e) => e.key === 'Enter' && (e.metaKey || !(e.target instanceof HTMLInputElement)) && (e.preventDefault(), go())}>
        <div className="ex-head">
          <DialogTitle asChild>
            <h2>Export</h2>
          </DialogTitle>
          <span className="mono faint" style={{ fontSize: 11 }}>
            {n} {ids ? 'selected' : n === 1 ? 'Pick' : 'Picks'} · {shootFolder()}
          </span>
        </div>
        <div className="ex-body">
          <nav className="ex-presets" aria-label="Presets">
            <span className="cap">Presets</span>
            {presets.map((p) => (
              <button key={p.id} className={p.id === current ? 'on' : ''} aria-pressed={p.id === current} onClick={() => set(p.set)}>
                <Layers size={12} />
                <span className="grow" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {p.label}
                </span>
                {p.id.startsWith('mine-') && (
                  <X
                    size={11}
                    aria-label={`Delete ${p.label}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      removePreset(p.id);
                    }}
                  />
                )}
              </button>
            ))}
            <span className="grow" />
            {naming != null ? (
              <form
                className="row"
                style={{ gap: 4, padding: '0 4px' }}
                onSubmit={(e) => {
                  e.preventDefault();
                  if (naming.trim()) savePreset(naming.trim());
                }}
              >
                <input className="ex-path" autoFocus value={naming} placeholder="Preset name" onChange={(e) => setNaming(e.target.value)} onKeyDown={(e) => e.key === 'Escape' && (e.stopPropagation(), setNaming(null))} aria-label="Preset name" />
              </form>
            ) : (
              <button onClick={() => setNaming('')} disabled={!!current}>
                <Plus size={12} /> Save as preset
              </button>
            )}
          </nav>

          <fieldset className="ex-form">
            <section className="ex-sec">
              <h3>Location</h3>
              <div className="ex-row">
                <span className="k">Folder</span>
                <span className="ex-path mono" title={where}>
                  {where}
                </span>
                <Button
                  size="sm"
                  onClick={async () => {
                    const dir = await pickFolder('Export to…');
                    if (dir) set({ folder: dir });
                  }}
                >
                  <FolderOpen size={12} /> Choose…
                </Button>
              </div>
              <label className="ex-row">
                <span className="k" />
                <Checkbox checked={s.shootFolder} onCheckedChange={(c) => set({ shootFolder: c === true })} />
                Put in a folder named after the Shoot
              </label>
              {LIVE && (
                <label className="ex-row">
                  <span className="k" />
                  <Checkbox checked={s.reveal} onCheckedChange={(c) => set({ reveal: c === true })} />
                  Open in Finder when done
                </label>
              )}
            </section>

            <section className="ex-sec">
              <h3>File naming</h3>
              <div className="ex-row">
                <span className="k">Name</span>
                <Select
                  label="File naming"
                  value={s.naming}
                  onValueChange={(naming) => set({ naming })}
                  items={[
                    { value: 'shoot', label: 'Shoot_Original' },
                    { value: 'sequence', label: 'Shoot_Number' },
                    { value: 'original', label: 'Original' },
                  ]}
                  className="w-[180px]"
                />
                <span className="mono faint" style={{ fontSize: 11 }} data-testid="export-example">
                  {photos[0] ? example(s, photos[0].file, n) : ''}
                </span>
              </div>
            </section>

            <section className="ex-sec">
              <h3>Image</h3>
              <div className="ex-row">
                <span className="k">Format</span>
                <Select
                  label="Format"
                  value={s.format}
                  onValueChange={(format) => set({ format })}
                  items={[
                    { value: 'jpeg', label: 'JPEG' },
                    { value: 'tiff', label: 'TIFF' },
                    { value: 'original', label: 'Original + XMP' },
                  ]}
                  className="w-[140px]"
                />
                {s.format !== 'original' && <span className="v-chip">sRGB</span>}
              </div>
              {s.format === 'jpeg' && (
                <div className="ex-row">
                  <span className="k">Quality</span>
                  <Slider className="export-quality" style={{ width: 220 }} min={50} max={100} step={1} value={[s.quality]} onValueChange={([q]) => set({ quality: q })} aria-label="Quality" />
                  <span className="mono" style={{ width: 28, textAlign: 'right' }}>
                    {s.quality}
                  </span>
                </div>
              )}
              {s.format !== 'original' ? (
                <div className="ex-row">
                  <span className="k">Size</span>
                  <Select
                    label="Size"
                    value={s.size}
                    onValueChange={(size) => set({ size })}
                    items={[
                      { value: 'full', label: 'Full size' },
                      { value: '4096', label: 'Long edge 4096 px' },
                      { value: '2048', label: 'Long edge 2048 px' },
                      { value: '1080', label: 'Long edge 1080 px' },
                    ]}
                    className="w-[180px]"
                  />
                </div>
              ) : (
                <div className="ex-row">
                  <span className="k" />
                  <span className="ex-note">The untouched original plus an XMP sidecar with its edits and Rejects, which Lightroom reads when it imports the folder.</span>
                </div>
              )}
            </section>

            <section className="ex-sec">
              <h3>Metadata</h3>
              <div className="ex-row">
                <span className="k">Include</span>
                <span>Camera, lens and capture time</span>
              </div>
              <label className="ex-row">
                <span className="k" />
                <Checkbox checked disabled />
                Remove location <span className="ex-note">(always)</span>
              </label>
            </section>
          </fieldset>
        </div>
        <div className="ex-foot">
          <span className="facts mono">
            {n} {n === 1 ? 'photo' : 'photos'}
            {total != null && ` · about ${bytes(total)}`} · about {duration(est.secs * n)}
            {free != null && ` · ${bytes(free)} free`}
          </span>
          <span className="facts">Runs in the background, with progress in a notice</span>
          <span className="grow" />
          <Button size="sm" onClick={() => onOpenChange(false)}>
            Cancel <Kbd>esc</Kbd>
          </Button>
          <Button size="sm" variant="default" onClick={go} disabled={!n || running} data-testid="export-start">
            <Download size={12} /> Export {n} <Kbd>↵</Kbd>
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
