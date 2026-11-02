// Export, laid out like Lightroom's Export dialog (Greg: follow modern Lightroom flows):
// presets down the left (built in, plus your own), then Location, File naming, Image and
// Metadata, and a footer saying how much, how long and how much room is left. Export then
// runs in the background with a progress toast and an ETA, so the dialog never blocks.
// Live, it calls the engine's `export_photos` one Photo at a time, so progress, the ETA
// and Stop are exact. With dummy data it pretends.

import { useEffect, useState, useSyncExternalStore, type ReactNode } from 'react';
import { toast } from 'sonner';
import { useHotkeys } from 'react-hotkeys-hook';
import { Download, FolderOpen, Layers, Plus, X } from 'lucide-react';
import { PHOTOS, SHOOT } from '../data/dummy';
import { LIVE } from '../data/live';
import { call, pickFolder } from '../data/backend';
import { useLibrary } from '../store/library';
import { picksOf } from '../data/modes';
import type { Photo } from '../domain/types';
import { Kbd } from './kit';
import { ProgressBar } from './Progress';
import { EMPTY_ART } from './emptyArt';
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Slider } from '@/components/ui/slider';
import { Select } from '@/components/ui/select';

type Format = 'jpeg' | 'heic' | 'tiff' | 'original';
type Naming = 'shoot' | 'sequence' | 'original';
/** Full, a long edge in px, `short-N` a short edge, `mp-N` megapixels. */
type Size = 'full' | '7680' | '4096' | '2048' | '1080' | 'short-1080' | 'mp-12' | `${number}`;
/** The long edges the Size menu lists; any other number is a Custom long edge. */
const LISTED_EDGES = ['7680', '4096', '2048', '1080'];
const isCustom = (size: Size) => /^\d+$/.test(size) && !LISTED_EDGES.includes(size);
/** Lightroom's Output sharpening for screen. */
type Sharpen = 'off' | 'low' | 'standard' | 'high';

interface Settings {
  folder: string;
  shootFolder: boolean;
  naming: Naming;
  format: Format;
  quality: number;
  size: Size;
  sharpen: Sharpen;
  reveal: boolean;
  /** Lightroom's "Remove Location Info": GPS stays out of rendered files unless unticked. */
  removeLocation: boolean;
  /** The last Custom long edge, so picking Custom again brings it back. */
  customEdge: number;
  /** sRGB, or Display P3 for Apple screens (JPEG and HEIC). */
  colorSpace: ColorSpace;
  /** Lightroom's Watermark: a line of text in a corner, on when `on`. */
  watermark: { on: boolean; text: string; corner: Corner; size: number; opacity: number };
  /** Import the files into Apple Photos when done, into an album named after the Shoot. */
  addToPhotos: boolean;
}
type ColorSpace = 'srgb' | 'display_p3';
type Corner = 'bottom_right' | 'bottom_left' | 'top_right' | 'top_left' | 'center';

interface Preset {
  id: string;
  label: string;
  set: Partial<Settings>;
}

const BUILT_IN: Preset[] = [
  // Measured on Greg's A7R V frames (2026-11-02, docs/export.md): past quality 85 the file
  // grows fast for little visible gain, and a full 61 MP frame at 85 is still only ~6 MB.
  { id: 'recommended', label: 'Recommended · JPEG 85 full', set: { format: 'jpeg', size: 'full', quality: 85, sharpen: 'off', colorSpace: 'srgb' } },
  { id: 'full', label: 'Full size JPEG', set: { format: 'jpeg', size: 'full', quality: 92 } },
  { id: 'web', label: 'Web · 2048 px', set: { format: 'jpeg', size: '2048', quality: 85 } },
  { id: 'social', label: 'Instagram · 1080 short edge', set: { format: 'jpeg', size: 'short-1080', quality: 85, sharpen: 'standard' } },
  { id: 'phone', label: 'Phone · HEIC 12 MP', set: { format: 'heic', size: 'mp-12', quality: 85, sharpen: 'low' } },
  { id: 'print', label: 'Print · TIFF', set: { format: 'tiff', size: 'full' } },
  { id: 'lightroom', label: 'To Lightroom · RAW + XMP', set: { format: 'original' } },
];
// What a preset carries: file settings, never where the files go.
const PRESET_KEYS: (keyof Settings)[] = ['format', 'size', 'quality', 'naming', 'sharpen', 'colorSpace'];

