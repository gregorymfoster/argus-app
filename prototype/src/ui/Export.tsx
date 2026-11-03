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
import { PHOTOS } from '../data/dummy';
import { LIVE } from '../data/live';
import { call, pickFolder } from '../data/backend';
import { useLibrary } from '../store/library';
import { BUILT_IN, ColorSpace, Corner, estimate, example, exportArgs, isCustom, KEY, learn, load, matches, pixelsOf, Preset, PRESET_KEYS, PRESETS_KEY, read, Settings, shootFolder, Size, write } from '../data/export';

import { picksOf } from '../data/modes';
import type { Photo } from '../domain/types';
import { Kbd } from './kit';
import { EMPTY_ART } from './emptyArt';
import { startWork } from '../data/work';
import { useGooglePhotos } from '../data/googlePhotos';
import { openSettings } from '../settings/Settings';
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Slider } from '@/components/ui/slider';
import { Select } from '@/components/ui/select';

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

/** What the finished Export says: how many went where, and what failed. */
function ExportDone() {
  const r = useRun();
  if (!r) return null;
  const ok = r.done - r.failed.length;
  return (
    <div className="export-toast" aria-live="polite">
      <div className="export-done faint" data-testid="export-status">
        {!r.cancelled && !r.failed.length && <img className="empty-art" src={EMPTY_ART.done} alt="" />}
        <span>{`${r.cancelled ? 'Stopped. ' : 'Done. '}${ok} ${ok === 1 ? 'file' : 'files'} in ${r.folder ?? 'the export folder'}`}</span>
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

/** The result, once it ends. While it runs it shows with the rest of the background work. */
function showDone() {
  const r = run!;
  const title = r.cancelled ? 'Export stopped' : r.failed.length ? `Exported with ${r.failed.length} failed` : `Exported ${r.done - r.failed.length}`;
  const folder = r.folder;
  toast(title, {
    id: 'export',
    description: <ExportDone />,
    duration: 10000,
    action: LIVE && folder ? { label: 'Show in Finder', onClick: () => void reveal(folder) } : undefined,
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
  const work = startWork({ id: 'export', kind: 'export', label: `Exporting ${total}`, total, stop: () => (stop = true) });
  update();
  toast.dismiss('export');
  const paths: string[] = [];
  // Recorded as it goes, so a quit or crash part way finishes on the next start, from the
  // Photo it reached and each file written once (Greg, 2026-11-02). A closed lid just
  // pauses this loop; it carries on at wake.
  let runId: number | undefined;
  if (LIVE) {
    try {
      const { photo_ids: _, sequence_start: __, ...settings } = exportArgs(s, 0, 1, total);
      const send = s.format === 'original' ? [] : [...(s.addToPhotos ? ['apple_photos'] : []), ...(s.toGoogle ? ['google_photos'] : [])];
      runId = (await call<{ run_id: number }>('start_run', { kind: 'export', label: `Exporting ${total}`, args: { photo_ids: photos.map((p) => Number(p.id)), export: settings, ...(send.length ? { send, album: shootFolder() } : {}) } })).run_id;
    } catch {
      // An older engine: the Export still runs, just not resumably.
    }
  }
  // Two Photos at a time: one renders while the other encodes and writes (a render uses
  // every core, the JPEG encoder and the disk only one), and the Queue waits meanwhile.
  let next = 0;
  const one = async (i: number) => {
    const p = photos[i];
    r.current = p.file;
    update();
    work.update({ detail: p.file });
    if (LIVE) {
      try {
        const out = await call<{ exported: { path: string; bytes?: number; width?: number; height?: number }[]; failed: { error: string }[] }>('export_photos', { ...exportArgs(s, Number(p.id), i + 1, total), ...(runId ? { run_id: runId } : {}) });
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
      // Dummy data: a pretend export; checks slow it down to watch it run.
      await new Promise((res) => setTimeout(res, (window as { __argusExportDelay?: number }).__argusExportDelay ?? 90));
    }
    r.done += 1;
    update();
    work.update({ done: r.done });
  };
  const lane = async () => {
    while (next < total) {
      if (stop) {
        r.cancelled = true;
        return;
      }
      await one(next++);
    }
  };
  await Promise.all([lane(), lane()]);
  if (LIVE && paths.length) void call('enqueue_job', { kind: 'learn_edits', args: {}, dedupe_key: 'learn_edits' }).catch(() => {});
  r.finished = true;
  r.current = undefined;
  if (runId) await call('finish_run', { run_id: runId, stopped: r.cancelled }).catch(() => {});
  update();
  work.end();
  showDone();
  if (LIVE && s.reveal && r.folder && r.done > r.failed.length) void reveal(r.folder);
  if (s.format === 'original' || r.cancelled) return;
  const sent = LIVE ? paths : photos.map((p) => p.file);
  if (s.addToPhotos && sent.length) void sendTo('apple_photos', sent, shootFolder());
  if (s.toGoogle && sent.length) void sendTo('google_photos', sent, shootFolder());
}

const APP = { apple_photos: 'Apple Photos', google_photos: 'Google Photos' } as const;

/** Queue the exported files for Apple Photos or Google Photos: the upload runs as a
 *  background job, with its bar in the Status bar and Work mode. */
async function sendTo(to: keyof typeof APP, paths: string[], album: string) {
  const id = `export-${to}`;
  try {
    if (LIVE) await call('send_to_photos', { to, paths, album });
    toast(`Sending ${paths.length} to ${APP[to]}, album ${album}`, { id, description: 'Runs in the background, see Work (⌘5)', duration: 6000 });
  } catch (e) {
    toast(`Couldn't send to ${APP[to]}: ${String((e as Error).message ?? e).replace(/^Error: /, '')}`, { id, duration: 10000 });
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
  const google = useGooglePhotos();
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
            </section>

            {s.format !== 'original' && (
              <section className="ex-sec" data-testid="export-send-to">
                <h3>Send to</h3>
                <div className="ex-row">
                  <span className="k">Apps</span>
                  <label className="ex-app">
                    <Checkbox checked={s.addToPhotos} onCheckedChange={(c) => set({ addToPhotos: c === true })} data-testid="export-to-photos" />
                    Apple Photos
                  </label>
                  <label className="ex-app">
                    <Checkbox
                      checked={s.toGoogle}
                      onCheckedChange={(c) => {
                        set({ toGoogle: c === true });
                        if (c === true && !google?.connected) openSettings('export');
                      }}
                      data-testid="export-to-google"
                    />
                    Google Photos
                  </label>
                </div>
                {(s.addToPhotos || s.toGoogle) && (
                  <div className="ex-row">
                    <span className="k" />
                    {s.toGoogle && google && !google.connected ? (
                      <span className="ex-note trunc">
                        Google Photos isn't connected.{' '}
                        <button className="ex-link" onClick={() => openSettings('export')}>
                          Connect in Settings
                        </button>
                      </span>
                    ) : (
                      <span className="ex-note trunc" title={shootFolder()}>
                        Into the album <span className="mono">{shootFolder()}</span>, in the background
                      </span>
                    )}
                  </div>
                )}
              </section>
            )}

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
