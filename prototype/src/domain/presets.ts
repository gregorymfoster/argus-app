// Presets, after Lightroom's Presets panel: built-in starting points grouped as Lightroom
// groups them, applied at an Amount (0-200%). The library is data shared with the engine
// (engine/presets/builtin.json), and applying one follows engine/src/presets.rs exactly:
// numbers add to the Recipe (so a Preset stacks on Auto), `absolute` keys, hues and shape
// settings are set, point curves replace. What it changed is kept in `recipe.presets`, so
// a new Amount or taking it off undoes exactly that and keeps the person's own moves since.
// Look-style Presets replace each other; Adaptive ones (Masks) and the Edit panel's fixes stack.
// The person's own Presets (group Yours, engine `save_preset`) set values instead of adding
// them, and send the rest of their Settings groups back to neutral, as a paste does.
// `npm run preset-parity` checks this copy against the engine on every built-in Preset.

import builtin from '../../../engine/presets/builtin.json';
import type { Adjust, AppliedPreset, LocalAdjust, MaskRef, Photo, Recipe } from './types';
import { groupOf } from './settings';

interface PresetLocal {
  name: string;
  mask: MaskRef;
  adjust: Adjust;
}

export interface Preset {
  id: string;
  name: string;
  group: string;
  /** What it does and when it suits, one line. */
  why?: string;
  /** Global settings it adds (or sets, see `absolute`). */
  global?: Adjust;
  /** Top-level keys that are set rather than added (B&W's saturation). */
  absolute?: string[];
  /** Local adjustments it adds, each on a Mask the engine finds. */
  local?: PresetLocal[];
  /** The person's own only: the Settings groups it holds (engine Preset `groups`). */
  settings?: string[];
}

/** The group of the person's own Presets, above the built-ins. */
export const YOURS = 'Yours';
/** Settings groups a Preset can hold: spots and crop belong to one frame. */
export const PRESET_SETTINGS = ['wb', 'light', 'color', 'effects', 'detail', 'optics', 'calibration', 'retouch', 'masks'] as const;
/** Ticked when the person doesn't choose: all but White balance and Masks. */
export const DEFAULT_PRESET_SETTINGS: string[] = PRESET_SETTINGS.filter((g) => g !== 'wb' && g !== 'masks');

let yours: Preset[] = [];
/** The person's own Presets, as the Library has them (data/yourPresets keeps this current). */
export const yourPresets = () => yours;
export function setYourPresets(list: Preset[]) {
  yours = list;
}
const isYours = (p: Preset) => !!p.settings?.length;

export const PRESETS: Preset[] = builtin.presets as unknown as Preset[];
export const PRESET_GROUPS: string[] = builtin.groups;
export const presetById = (id: string) => PRESETS.find((p) => p.id === id);

/**
 * The Edit panel's one-click fixes (design v2): each toggles one adaptive change on and off.
 * Brighten faces uses a per-person Mask (Lightroom's Select People: face skin of everyone).
 */
export const FIXES: (Preset & { icon: 'sky' | 'face' | 'skin' | 'shadows' })[] = [
  { id: 'fix-sky', name: 'Enhance sky', group: 'Fixes', icon: 'sky', local: [{ name: 'Sky: Enhance', mask: { kind: 'sky' }, adjust: { exposure: -0.15, highlights: -30, saturation: 12, contrast: 8 } }] },
  { id: 'fix-faces', name: 'Brighten faces', group: 'Fixes', icon: 'face', local: [{ name: 'Faces: Brighten', mask: { kind: 'person', parts: ['face_skin'] }, adjust: { exposure: 0.25, shadows: 15 } }] },
  { id: 'fix-skin', name: 'Even skin', group: 'Fixes', icon: 'skin', global: { skin: 30 } },
  { id: 'fix-shadows', name: 'Lift shadows', group: 'Fixes', icon: 'shadows', global: { shadows: 30, blacks: 8 } },
];