const DEFAULTS: Settings = { folder: '~/Pictures/Argus/Exports', shootFolder: true, naming: 'shoot', format: 'jpeg', quality: 85, size: 'full', sharpen: 'off', reveal: false, removeLocation: true, customEdge: 6000, colorSpace: 'srgb', watermark: { on: false, text: '©', corner: 'bottom_right', size: 3, opacity: 0.6 }, addToPhotos: false };
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

const load = (): Settings => {
  const saved = read<Partial<Settings>>(KEY, {});
  return { ...DEFAULTS, ...saved, watermark: { ...DEFAULTS.watermark, ...saved.watermark } };
};

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
  const format = s.format === 'tiff' ? 'TIFF' : s.format === 'original' ? 'Original' : `${s.format === 'heic' ? 'HEIC' : 'JPEG'}${s.quality ? ` ${s.quality}` : ''}`;
  const size = !s.size || s.size === 'full' ? 'full size' : sizeLabel(s.size);
  const sharp = s.sharpen && s.sharpen !== 'off' ? ` · sharpen ${s.sharpen}` : '';
  const p3 = s.colorSpace === 'display_p3' ? ' · P3' : '';
  return s.format === 'original' ? format : `${format} · ${size}${sharp}${p3}`;
}

function sizeLabel(size: Size): string {
  if (size.startsWith('short-')) return `${size.slice(6)} px short edge`;
  if (size.startsWith('mp-')) return `${size.slice(3)} MP`;
  return `${size} px`;
}

/** What `export_photos` takes for a size. */
function sizeArgs(size: Size): Record<string, number> {
  if (size === 'full') return {};
  if (size.startsWith('short-')) return { short_edge: Number(size.slice(6)) };
  if (size.startsWith('mp-')) return { megapixels: Number(size.slice(3)) };
  return { long_edge: Number(size) };
}

function matches(p: Preset, s: Settings) {
  return Object.entries(p.set).every(([k, v]) => s[k as keyof Settings] === v);
}

const stem = (file: string) => file.replace(/\.[^.]+$/, '').replace(/\s+/g, '-');
/** The Shoot's folder name, as `engine::naming` makes it (no spaces, ever). */
const shootFolder = () => SHOOT.name.replace(/[^\p{L}\p{N}_-]+/gu, '-').replace(/^-+|-+$/g, '');

/** What the first file will be called, as the engine names it (docs/naming.md). */
function example(s: Settings, file: string, total: number): string {
  const ext = s.format === 'jpeg' ? 'jpg' : s.format === 'heic' ? 'heic' : s.format === 'tiff' ? 'tif' : (file.split('.').pop() ?? '');
  const shoot = shootFolder();
  const name = s.naming === 'original' ? stem(file) : s.naming === 'sequence' ? `${shoot}_${'1'.padStart(Math.max(3, String(total).length), '0')}` : `${shoot}_${stem(file)}`;
  return `${name}.${ext}`;
}

/** A Photo's pixels when the Library knows them; dummy data and unknowns are A7R V frames. */
const pixelsOf = (p?: Photo): [number, number] => p?.pixels ?? [9504, 6336];
/** The pixels a Photo leaves at, for a size choice. Never upscales, as the engine. */
export function outPixels(size: Size, [w, h]: [number, number]): [number, number] {
  const long = Math.max(w, h);
  const ratio = long / Math.max(1, Math.min(w, h));
  const want = size === 'full' ? long : size.startsWith('mp-') ? Math.sqrt(Number(size.slice(3)) * 1e6 * ratio) : size.startsWith('short-') ? Number(size.slice(6)) * ratio : Number(size);
  const k = Math.min(1, want / long);
  return [Math.round(w * k), Math.round(h * k)];
}
/** Bytes per pixel the last real export of this format and quality came out at, so
 *  estimates match this person's Photos after one export. */
