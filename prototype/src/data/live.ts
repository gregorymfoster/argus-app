// Live mode loads real Photos from the engine in place of the painted dummy Shoot. The
// Library is the only source of truth for Verdicts, stars and Recipes: flags, stars, edits,
// Undo and Redo are engine calls (writeCull, saveRecipe, undoBatch) and screens show what
// they answer. It is
// always on in the desktop app, and in a browser with `?library=live` (through `argus serve`). backend.ts is the transport. The Agent's
// requests go to the engine (liveAgent.ts); the Look screen and Masks still run on
// prototype data.

import { OTHER_SHOOTS, PHOTOS, PHOTO_BY_ID, SHOOT, STACKS, STACK_BY_ID } from './dummy';
import { IN_APP, call, libraryUrl } from './backend';
import { loadRecipe } from './liveEdit';
import type { Look, Photo, Recipe, Verdict } from '../domain/types';

export const LIVE = IN_APP || new URLSearchParams(location.search).get('library') === 'live';

/** One Photo as `argus serve` returns it (engine::library::Photo). */
interface EnginePhoto {
  id: number;
  /** Changes whenever the Photo's Recipe does; null if it was never edited. */
  edit?: string | null;
  shoot_id: number;
  file_name: string;
  kind: string;
  captured_at: string;
  width: number;
  height: number;
  camera: string;
  shutter?: number | null;
  aperture?: number | null;
  iso?: number | null;
  focal?: number | null;
  lens?: string | null;
  preview: string;
  verdict: Verdict;
  verdict_by: 'ai' | 'person' | null;
  reason: string | null;
  stack: number | null;
  stack_rank: number | null;
  stars: number;
  stars_by: 'ai' | 'person' | null;
  best: number | null;
}

interface EngineShoot {
  id: number;
  name: string;
  photos: number;
}

/** Exposure settings as Lightroom writes them: 1/250, f/2.8, ISO 400, 35mm. */
export function exposureParts(p: Pick<EnginePhoto, 'shutter' | 'aperture' | 'iso' | 'focal'>) {
  const s = p.shutter;
  return {
    shutter: s ? (s < 0.4 ? `1/${Math.round(1 / s)}` : `${+s.toFixed(1)}s`) : null,
    aperture: p.aperture ? `f/${+p.aperture.toFixed(1)}` : null,
    iso: p.iso ? `ISO ${p.iso}` : null,
    focal: p.focal ? `${Math.round(p.focal)}mm` : null,
  };
}

function toPhoto(p: EnginePhoto, index: number): Photo {
  const x = exposureParts(p);
  const captured = p.captured_at.replace('T', ' ');
  const size = `${p.width}×${p.height}`;
  const join = (...parts: (string | null | undefined)[]) => parts.filter(Boolean).join(' · ');
  const exposure = join(x.shutter, x.aperture);
  const isoFocal = join(x.iso?.replace('ISO ', ''), x.focal);
  return {
    id: String(p.id),
    file: p.file_name,
    index,
    scene: { kind: 'lake', seed: p.id, people: [] },
    capture: { blur: 0, motion: 0, ev: 0, wb: 0, faceShadow: 0, shift: 0 },
    faces: [],
    horizon: 0.4,
    signals: { focus: 0, eyesOpen: [0, 0], exposure: 0, expression: 'Neutral' },
    aiVerdict: p.verdict_by === 'ai' ? p.verdict : 'unflagged',
    personVerdict: p.verdict_by === 'person' ? p.verdict : undefined,
    reason: p.reason ?? '',
    exif: exposure || isoFocal ? join(x.shutter, x.aperture, x.iso, x.focal) : join(p.camera, captured, size),
    meta: { camera: p.camera, lens: p.lens ?? undefined, captured, size, exposure: exposure || undefined, isoFocal: isoFocal || undefined },
    group: false,
    preview: libraryUrl(p.preview),
    pixels: p.width && p.height ? [p.width, p.height] : undefined,
    stackId: p.stack == null ? undefined : `s${p.stack}`,
    stackRank: p.stack_rank ?? undefined,
    aiStars: p.stars_by === 'ai' ? p.stars : undefined,
    personStars: p.stars_by === 'person' ? p.stars : undefined,
    best: p.best ?? undefined,
    edit: p.edit ?? null,
  };
}