const byId = (id: string) => yours.find((p) => p.id === id) ?? presetById(id) ?? FIXES.find((f) => f.id === id);
/** A Preset's slot by id; one of the person's since deleted still holds the Look slot. */
const slotOf = (id: string) => {
  const p = byId(id);
  return p ? slot(p) : id.startsWith('yours-') ? 'look' : null;
};

/** A ranking's order, best first (engine `rank_presets`), or null. */
export type Ranking = { ranked: { id: string; score: number; why?: string }[]; photos: number[]; provider: string; at: string };

/**
 * Four Looks for this Photo (Presets' Suggested row until the AI ranks the Shoot). With the Shoot's AI ranking, its top
 * Look-style Presets; without one, a guess from the Photo's light and people.
 */
export function recommended(photo: Photo, ranking?: Ranking | null): Preset[] {
  const picked = (ranking?.ranked ?? []).filter((r) => r.score > 0).map((r) => presetById(r.id)).filter((p): p is Preset => !!p && slot(p) === 'look');
  if (picked.length >= 4) return picked.slice(0, 4);
  const dark = photo.signals.exposure < -0.2;
  const people = photo.faces.length > 0;
  const ids = dark ? ['bright-airy', 'natural', 'warm-film', 'moody'] : people ? ['natural', 'film-portrait', 'bright-airy', 'portrait-soft'] : ['natural', 'land-clear', 'warm-film', 'moody'];
  const guess = ids.map((id) => presetById(id)!);
  return [...picked, ...guess.filter((g) => !picked.includes(g))].slice(0, 4);
}

/** Adaptive Presets and fixes stack, one of each; the rest replace the last. */
export function slot(p: Preset): string {
  return p.group.startsWith('Adaptive') || p.group === 'Fixes' ? p.id : 'look';
}

// --- applying, as engine/src/presets.rs ---

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v);
const POINT_CURVES = ['rgb', 'red', 'green', 'blue'];
const isPoints = (path: string[]) => path.length === 2 && path[0] === 'curve' && POINT_CURVES.includes(path[1]);

/** Sliders whose neutral value isn't 0 (engine MORE_SLIDERS defaults, curve splits...). */
const NEUTRAL: Record<string, number> = {
  vignetteMidpoint: 50, vignetteFeather: 50, grainSize: 25, grainRoughness: 50, sharpenRadius: 1, sharpenDetail: 25,
  noiseDetail: 50, colorNoiseDetail: 50, colorNoiseSmoothness: 50, lensVignetteMidpoint: 50,
  'curve.splitShadows': 25, 'curve.splitMidtones': 50, 'curve.splitHighlights': 75, 'curve.refineSat': 100, 'grading.blending': 50,
};
const neutral = (path: string[]) => NEUTRAL[path.join('.')] ?? 0;

/** Ranges the engine clamps to (engine Adjust::clamped); everything else is -100..100. */
const RANGE: Record<string, [number, number]> = {
  exposure: [-5, 5], fade: [0, 100], skin: [0, 100], eyes: [0, 100], teeth: [0, 100], grain: [0, 100], grainSize: [0, 100], grainRoughness: [0, 100],
  vignetteMidpoint: [0, 100], vignetteFeather: [0, 100], vignetteHighlights: [0, 100], sharpen: [0, 150], sharpenRadius: [0.5, 3],
  sharpenDetail: [0, 100], sharpenMasking: [0, 100], noise: [0, 100], noiseDetail: [0, 100], noiseContrast: [0, 100], colorNoise: [0, 100],
  colorNoiseDetail: [0, 100], colorNoiseSmoothness: [0, 100], removeCA: [0, 1], defringePurple: [0, 20], defringeGreen: [0, 20], lensVignetteMidpoint: [0, 100],
};
function clamp(path: string[], v: number): number {
  const last = path[path.length - 1];
  let r = RANGE[path.join('.')] ?? [-100, 100];
  if (path[0] === 'grading' && last === 'hue') r = [0, 360];
  else if (path[0] === 'grading' && (last === 'sat' || last === 'blending')) r = [0, 100];
  else if (path[0] === 'curve' && last.startsWith('split')) r = [0, 100];
  else if (path[0] === 'curve' && last === 'refineSat') r = [0, 100];
  return Math.min(r[1], Math.max(r[0], v));
}