const LEARNED_KEY = 'argus-export-bpp';
const learnedKey = (s: Pick<Settings, 'format' | 'quality'>) => (s.format === 'jpeg' || s.format === 'heic' ? `${s.format}-${s.quality}` : s.format);
/** Rough size and time for one Photo, before anything has run. */
export function estimate(s: Pick<Settings, 'format' | 'quality' | 'size'>, pixels: [number, number]): { bytes: number | null; secs: number; out: [number, number] } {
  const out = outPixels(s.size, pixels);
  const px = out[0] * out[1];
  const secs = 0.35 + (px / 1e6) * 0.02 + (s.format === 'heic' ? 0.4 : 0);
  if (s.format === 'original') return { bytes: null, secs: 0.3, out: pixels };
  const learned = read<Record<string, number>>(LEARNED_KEY, {})[learnedKey(s)];
  if (learned) return { bytes: px * learned, secs, out };
  if (s.format === 'tiff') return { bytes: px * 3, secs: secs + 0.3, out };
  const jpeg = px * jpegBpp(s.quality);
  return { bytes: s.format === 'heic' ? jpeg * 0.6 : jpeg, secs, out };
}
/** Bytes per pixel of Argus JPEGs by quality, measured on A7R V frames (docs/export.md). */
const JPEG_BPP: [number, number][] = [[50, 0.03], [75, 0.057], [80, 0.069], [85, 0.086], [90, 0.12], [95, 0.203], [100, 0.42]];
function jpegBpp(q: number): number {
  const i = JPEG_BPP.findIndex(([at]) => at >= q);
  if (i <= 0) return JPEG_BPP[Math.max(0, i)][1];
  const [[q0, b0], [q1, b1]] = [JPEG_BPP[i - 1], JPEG_BPP[i]];
  return b0 + ((b1 - b0) * (q - q0)) / (q1 - q0);
}
function learn(s: Settings, bytesOut: number, px: number) {
  if (s.format === 'original' || !bytesOut || !px) return;
  const all = read<Record<string, number>>(LEARNED_KEY, {});
  const k = learnedKey(s);
  const bpp = bytesOut / px;
  // A running average, so one odd frame (a black sky) doesn't swing it.
  write(LEARNED_KEY, { ...all, [k]: all[k] ? all[k] * 0.7 + bpp * 0.3 : bpp });
}
function bytes(n: number): string {
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

/** The running Export, for the Status bar: done, total and seconds left. */
export function useExportProgress(): { done: number; total: number; eta_s: number | null } | null {
  const r = useRun();
  if (!r || r.finished) return null;
  return { done: r.done, total: r.total, eta_s: r.done > 0 ? ((Date.now() - r.started) / r.done) * (r.total - r.done) / 1000 : null };
}

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
      {!r.finished && <ProgressBar label={r.current ?? 'Starting'} done={r.done} total={r.total} eta={left} testId="export-status" />}
      {r.finished && (
        <div className="export-done faint" data-testid="export-status">
          {!r.cancelled && !r.failed.length && <img className="empty-art" src={EMPTY_ART.done} alt="" />}
          <span>{`${r.cancelled ? 'Stopped. ' : 'Done. '}${ok} ${ok === 1 ? 'file' : 'files'} in ${r.folder ?? 'the export folder'}`}</span>
        </div>
      )}
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
  const paths: string[] = [];
  for (const [i, p] of photos.entries()) {
    if (stop) {
      r.cancelled = true;
      break;
    }
    r.current = p.file;
    update();
    if (LIVE) {
      try {
        const out = await call<{ exported: { path: string; bytes?: number; width?: number; height?: number }[]; failed: { error: string }[] }>('export_photos', {
          photo_ids: [Number(p.id)],
          folder: s.folder,
          shoot_folder: s.shootFolder,
          format: s.format,
          naming: s.naming,
          ...(s.format === 'jpeg' || s.format === 'heic' ? { quality: s.quality } : {}),
          ...(s.format === 'original' ? {} : sizeArgs(s.size)),
          ...(s.format !== 'original' && s.sharpen !== 'off' ? { sharpen: s.sharpen } : {}),
          ...(s.format !== 'original' && !s.removeLocation ? { keep_location: true } : {}),
          ...(s.format !== 'original' && s.format !== 'tiff' && s.colorSpace !== 'srgb' ? { color_space: s.colorSpace } : {}),
          ...(s.format !== 'original' && s.watermark.on && s.watermark.text.trim()
            ? { watermark: { text: s.watermark.text.trim(), corner: s.watermark.corner, size: s.watermark.size, opacity: s.watermark.opacity } }
            : {}),
          sequence_start: i + 1,
          sequence_total: total,
        });
        if (out.failed.length) r.failed.push({ file: p.file, error: out.failed[0].error });
        const x = out.exported[0];
        if (x?.bytes && x.width && x.height) learn(s, x.bytes, x.width * x.height);
        const path = x?.path;
        if (path) paths.push(path);
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
  if (s.addToPhotos && s.format !== 'original' && paths.length) void toPhotos(paths, shootFolder());
}

/** Hand the exported files to Apple Photos, with a notice either way. */
async function toPhotos(paths: string[], album: string) {
  const id = 'export-photos';
  toast(`Adding ${paths.length} to Apple Photos…`, { id, duration: Infinity });
  try {
    await call('add_to_apple_photos', { paths, album });
    toast(`Added ${paths.length} to Apple Photos, album ${album}`, { id, duration: 6000 });
  } catch (e) {
    toast(`Couldn't add to Apple Photos: ${String((e as Error).message ?? e).replace(/^Error: /, '')}`, { id, duration: 10000 });
  }
}

/** Lightroom's Export with Previous: the last settings, straight to the background run. */
export function exportWithPrevious(photos: Photo[]) {
  if (!photos.length) return void toast('No Picks to export', { id: 'export' });
  void startExport(photos, load());
}

/** `ids`: the Photos to export (a selection); the Shoot's Picks when omitted. */
export function ExportDialog(props: { open: boolean; onOpenChange: (o: boolean) => void; ids?: string[] }) {
  return <Export {...props} />;
}

/** What Export mode (⌘4) puts around the same presets and settings. */
export interface ExportMode {
  /** Between the presets and the settings: the Photos that will go out. */
  middle: ReactNode;
  /** The screen's header, given the count, a one-line estimate and the export action. */
  head: (a: { n: number; facts: string; start: () => void; running: boolean }) => ReactNode;
  /** The Photos to export (the mode's Picks, less any left out). */
  photos: Photo[];
}

/** Export mode's body: the dialog's presets and settings laid out on a screen. */
export function ExportModeBody({ mode }: { mode: ExportMode }) {
  return <Export open onOpenChange={() => {}} mode={mode} />;
}

function Export({ open, onOpenChange, ids, mode }: { open: boolean; onOpenChange: (o: boolean) => void; ids?: string[]; mode?: ExportMode }) {
  const { state } = useLibrary();
  const photos = mode ? mode.photos : ids ? PHOTOS.filter((p) => ids.includes(p.id)) : picksOf(state);
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
  // Summed over the actual Photos (portraits, crops, small JPEGs), not n times one guess.
  const ests = photos.map((p) => estimate(s, pixelsOf(p)));
  const est = { secs: ests.reduce((a, e) => a + e.secs, 0) };
  const total = s.format === 'original' || !n ? null : ests.reduce((a, e) => a + (e.bytes ?? 0), 0);
  const each = total != null && n ? total / n : null;
  const first = ests[0]?.out;
  const tight = total != null && free != null && total > free * 0.9;
  const where = `${s.folder}${s.shootFolder ? `/${shootFolder()}` : ''}`;
  const go = () => {
    if (!n || running) return;
    onOpenChange(false);
    void startExport(photos, s);
  };

  const presetsNav = (
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
  );
  const settingsForm = (

          <fieldset className="ex-form">
            <section className="ex-sec">
              <h3>Location</h3>
              <div className="ex-row">
                <span className="k">Folder</span>
                <span className="ex-path ex-path-tail mono" title={where}>
                  {`\u200E${where}\u200E`}
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
              {s.format !== 'original' && (
                <label className="ex-row">
                  <span className="k" />
                  <Checkbox checked={s.addToPhotos} onCheckedChange={(c) => set({ addToPhotos: c === true })} data-testid="export-to-photos" />
                  Add to Apple Photos, in the Shoot's album
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
                  className="w-[180px] shrink-0"
                />
              </div>
              {photos[0] && (
                <div className="ex-row">
                  <span className="k" />
                  <span className="ex-example mono faint" data-testid="export-example" title={example(s, photos[0].file, n)}>
                    {example(s, photos[0].file, n)}
                  </span>
                </div>
              )}
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
                    { value: 'heic', label: 'HEIC' },
                    { value: 'tiff', label: 'TIFF' },
                    { value: 'original', label: 'Original + XMP' },
                  ]}
                  className="w-[140px]"
                />
                {s.format === 'tiff' && <span className="v-chip">sRGB</span>}
              </div>
              {(s.format === 'jpeg' || s.format === 'heic') && (
                <div className="ex-row">
                  <span className="k">Color</span>
                  <Select
                    label="Color space"
                    value={s.colorSpace}
                    onValueChange={(colorSpace) => set({ colorSpace: colorSpace as ColorSpace })}
                    items={[
                      { value: 'srgb', label: 'sRGB' },
                      { value: 'display_p3', label: 'Display P3' },
                    ]}
                    className="w-[140px] shrink-0"
                  />
                  <span className="ex-note trunc">{s.colorSpace === 'srgb' ? 'Safe everywhere' : 'Wider color on Apple screens'}</span>
                </div>
              )}
              {(s.format === 'jpeg' || s.format === 'heic') && (
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
                    value={isCustom(s.size) ? 'custom' : s.size}
                    onValueChange={(size) => set({ size: size === 'custom' ? (`${s.customEdge}` as Size) : (size as Size) })}
                    items={[
                      { value: 'full', label: 'Full size' },
                      { value: '7680', label: 'Long edge 7680 px (8K)' },
                      { value: '4096', label: 'Long edge 4096 px' },
                      { value: '2048', label: 'Long edge 2048 px' },
                      { value: '1080', label: 'Long edge 1080 px' },
                      { value: 'short-1080', label: 'Short edge 1080 px' },
                      { value: 'mp-12', label: '12 megapixels' },
                      { value: 'custom', label: 'Custom long edge…' },
                    ]}
                    className="w-[180px] shrink-0"
                  />
                </div>
              ) : null}
              {s.format !== 'original' && isCustom(s.size) ? (
                <div className="ex-row">
                  <span className="k">Long edge</span>
                  <input
                    className="ex-path ex-num mono"
                    type="number"
                    min={64}
                    max={20000}
                    step={1}
                    defaultValue={s.customEdge}
                    aria-label="Custom long edge in pixels"
                    data-testid="export-custom-edge"
                    onChange={(e) => {
                      const v = Math.round(Number(e.target.value));
                      if (v >= 64 && v <= 20000) set({ size: `${v}` as Size, customEdge: v });
                    }}
                  />
                  <span className="ex-note">px, never upscaled</span>
                </div>
              ) : null}
              {s.format !== 'original' ? (
                <div className="ex-row">
                  <span className="k">Sharpen</span>
                  <Select
                    label="Output sharpening"
                    value={s.sharpen}
                    onValueChange={(sharpen) => set({ sharpen })}
                    items={[
                      { value: 'off', label: 'Off' },
                      { value: 'low', label: 'Screen, low' },
                      { value: 'standard', label: 'Screen, standard' },
                      { value: 'high', label: 'Screen, high' },
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

            {s.format !== 'original' && (
              <section className="ex-sec" data-testid="export-watermark">
                <h3>Watermark</h3>
                <label className="ex-row">
                  <span className="k" />
                  <Checkbox checked={s.watermark.on} onCheckedChange={(c) => set({ watermark: { ...s.watermark, on: c === true } })} data-testid="export-watermark-on" />
                  Add a watermark
                </label>
                {s.watermark.on && (
                  <>
                    <div className="ex-row">
                      <span className="k">Text</span>
                      <input className="ex-path" value={s.watermark.text} placeholder="© Your Name" aria-label="Watermark text" data-testid="export-watermark-text" onChange={(e) => set({ watermark: { ...s.watermark, text: e.target.value } })} />
                    </div>
                    <div className="ex-row">
                      <span className="k">Place</span>
                      <Select
                        label="Watermark place"
                        value={s.watermark.corner}
                        onValueChange={(corner) => set({ watermark: { ...s.watermark, corner: corner as Corner } })}
                        items={[
                          { value: 'bottom_right', label: 'Bottom right' },
                          { value: 'bottom_left', label: 'Bottom left' },
                          { value: 'top_right', label: 'Top right' },
                          { value: 'top_left', label: 'Top left' },
                          { value: 'center', label: 'Center' },
                        ]}
                        className="w-[140px] shrink-0"
                      />
                    </div>
                    <div className="ex-row">
                      <span className="k">Size</span>
                      <Slider style={{ width: 160 }} min={1} max={10} step={0.5} value={[s.watermark.size]} onValueChange={([v]) => set({ watermark: { ...s.watermark, size: v } })} aria-label="Watermark size" />
                      <span className="mono ex-note">{s.watermark.size}%</span>
                    </div>
                    <div className="ex-row">
                      <span className="k">Opacity</span>
                      <Slider style={{ width: 160 }} min={10} max={100} step={5} value={[Math.round(s.watermark.opacity * 100)]} onValueChange={([v]) => set({ watermark: { ...s.watermark, opacity: v / 100 } })} aria-label="Watermark opacity" />
                      <span className="mono ex-note">{Math.round(s.watermark.opacity * 100)}%</span>
                    </div>
                  </>
                )}
              </section>
            )}

            <section className="ex-sec">
              <h3>Metadata</h3>
              {s.format === 'original' ? (
                <div className="ex-row">
                  <span className="k">Include</span>
                  <span>Everything the camera wrote (the original is copied untouched)</span>
                </div>
              ) : (
                <>
                  <div className="ex-row">
                    <span className="k">Include</span>
                    <span>Camera, lens, capture time, Keywords and caption</span>
                  </div>
                  <label className="ex-row">
                    <span className="k" />
                    <Checkbox checked={s.removeLocation} onCheckedChange={(c) => set({ removeLocation: c === true })} data-testid="export-remove-location" />
                    Remove location
                  </label>
                </>
              )}
            </section>

            {mode && (
            <section className="ex-sec" data-testid="export-estimate">
              <h3>Estimate</h3>
              {s.format !== 'original' && first && (
                <div className="ex-row">
                  <span className="k">Each file</span>
                  <span className="mono">
                    {n > 1 && ests.some((e) => e.out[0] !== first[0] || e.out[1] !== first[1]) ? 'up to ' : ''}
                    {Math.max(...ests.map((e) => Math.max(...e.out)))} px{each != null ? ` · about ${bytes(each)}` : ''}
                  </span>
                </div>
              )}
              <div className="ex-row">
                <span className="k">All {n}</span>
                <span className="mono">
                  {total != null ? `about ${bytes(total)} · ` : ''}about {duration(est.secs)}
                </span>
              </div>
              {free != null && (
                <div className="ex-row">
                  <span className="k">Free</span>
                  <span className="mono" style={tight ? { color: 'var(--reject)' } : undefined}>
                    {bytes(free)}
                    {tight ? ' · not enough room' : ''}
                  </span>
                </div>
              )}
            </section>
            )}
          </fieldset>
  );
  useHotkeys('mod+enter', () => go(), { enabled: !!mode, preventDefault: true, enableOnFormTags: true }, [n, running, s, photos]);
  const facts = `${n} ${n === 1 ? 'photo' : 'photos'}${total != null ? ` · about ${bytes(total)}` : ''} · about ${duration(est.secs)}${free != null ? ` · ${bytes(free)} free` : ''}`;
  if (mode)
    return (
      <div className="export-v2 export-mode" data-testid="export-mode">
        {mode.head({ n, facts, start: go, running })}
        <div className="ex-body">
          {presetsNav}
          {mode.middle}
          {settingsForm}
        </div>
      </div>
    );
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="export-v2" data-testid="export-dialog" onOpenAutoFocus={(e) => {
        // Focus lands on Export (↵ runs it), not the first preset, which drew a focus ring on open.
        e.preventDefault();
        (e.currentTarget as HTMLElement).querySelector<HTMLElement>('[data-testid="export-start"]')?.focus();
      }} onKeyDown={(e) => e.key === 'Enter' && (e.metaKey || !(e.target instanceof HTMLInputElement)) && (e.preventDefault(), go())}>
        <div className="ex-head">
          <DialogTitle asChild>
            <h2>Export</h2>
          </DialogTitle>
          <span className="mono faint" style={{ fontSize: 11 }}>
            {n} {ids ? 'selected' : n === 1 ? 'Pick' : 'Picks'} · {shootFolder()}
          </span>
        </div>
        <div className="ex-body">
          {presetsNav}
          {settingsForm}
        </div>
        <div className="ex-foot">
          <span className="facts mono">
            {n} {n === 1 ? 'photo' : 'photos'}
            {total != null && each != null && ` · about ${bytes(total)}${n > 1 ? ` (${bytes(each)} each)` : ''}`} · about {duration(est.secs)}
            {free != null && <span style={tight ? { color: 'var(--reject)' } : undefined}>{` · ${bytes(free)} free`}</span>}
          </span>
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