const SHOOT_KEY = 'argus:shoot';
const CULL_KEY = 'argus:cull-after-load';

function remembered(key: string): string | null {
  try {
    return sessionStorage.getItem(key);
  } catch {
    return null;
  }
}

function remember(key: string, value: string | null) {
  try {
    if (value == null) sessionStorage.removeItem(key);
    else sessionStorage.setItem(key, value);
  } catch {
    // No storage (private window): the newest Shoot opens instead.
  }
}

/** Open another Shoot. The UI's data is loaded once at start, so this reloads. */
export function openShoot(id: string) {
  remember(SHOOT_KEY, id);
  location.hash = '#/cull';
  location.reload();
}

/**
 * Replace the dummy Shoot with the chosen Shoot (the newest by default). Call before
 * first render. Returns false when the Library is still empty.
 */
export async function loadLive(): Promise<boolean> {
  const [shoots] = await Promise.all([call<EngineShoot[]>('list_shoots'), loadProviders()]);
  const key = remembered(SHOOT_KEY) ?? '';
  if (shoots.length && key in COLLECTIONS) return loadCollection(key as Collection, shoots);
  const want = Number(key);
  const shoot = shoots.find((s) => s.id === want) ?? shoots[shoots.length - 1];
  if (!shoot) {
    for (const list of [PHOTOS, STACKS, OTHER_SHOOTS] as unknown[][]) list.splice(0, list.length);
    for (const k of Object.keys(PHOTO_BY_ID)) delete PHOTO_BY_ID[k];
    Object.assign(SHOOT, { id: 'none', name: 'No Shoots yet', count: 0, live: true });
    return false;
  }
  photosQuery = { shoot_id: shoot.id };
  const photos = (await call<EnginePhoto[]>('photos', photosQuery)).map(toPhoto);
  PHOTOS.splice(0, PHOTOS.length, ...photos);
  const stacks = new Map<string, string[]>();
  for (const p of photos) if (p.stackId) stacks.set(p.stackId, [...(stacks.get(p.stackId) ?? []), p.id]);
  STACKS.splice(0, STACKS.length, ...[...stacks].map(([id, photoIds]) => ({ id, photoIds })));
  for (const k of Object.keys(PHOTO_BY_ID)) delete PHOTO_BY_ID[k];
  for (const k of Object.keys(STACK_BY_ID)) delete STACK_BY_ID[k];
  for (const p of photos) PHOTO_BY_ID[p.id] = p;
  for (const s of STACKS) STACK_BY_ID[s.id] = s;
  liveShoot = shoot.id;
  Object.assign(SHOOT, { id: String(shoot.id), name: shoot.name, count: photos.length, live: true });
  const others = shoots.filter((s) => s.id !== shoot.id).reverse();
  OTHER_SHOOTS.splice(0, OTHER_SHOOTS.length, ...others.map((s) => ({ id: String(s.id), name: s.name, count: s.photos, live: false })));
  return true;
}

/** The `photos` query behind the grid: the open Shoot, or a Library-wide view. */
export let photosQuery: Record<string, unknown> = {};

/** Library-wide views, like Lightroom's All Photographs: every Photo, or every Pick. */
export const COLLECTIONS = { all: 'All photos', picks: 'Picks' } as const;
export type Collection = keyof typeof COLLECTIONS;

/** True when the grid shows a Library-wide view, not one Shoot (so Shoot-only work is off). */
export function inCollection(): boolean {
  return LIVE && SHOOT.id in COLLECTIONS;
}