const round = (x: number) => Math.round(x * 1000) / 1000;

function leaves(v: unknown, path: string[] = [], out: [string[], unknown][] = []): [string[], unknown][] {
  if (isObj(v)) {
    for (const [k, x] of Object.entries(v)) {
      const p = [...path, k];
      if (isObj(x) && !isPoints(p)) leaves(x, p, out);
      else out.push([p, x]);
    }
  }
  return out;
}

function get(v: unknown, path: string[]): unknown {
  return path.reduce<unknown>((o, k) => (isObj(o) ? o[k] : undefined), v);
}

/** `v` with `path` set to `x` (undefined deletes it), copying the objects on the way. */
function set(v: Obj, path: string[], x: unknown): Obj {
  const [k, ...rest] = path;
  const out = { ...v };
  if (!rest.length) {
    if (x === undefined) delete out[k];
    else out[k] = x;
  } else {
    out[k] = set(isObj(v[k]) ? (v[k] as Obj) : {}, rest, x);
  }
  return out;
}

function prune(v: Obj): Obj {
  const out: Obj = {};
  for (const [k, x] of Object.entries(v)) {
    if (isObj(x)) {
      const p = prune(x);
      if (Object.keys(p).length) out[k] = p;
    } else out[k] = x;
  }
  return out;
}

const scalePoints = (pts: unknown, amount: number) =>
  (Array.isArray(pts) ? (pts as [number, number][]) : []).map(([x, y]) => [x, round(Math.min(255, Math.max(0, x + (y - x) * amount)))]);

const localId = (p: Preset, i: number) => `preset-${p.id}-${i}`;

function putOn(r: Recipe, p: Preset, amount: number): Recipe {
  const entry: AppliedPreset = { id: p.id, amount, before: {}, after: {} };
  let global = r.global as Obj;
  const named = leaves(p.global ?? {});
  const todo: [string[], unknown][] = [];
  if (isYours(p)) {
    // A setting in its groups that it leaves out goes back to neutral, as a paste does.
    for (const [path, x] of leaves(global)) {
      const g = groupOf(path[0]);
      const covered = g != null && p.settings!.includes(g);
      if (covered && (typeof x === 'number' || isPoints(path)) && !named.some(([n]) => n.join('.') === path.join('.'))) todo.push([path, undefined]);
    }
  }
  todo.push(...named);
  for (const [path, x] of todo) {
    const old = get(global, path);
    let next: unknown;
    if (isPoints(path)) next = x === undefined ? [[0, 0], [255, 255]] : isYours(p) ? x : scalePoints(x, amount);
    else if (x !== undefined && typeof x !== 'number') next = x;
    else {
      const cur = typeof old === 'number' ? old : neutral(path);
      const last = path[path.length - 1];
      const v = x === undefined ? neutral(path) : (x as number);
      const to = last === 'hue' ? v : isYours(p) || neutral(path) !== 0 || (path.length === 1 && p.absolute?.includes(last)) ? cur + (v - cur) * amount : cur + v * amount;
      next = round(clamp(path, to));
    }
    const key = path.join('.');
    entry.before[key] = old ?? null;
    entry.after[key] = next;
    global = set(global, path, next);
  }
  const added: LocalAdjust[] = (p.local ?? []).map((l, i) => {
    let adjust: Obj = {};
    for (const [path, x] of leaves(l.adjust)) adjust = set(adjust, path, path[path.length - 1] === 'hue' ? x : round((x as number) * amount));
    return { id: localId(p, i), name: l.name, mask: l.mask, adjust: adjust as Adjust, source: 'you' };
  });
  const ids = new Set(added.map((l) => l.id));
  return {
    ...r,
    global: global as Adjust,
    local: [...r.local.filter((l) => !ids.has(l.id)), ...added],
    presets: [...(r.presets ?? []).filter((e) => e.id !== p.id), entry],
  };
}

