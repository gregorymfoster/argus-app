// Export's settings and what they mean, apart from the dialog that edits them: the saved
// settings and presets, file names, the size and time estimates (learned from real exports)
// and the arguments the engine's `export_photos` takes. The dialog, Export mode, Settings ›
// Export and the Status bar all read them here.

import { SHOOT } from './dummy';
import type { Photo } from '../domain/types';

type Format = 'jpeg' | 'heic' | 'tiff' | 'original';
type Naming = 'shoot' | 'sequence' | 'original';
/** Full, a long edge in px, `short-N` a short edge, `mp-N` megapixels. */
export type Size = 'full' | '7680' | '4096' | '2048' | '1080' | 'short-1080' | 'mp-12' | `${number}`;
/** The long edges the Size menu lists; any other number is a Custom long edge. */
const LISTED_EDGES = ['7680', '4096', '2048', '1080'];
export const isCustom = (size: Size) => /^\d+$/.test(size) && !LISTED_EDGES.includes(size);
/** Lightroom's Output sharpening for screen. */
type Sharpen = 'off' | 'low' | 'standard' | 'high';

export interface Settings {
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
  /** Upload the files to Google Photos when done, into an album named after the Shoot. */
  toGoogle: boolean;
}
export type ColorSpace = 'srgb' | 'display_p3';
export type Corner = 'bottom_right' | 'bottom_left' | 'top_right' | 'top_left' | 'center';

export interface Preset {
  id: string;
  label: string;
  set: Partial<Settings>;
}

export const BUILT_IN: Preset[] = [
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
export const PRESET_KEYS: (keyof Settings)[] = ['format', 'size', 'quality', 'naming', 'sharpen', 'colorSpace'];

const DEFAULTS: Settings = { folder: '~/Pictures/Argus/Exports', shootFolder: true, naming: 'shoot', format: 'jpeg', quality: 85, size: 'full', sharpen: 'off', reveal: false, removeLocation: true, customEdge: 6000, colorSpace: 'srgb', watermark: { on: false, text: '©', corner: 'bottom_right', size: 3, opacity: 0.6 }, addToPhotos: false, toGoogle: false };
export const KEY = 'argus-export';
export const PRESETS_KEY = 'argus-export-presets';

export function read<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function write(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    // No storage: the defaults come back next time.
  }
}

export const load = (): Settings => {
  const saved = read<Partial<Settings>>(KEY, {});
  return { ...DEFAULTS, ...saved, watermark: { ...DEFAULTS.watermark, ...saved.watermark } };
};

/** Where exports go and what happens after, as Settings › Export shows them. */
export type ExportPlace = Pick<Settings, 'folder' | 'shootFolder' | 'reveal' | 'addToPhotos' | 'toGoogle'>;
export const exportPlace = (): ExportPlace => {
  const s = load();
  return { folder: s.folder, shootFolder: s.shootFolder, reveal: s.reveal, addToPhotos: s.addToPhotos, toGoogle: s.toGoogle };
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

export function matches(p: Preset, s: Settings) {
  return Object.entries(p.set).every(([k, v]) => s[k as keyof Settings] === v);
}

const stem = (file: string) => file.replace(/\.[^.]+$/, '').replace(/\s+/g, '-');
/** The Shoot's folder name, as `engine::naming` makes it (no spaces, ever). */
export const shootFolder = () => SHOOT.name.replace(/[^\p{L}\p{N}_-]+/gu, '-').replace(/^-+|-+$/g, '');

/** What the first file will be called, as the engine names it (docs/naming.md). */
export function example(s: Settings, file: string, total: number): string {
  const ext = s.format === 'jpeg' ? 'jpg' : s.format === 'heic' ? 'heic' : s.format === 'tiff' ? 'tif' : (file.split('.').pop() ?? '');
  const shoot = shootFolder();
  const name = s.naming === 'original' ? stem(file) : s.naming === 'sequence' ? `${shoot}_${'1'.padStart(Math.max(3, String(total).length), '0')}` : `${shoot}_${stem(file)}`;
  return `${name}.${ext}`;
}

/** A Photo's pixels when the Library knows them; dummy data and unknowns are A7R V frames. */
export const pixelsOf = (p?: Photo): [number, number] => p?.pixels ?? [9504, 6336];
/** The pixels a Photo leaves at, for a size choice. Never upscales, as the engine. */
function outPixels(size: Size, [w, h]: [number, number]): [number, number] {
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
export function learn(s: Settings, bytesOut: number, px: number) {
  if (s.format === 'original' || !bytesOut || !px) return;
  const all = read<Record<string, number>>(LEARNED_KEY, {});
  const k = learnedKey(s);
  const bpp = bytesOut / px;
  // A running average, so one odd frame (a black sky) doesn't swing it.
  write(LEARNED_KEY, { ...all, [k]: all[k] ? all[k] * 0.7 + bpp * 0.3 : bpp });
}
/** What `export_photos` takes to export one Photo, the `n`th of `total`, with these settings. */
export function exportArgs(s: Settings, photoId: number, n: number, total: number): Record<string, unknown> {
  return {
    photo_ids: [photoId],
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
          sequence_start: n,
          sequence_total: total,
          // The caller queues learning from the edits once, after the last Photo.
          learn: false,
  };
}