async function loadCollection(c: Collection, shoots: EngineShoot[]): Promise<boolean> {
  photosQuery = c === 'picks' ? { verdict: 'pick' } : {};
  const photos = (await call<EnginePhoto[]>('photos', photosQuery)).map(toPhoto);
  PHOTOS.splice(0, PHOTOS.length, ...photos);
  const stacks = new Map<string, string[]>();
  for (const p of photos) if (p.stackId) stacks.set(p.stackId, [...(stacks.get(p.stackId) ?? []), p.id]);
  STACKS.splice(0, STACKS.length, ...[...stacks].map(([id, photoIds]) => ({ id, photoIds })));
  for (const k of Object.keys(PHOTO_BY_ID)) delete PHOTO_BY_ID[k];
  for (const k of Object.keys(STACK_BY_ID)) delete STACK_BY_ID[k];
  for (const p of photos) PHOTO_BY_ID[p.id] = p;
  for (const s of STACKS) STACK_BY_ID[s.id] = s;
  liveShoot = 0;
  Object.assign(SHOOT, { id: c, name: COLLECTIONS[c], count: photos.length, live: true });
  OTHER_SHOOTS.splice(0, OTHER_SHOOTS.length, ...shoots.slice().reverse().map((s) => ({ id: String(s.id), name: s.name, count: s.photos, live: false })));
  return true;
}

export interface ImportReport {
  shoot_id: number | null;
  shoot_name: string;
  found: number;
  imported: number;
  duplicates: number;
  failed: { file: string; error: string }[];
  total_ms: number;
}

/**
 * Import a folder (or just `files` in it) as a new Shoot and open it as soon as the engine
 * has made it, so Photos pop into the grid while the rest copy (Greg, like Lightroom). The
 * engine queues the cull itself when `cullAfter`, so leaving the page doesn't lose it.
 * Resolves with the report only when the Import ends before its Shoot could open (nothing
 * new, or an error).
 */
export async function liveImport(folder: string, shoot: string | null, cullAfter: boolean, files?: string[], rawOnly = false, copy?: boolean, bytes?: number, auto = false, suggest = cullAfter): Promise<ImportReport> {
  if (bytes) noteImportStarted(bytes, copy ?? true);
  const before = new Set((await call<EngineShoot[]>('list_shoots')).map((s) => s.id));
  await call('start_import', { folder, ...(shoot ? { shoot_name: shoot } : {}), ...(files ? { files } : {}), raw_only: rawOnly, cull: cullAfter, suggest, auto, provider: chosenProvider(), ...(copy != null ? { copy } : {}) });
  for (;;) {
    await new Promise((r) => setTimeout(r, 300));
    const fresh = (await call<EngineShoot[]>('list_shoots').catch(() => [])).find((s) => !before.has(s.id) && s.photos > 0);
    if (fresh) {
      openShoot(String(fresh.id));
      return new Promise(() => {}); // the page reloads
    }
    const r = await call<{ running: boolean; report?: ImportReport; error?: string }>('import_result').catch(() => ({ running: true }) as { running: boolean; report?: ImportReport; error?: string });
    if (r.running) continue;
    if (r.error) throw new Error(r.error);
    if (r.report?.shoot_id != null) {
      openShoot(String(r.report.shoot_id));
      return new Promise(() => {});
    }
    if (r.report) return r.report;
  }
}

// How long an Import takes, learned from the last one on this Mac: seconds per byte of
// originals, copying or reading in place. Before any, Greg's Mac baseline (2026-10-18):
// 266 A7R V RAWs (about 16.8 GB) copied with previews in 3 min 48 s.
const RATE_KEY = 'argus:import-rate:';
const STARTED_KEY = 'argus:import-started';
const DEFAULT_RATE = { copy: 228 / 16.8e9, add: 90 / 16.8e9 };