const same = (a: unknown, b: unknown) => (typeof a === 'number' && typeof b === 'number' ? Math.abs(a - b) < 1e-6 : JSON.stringify(a) === JSON.stringify(b));

function takeOff(r: Recipe, id: string): Recipe {
  const e = r.presets?.find((x) => x.id === id);
  if (!e) return r;
  let global = r.global as Obj;
  for (const [key, after] of Object.entries(e.after)) {
    const path = key.split('.');
    const before = e.before[key] ?? null;
    const cur = get(global, path) ?? (typeof after === 'number' ? neutral(path) : undefined);
    let back: unknown;
    if (cur !== undefined && same(cur, after)) back = before === null ? undefined : before;
    else if (typeof cur === 'number' && typeof after === 'number' && !isPoints(path)) {
      // Moved since: keep the person's change, minus what the Preset added.
      back = round(cur - (after - (typeof before === 'number' ? before : neutral(path))));
    } else back = cur;
    global = set(global, path, back);
  }
  const prefix = `preset-${id}-`;
  const rest = (r.presets ?? []).filter((x) => x.id !== id);
  const out: Recipe = { ...r, global: prune(global) as Adjust, local: r.local.filter((l) => !l.id.startsWith(prefix)) };
  if (rest.length) out.presets = rest;
  else delete out.presets;
  return out;
}

/**
 * `base` with the preset applied at `amount` (0..2, Lightroom's Amount 0-200%), replacing a
 * Preset in the same slot (and this one, if it was on). Amount 0 takes it off.
 */
export function applyPreset(base: Recipe, p: Preset, amount = 1): Recipe {
  let r = base;
  for (const e of base.presets ?? []) if (slotOf(e.id) === slot(p)) r = takeOff(r, e.id);
  return amount > 0 ? putOn(r, p, Math.min(2, amount)) : r;
}

/** `recipe` without the preset: what it changed taken back off, its Masks gone. */
export function removePreset(recipe: Recipe, p: Preset): Recipe {
  return takeOff(recipe, p.id);
}

/** A Preset named `name` from `recipe`'s settings in `groups` (engine `presets::from_recipe`), for dummy data. */
export function presetFromRecipe(name: string, recipe: Recipe, groups: string[]): Preset {
  const global = Object.fromEntries(Object.entries(recipe.global).filter(([k]) => {
    const g = groupOf(k);
    return g != null && groups.includes(g);
  })) as Adjust;
  const local = groups.includes('masks') ? recipe.local.map((l) => ({ name: l.name || 'Mask', mask: l.mask, adjust: l.adjust })) : [];
  return { id: '', name: name.trim(), group: YOURS, why: '', global, local, settings: groups };
}

/** The applied Preset in `p`'s slot, if any, with its Amount. */
export function appliedIn(recipe: Recipe, s: string): { preset: Preset; amount: number } | null {
  for (const e of [...(recipe.presets ?? [])].reverse()) {
    const p = byId(e.id);
    if (p && slot(p) === s) return { preset: p, amount: e.amount };
  }
  return null;
}

/** True when the Preset is on this Recipe. */
export function hasPreset(recipe: Recipe, p: Preset): boolean {
  return !!recipe.presets?.some((e) => e.id === p.id);
}

/** A fix is on when it's applied, or (set by hand) its sliders are at least what it sets. */
export function hasFix(recipe: Recipe, p: Preset): boolean {
  if (hasPreset(recipe, p)) return true;
  if (p.local) return false;
  return Object.entries(p.global ?? {}).every(([k, v]) => ((recipe.global as Record<string, number>)[k] ?? 0) >= (v as number));
}
