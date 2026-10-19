// Presets, after Lightroom's Presets panel: a few built-in Looks that set sliders, and
// adaptive ones that add a Mask the engine finds by itself (subject, sky, people). A preset
// sets the sliders it names and keeps the rest of the Recipe, as Lightroom's do.

import type { Adjust, LocalAdjust, MaskRef, NumKey, Photo, Recipe } from './types';

export interface Preset {
  id: string;
  name: string;
  group: 'Looks' | 'Black and white' | 'Adaptive: Subject' | 'Adaptive: Sky' | 'Adaptive: Portrait' | 'Fixes';
  /** Global sliders it sets. */
  global?: Adjust;
  /** A local adjustment it adds (replacing one this preset added before). */
  local?: { mask: MaskRef; adjust: Adjust; name: string };
}

export const PRESETS: Preset[] = [
  { id: 'natural', name: 'Natural', group: 'Looks', global: { contrast: 8, highlights: -20, shadows: 20, vibrance: 10 } },
  { id: 'bright-airy', name: 'Bright and airy', group: 'Looks', global: { exposure: 0.35, contrast: -12, highlights: -30, shadows: 35, whites: 12, vibrance: 8, saturation: -8, temp: 4 } },
  { id: 'warm-film', name: 'Warm film', group: 'Looks', global: { contrast: -8, highlights: -18, shadows: 12, blacks: 18, temp: 16, tint: 4, vibrance: -6, saturation: -10 } },
  { id: 'moody', name: 'Moody', group: 'Looks', global: { exposure: -0.25, contrast: 22, highlights: -35, shadows: -10, blacks: -18, temp: -6, vibrance: -10, saturation: -15 } },
  { id: 'vivid', name: 'Vivid', group: 'Looks', global: { contrast: 18, highlights: -15, shadows: 10, whites: 10, blacks: -10, vibrance: 30, saturation: 8 } },
  { id: 'cool-clean', name: 'Cool and clean', group: 'Looks', global: { contrast: 6, highlights: -12, shadows: 10, whites: 8, temp: -14, vibrance: 6 } },
  { id: 'bw-soft', name: 'B&W soft', group: 'Black and white', global: { saturation: -100, vibrance: 0, contrast: -10, shadows: 20, highlights: -15 } },
  { id: 'bw-contrast', name: 'B&W contrast', group: 'Black and white', global: { saturation: -100, vibrance: 0, contrast: 35, whites: 15, blacks: -25 } },
  { id: 'subject-pop', name: 'Pop', group: 'Adaptive: Subject', local: { name: 'Subject: Pop', mask: { kind: 'subject' }, adjust: { exposure: 0.2, contrast: 12, shadows: 10, vibrance: 15 } } },
  { id: 'subject-light', name: 'Light', group: 'Adaptive: Subject', local: { name: 'Subject: Light', mask: { kind: 'subject' }, adjust: { exposure: 0.35, shadows: 20 } } },
  { id: 'bg-darken', name: 'Darken background', group: 'Adaptive: Subject', local: { name: 'Background: Darken', mask: { kind: 'background' }, adjust: { exposure: -0.35, saturation: -10 } } },
  { id: 'sky-blue', name: 'Blue drama', group: 'Adaptive: Sky', local: { name: 'Sky: Blue drama', mask: { kind: 'sky' }, adjust: { exposure: -0.2, highlights: -40, temp: -12, saturation: 15, contrast: 10 } } },
  { id: 'sky-warm', name: 'Golden sky', group: 'Adaptive: Sky', local: { name: 'Sky: Golden', mask: { kind: 'sky' }, adjust: { highlights: -30, temp: 22, tint: 6, saturation: 12 } } },
  { id: 'faces-brighten', name: 'Brighten faces', group: 'Adaptive: Portrait', local: { name: 'People: Brighten', mask: { kind: 'faces' }, adjust: { exposure: 0.3, shadows: 15 } } },
  { id: 'skin-smooth', name: 'Smooth skin', group: 'Adaptive: Portrait', global: { skin: 35 } },
  { id: 'eyes-teeth', name: 'Eyes and teeth', group: 'Adaptive: Portrait', global: { eyes: 40, teeth: 30 } },
];