/** Seconds an Import of `bytes` of originals should take. */
export function importSeconds(bytes: number, copy: boolean): number {
  const mode = copy ? 'copy' : 'add';
  return bytes * (Number(stored(RATE_KEY + mode)) || DEFAULT_RATE[mode]);
}

function noteImportStarted(bytes: number, copy: boolean) {
  store(STARTED_KEY, JSON.stringify({ bytes, copy, at: Date.now() }));
}

/** Bytes of originals the running Import reads, as the dialog estimated them, or null. */
export function importingBytes(): number | null {
  const raw = stored(STARTED_KEY);
  return raw ? (JSON.parse(raw) as { bytes: number }).bytes : null;
}

/** The Import this page followed has ended: remember how fast it went. */
export function noteImportFinished() {
  const raw = stored(STARTED_KEY);
  store(STARTED_KEY, '');
  if (!raw) return;
  const { bytes, copy, at } = JSON.parse(raw) as { bytes: number; copy: boolean; at: number };
  const took = (Date.now() - at) / 1000;
  if (bytes > 1e8 && took > 5) store(RATE_KEY + (copy ? 'copy' : 'add'), String(took / bytes));
}

/** The running Import's progress, if one is filling the open Shoot. */
export async function runningImport(): Promise<{ label: string; done: number; total: number; eta_s: number | null } | null> {
  const r = await call<{ tasks: { kind: string; label: string; done: number; total: number; eta_s: number | null }[] }>('progress').catch(() => null);
  // Auto on import runs right after the last Photo lands.
  const t = r?.tasks.find((t) => t.kind === 'import') ?? r?.tasks.find((t) => t.kind === 'auto');
  return t ? { ...t, label: t.kind === 'import' ? 'Importing · photos appear as they land' : t.label } : null;
}

/** Add the open Shoot's Photos that landed since the page loaded. Returns how many there are now. */
export async function refreshLivePhotos(): Promise<number> {
  if (!liveShoot) return PHOTOS.length;
  const photos = (await call<EnginePhoto[]>('photos', { shoot_id: liveShoot })).map(toPhoto);
  // In capture order, as the engine lists them; Photos already shown keep their object.
  const next = photos.map((p) => {
    const had = PHOTO_BY_ID[p.id];
    if (!had) return (PHOTO_BY_ID[p.id] = p);
    had.index = p.index;
    return had;
  });
  PHOTOS.splice(0, PHOTOS.length, ...next);
  SHOOT.count = PHOTOS.length;
  return PHOTOS.length;
}

/** A memory card: a mounted volume with a DCIM folder. */
export interface Card {
  name: string;
  path: string;
}

export interface ScannedPhoto {
  path: string;
  file_name: string;
  kind: string;
  bytes: number;
  captured_at: string | null;
  camera: string | null;
  imported: boolean;
  /** The camera's JPEG of the same frame (RAW+JPEG); the RAW is the Photo. */
  jpeg: string | null;
}

export interface Scan {
  folder: string;
  photos: ScannedPhoto[];
  new: number;
  imported: number;
}

export interface NameSuggestion {
  name: string;
  by: 'ai' | 'date';
  note: string | null;
}

export interface ImportEstimate {
  photos: number;
  original_bytes: number;
  copy: boolean;
  copy_bytes: number;
  preview_bytes: number;
  needed_bytes: number;
  free_bytes: number;
}

export const estimateImport = (folder: string, files: string[], copy: boolean, rawOnly: boolean) => call<ImportEstimate>('estimate_import', { folder, files, copy, raw_only: rawOnly });

/** A Shoot name as it will be saved (`2026-10-18_Boat-Trip`), per the engine's naming rules. */
export const formatShootName = (title: string, capturedAt: string | null) =>
  call<{ shoot_name: string }>('shoot_name', { title, ...(capturedAt ? { captured_at: capturedAt } : {}) }).then((r) => r.shoot_name);