export const PRESET_GROUPS = [...new Set(PRESETS.map((p) => p.group))];

/**
 * The Edit panel's one-click fixes (design v2): each toggles one adaptive change on and off.
 * Brighten faces uses a per-person Mask (Lightroom's Select People: face skin of everyone).
 */
export const FIXES: (Preset & { icon: 'sky' | 'face' | 'skin' | 'shadows' })[] = [
  { id: 'fix-sky', name: 'Enhance sky', group: 'Fixes', icon: 'sky', local: { name: 'Sky: Enhance', mask: { kind: 'sky' }, adjust: { exposure: -0.15, highlights: -30, saturation: 12, contrast: 8 } } },
  { id: 'fix-faces', name: 'Brighten faces', group: 'Fixes', icon: 'face', local: { name: 'Faces: Brighten', mask: { kind: 'person', parts: ['face_skin'] }, adjust: { exposure: 0.25, shadows: 15 } } },
  { id: 'fix-skin', name: 'Even skin', group: 'Fixes', icon: 'skin', global: { skin: 30 } },
  { id: 'fix-shadows', name: 'Lift shadows', group: 'Fixes', icon: 'shadows', global: { shadows: 30, blacks: 8 } },
];

/** The Recommended row: four Looks, the likeliest first for this Photo's light. */
export function recommended(photo: Photo): Preset[] {
  const by = (id: string) => PRESETS.find((p) => p.id === id)!;
  const dark = photo.signals.exposure < -0.2;
  const people = photo.faces.length > 0;
  const ids = dark ? ['bright-airy', 'natural', 'warm-film', 'moody'] : people ? ['natural', 'warm-film', 'bright-airy', 'moody'] : ['natural', 'vivid', 'warm-film', 'moody'];
  return ids.map(by);
}

const roundTo = (k: string, v: number) => (k === 'exposure' ? Math.round(v * 20) / 20 : Math.round(v));

/**
 * `base` with the preset applied at `amount` (0..1, Lightroom's preset Amount): each slider
 * it sets goes that far from the base's value toward the preset's, and its Mask's sliders scale.
 */
export function applyPreset(base: Recipe, p: Preset, amount = 1): Recipe {
  const global = { ...base.global };
  for (const [k, v] of Object.entries(p.global ?? {}) as [NumKey, number][]) {
    const from = (base.global[k] as number | undefined) ?? 0;
    global[k] = roundTo(k, from + (v - from) * amount);
  }
  let out: Recipe = { ...base, global };
  if (p.local) {
    const id = `preset-${p.id}`;
    const adjust: Adjust = {};
    for (const [k, v] of Object.entries(p.local.adjust) as [NumKey, number][]) adjust[k] = roundTo(k, v * amount);
    const local: LocalAdjust = { id, name: p.local.name, mask: p.local.mask, adjust, source: 'you' };
    out = { ...out, local: [...out.local.filter((l) => l.id !== id), local] };
  }
  return out;
}

/** `recipe` without the preset: its Mask gone and the sliders it sets back at 0. */
export function removePreset(recipe: Recipe, p: Preset): Recipe {
  const global = { ...recipe.global };
  for (const k of Object.keys(p.global ?? {}) as NumKey[]) delete global[k];
  return { ...recipe, global, local: recipe.local.filter((l) => l.id !== `preset-${p.id}`) };
}

/** A fix is on when its Mask is there, or its sliders are at least what it sets. */
export function hasFix(recipe: Recipe, p: Preset): boolean {
  if (p.local) return recipe.local.some((l) => l.id === `preset-${p.id}`);
  return Object.entries(p.global ?? {}).every(([k, v]) => ((recipe.global as Record<string, number>)[k] ?? 0) >= v);
}

/** True when the Recipe already carries everything this preset sets. */
export function hasPreset(recipe: Recipe, p: Preset): boolean {
  const g = Object.entries(p.global ?? {}).every(([k, v]) => (recipe.global as Record<string, number>)[k] === v);
  return g && (!p.local || recipe.local.some((l) => l.id === `preset-${p.id}`));
}