export const findCards = () => call<Card[]>('find_cards');
export const scanFolder = (folder: string) => call<Scan>('scan_folder', { folder });
export const thumbnail = (path: string) => call<{ images: string[] }>('thumbnail', { path, long_edge: 240 }).then((r) => r.images[0]);

/** Ask the default Provider to name a Shoot from a few of its photos, spread across it. */
export function suggestName(photos: ScannedPhoto[]): Promise<NameSuggestion> {
  const n = Math.min(4, photos.length);
  const paths = Array.from({ length: n }, (_, i) => photos[Math.floor(((i + 0.5) * photos.length) / n)].path);
  return call<NameSuggestion>('suggest_name', { paths, captured_at: photos.find((p) => p.captured_at)?.captured_at ?? null, provider: chosenProvider() });
}

/** True once, right after an import that asked for a cull of the Shoot now open. */
export function takeCullAfterLoad(): boolean {
  const want = remembered(CULL_KEY);
  if (want == null || want !== String(liveShoot)) return false;
  remember(CULL_KEY, null);
  return true;
}

let liveShoot = 0;

/** The Shoot open now (0 before load). */
export function liveShootId(): number {
  return liveShoot;
}

export interface CullReport {
  summary: string;
  picks: number;
  rejects: number;
  unflagged: number;
  stacks: number;
  rated: number;
  /** How many Photos got each number of stars (index 1 = one star). */
  stars: number[];
  best: number[];
  kept_person_verdicts: number;
  kept_person_ratings: number;
}

/** Ask the engine to AI cull the live Shoot (the model sees the whole Shoot at once). */
export async function liveCull(): Promise<CullReport> {
  return call<CullReport>('cull_shoot', { shoot_id: liveShoot, provider: chosenProvider() });
}

/** An AI Provider as the engine lists it (`providers` command). */
export interface Provider {
  id: string;
  label: string;
  model: string | null;
  note: string;
  /** Why it can't run right now ("Ollama isn't running"), as of the last `providers`. */
  down?: string;
}

const PROVIDER_KEY = 'argus:provider';
export const PROVIDERS: Provider[] = [];
let defaultProvider = 'ollama';

function stored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // No storage: the choice lasts until the next reload.
  }
}

let picked: string | null = stored(PROVIDER_KEY);

/** The Provider a cull will use: the person's pick, else the engine's default. */
export function chosenProvider(): string {
  const id = picked ?? defaultProvider;
  return PROVIDERS.some((p) => p.id === id) ? id : defaultProvider;
}

/** Who the AI is in UI copy: the chosen Provider's name in the app, Claude in the demo. */
export function aiName(): string {
  if (!LIVE) return 'Claude';
  return PROVIDERS.find((p) => p.id === chosenProvider())?.label ?? 'AI';
}

/** Why the chosen Provider can't run right now, or null when it can. */
export function providerDown(): string | null {
  if (!LIVE) return null;
  return PROVIDERS.find((p) => p.id === chosenProvider())?.down ?? null;
}

export function chooseProvider(id: string) {
  picked = id;
  store(PROVIDER_KEY, id);
}

/** Re-read the Providers, e.g. after a Claude API key is saved. */
export async function loadProviders() {
  const r = await call<{ default: string; providers: Provider[] }>('providers');
  defaultProvider = r.default;
  PROVIDERS.splice(0, PROVIDERS.length, ...r.providers);
}

/** The Agent's conversations for the open Shoot, kept across reloads and relaunches. */
export function savedChats<T>(): T | null {
  const raw = stored(`argus:chats:${liveShoot}`);
  try {
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

export function saveChats(chats: unknown) {
  if (liveShoot) store(`argus:chats:${liveShoot}`, JSON.stringify(chats));
}

/** A Photo's Verdict and stars as the Library has them (`photo_states`, engine::library::CullState). */
export interface CullState {
  id: number;
  verdict: Verdict;
  verdict_by: 'ai' | 'person' | null;
  stars: number;
  stars_by: 'ai' | 'person' | null;
}

/** What one write to the Library did: the batch Undo reverts, and the Photos as they are now. */
export interface Written {
  batch: number | null;
  photos: CullState[];
  /** Photos whose Recipe changed, with the Recipe the Library now has. */
  recipes?: Record<string, Recipe>;
  /** The Shoot's Looks as they are now, when the write could have changed one. */
  looks?: Look[];
}

// The Library is the only source of truth for Verdicts, stars and Recipes. Every write and every
// Undo or Redo runs through one queue, in the order the person made them, so an Undo
// never overtakes the flag it undoes.
let queue: Promise<unknown> = Promise.resolve();
function serial<T>(job: () => Promise<T>): Promise<T> {
  const p = queue.then(job);
  queue = p.catch(() => {});
  return p;
}

/** One change the person made: flags and stars on some Photos, saved as one batch. */
export interface CullWrite {
  label: string;
  flags: { ids: string[]; verdict: Verdict }[];
  stars: { ids: string[]; stars: number }[];
}

/** Save a change as one batch (`flag` and `rate`; later calls join the first one's batch). */
export function writeCull(w: CullWrite): Promise<Written> {
  return serial(async () => {
    let batch: number | null = null;
    const photos = new Map<number, CullState>();
    const calls = [...w.flags.map((f) => ['flag', { ids: f.ids.map(Number), verdict: f.verdict }] as const), ...w.stars.map((r) => ['rate', { ids: r.ids.map(Number), stars: r.stars }] as const)];
    for (const [command, args] of calls) {
      const r: { batch_id: number | null; photos: CullState[] } = await call(command, { ...args, label: w.label, ...(batch != null ? { batch_id: batch } : {}) });
      batch ??= r.batch_id;
      for (const p of r.photos) photos.set(p.id, p);
    }
    return { batch, photos: [...photos.values()] };
  });
}

/** Save the person's Recipe for a Photo as one batch (`edit`): one slider drag, one Mask. */
export function saveRecipe(photoId: string, recipe: Recipe, label: string): Promise<Written> {
  return serial(async () => {
    const r = await call<{ batch_id: number }>('edit', { photo_id: Number(photoId), recipe, label });
    return { batch: r.batch_id, photos: [] };
  });
}

/**
 * Run a tool as the person (`as_person`): the Look screen's sliders, Remove Look, Sync to
 * Look. Its changes are one batch of the person's own, which ⌘Z undoes. `recipes` are
 * the Photos whose Recipe it changed, read back afterwards.
 */
export function asPerson(tool: string, args: Record<string, unknown>, label: string, recipes: (r: Record<string, unknown>) => number[] = () => []): Promise<Written> {
  return serial(async () => {
    const r = await call<Record<string, unknown> & { batch_id: number }>('as_person', { tool, args, label });
    return { batch: r.batch_id, photos: [], recipes: await readRecipes(recipes(r)), looks: await listLooks() };
  });
}

/** The open Shoot's Looks (`list_looks`), each at its newest version. */
export const listLooks = () => call<Look[]>('list_looks', { shoot_id: liveShoot });

/** The Recipes the Library has for these Photos (Photos never edited are left out). */
export async function readRecipes(ids: (string | number)[]): Promise<Record<string, Recipe>> {
  const out: Record<string, Recipe> = {};
  for (const id of ids) {
    const r = await loadRecipe(String(id));
    if (r) out[id] = r;
  }
  return out;
}

/** A write something else already started (Sync to Look, accepting a Suggestion), as an Undo step. */
export function wrote(batch: Promise<number | null>): Promise<Written> {
  return serial(() => batch.then((b) => ({ batch: b, photos: [] })));
}

/**
 * Undo a batch in the Library and read back what it touched: Verdicts, stars and Recipes.
 * Redo is the same call on the Undo's batch.
 */
export function undoBatch(batch: number): Promise<Written> {
  return serial(async () => {
    const r = await call<{ batch_id: number; verdicts: number[]; ratings?: number[]; recipes: number[]; looks: number }>('undo_batch', { batch_id: batch });
    const ids = [...new Set([...r.verdicts, ...(r.ratings ?? [])])];
    const looks = r.looks || r.recipes.length ? await listLooks() : undefined;
    return { batch: r.batch_id, photos: ids.length ? await readCull(ids) : [], recipes: await readRecipes(r.recipes), looks };
  });
}

/** The Photos' Verdicts and stars as the Library has them now. */
export function readCull(ids: (string | number)[]): Promise<CullState[]> {
  return call<CullState[]>('photo_states', { ids: ids.map(Number) });
}

/** Rename a Shoot (engine `rename_shoot`): the stored name follows the naming rules and
 * its originals folder moves with it. Updates SHOOT / OTHER_SHOOTS in place. */
export async function renameShoot(id: string, name: string): Promise<string> {
  let stored: string;
  if (LIVE) {
    const r = await call<{ name: string }>('rename_shoot', { shoot_id: Number(id), name });
    stored = r.name;
  } else {
    const date = /^\d{4}-\d{2}-\d{2}/.exec(name)?.[0] ?? '2026-10-12';
    const title = name.replace(/^\d{4}-\d{2}-\d{2}[\s_-]*/, '').trim().replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '');
    stored = title ? `${date}_${title}` : date;
  }
  for (const s of [SHOOT, ...OTHER_SHOOTS]) if (s.id === id) s.name = stored;
  return stored;
}

/** A name for a Shoot from the configured local model, looking at a few of its previews. */
export async function suggestShootName(id: string): Promise<{ name: string; by: string; note: string | null }> {
  if (!LIVE) return { name: '2026-10-12_Lake-Tahoe', by: 'ai', note: null };
  return call('suggest_name', { shoot_id: Number(id), provider: chosenProvider() });
}

/** Show a Shoot's originals folder (or one Photo's original) in Finder. */
export async function revealInFinder(what: { shoot_id: number } | { photo_id: number }): Promise<void> {
  if (!LIVE) return;
  const r = await call<{ path: string; shown: boolean }>('reveal', what);
  if (!r.shown) throw new Error(`Couldn't open Finder at ${r.path}`);
}

const EXPORT_KEY = 'argus:export-after-load';

/** Open another Shoot and its Export dialog once it has loaded. */
export function openShootToExport(id: string) {
  remember(EXPORT_KEY, id);
  openShoot(id);
}

/** True once, right after `openShootToExport` reloaded into that Shoot. */
export function exportAfterLoad(): boolean {
  const want = remembered(EXPORT_KEY);
  if (want == null) return false;
  remember(EXPORT_KEY, null);
  return want === String(liveShoot);
}

/** Greg's "AI work on the photo I'm on goes first": its queued Suggestions jump the queue
 * at Full, then the next few in the filmstrip. Fire and forget; the demo has no queue. */
export function focusPhoto(id: string, nextIds: string[]) {
  if (!LIVE) return;
  void call('focus_photo', { photo_id: Number(id), next_ids: nextIds.map(Number) }).catch(() => {});
}

/** An AI request's error in words a photographer can act on; the engine's own text otherwise. */
export function aiError(e: unknown): string {
  const s = String(e).replace(/^Error: /, '');
  if (/can't reach Ollama/.test(s)) return 'Ollama isn’t running on this Mac. Open the Ollama app and ask again, or choose your Claude plan in Settings › AI.';
  if (/no Claude API key/.test(s)) return 'There’s no Claude API key yet. Add one in Settings › AI, or choose another model there.';
  if (/claude auth login/.test(s)) return `${s.replace(/ \(run `claude auth login`.*\)$/, '')}. If you aren’t signed in, run \`claude auth login\` in Terminal.`;
  return s;
}
