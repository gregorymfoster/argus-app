// Settings groups for Copy, Paste and Sync settings, as in Lightroom's Copy Settings dialog.
// The engine's `paste_settings` tool (engine/src/paste.rs) is the real one and owns the
// rules; this copy of its groups names the checkboxes and pastes on dummy data.

import type { Recipe } from './types';

export type GroupId = 'wb' | 'light' | 'color' | 'effects' | 'detail' | 'optics' | 'calibration' | 'retouch' | 'masks' | 'spots' | 'crop';

export const GROUPS: { id: GroupId; label: string; default: boolean }[] = [
  { id: 'wb', label: 'White balance', default: true },
  { id: 'light', label: 'Light', default: true },
  { id: 'color', label: 'Color', default: true },
  { id: 'effects', label: 'Effects', default: true },
  { id: 'detail', label: 'Detail', default: true },
  { id: 'optics', label: 'Optics', default: true },
  { id: 'calibration', label: 'Calibration', default: true },
  { id: 'retouch', label: 'Retouch', default: true },
  { id: 'masks', label: 'Masks', default: false },
  { id: 'spots', label: 'Spot removal', default: false },
  { id: 'crop', label: 'Crop', default: false },
];

export const DEFAULT_GROUPS: GroupId[] = GROUPS.filter((g) => g.default).map((g) => g.id);
export const ALL_GROUPS: GroupId[] = GROUPS.map((g) => g.id);

/** The group a key of `global` belongs to (engine `paste::group_of`). */
export function groupOf(key: string): GroupId | null {
  if (key === 'temp' || key === 'tint') return 'wb';
  if (['exposure', 'contrast', 'highlights', 'shadows', 'whites', 'blacks', 'curve'].includes(key)) return 'light';
  if (['vibrance', 'saturation', 'hsl', 'grading', 'pointColor'].includes(key)) return 'color';
  if (['texture', 'clarity', 'dehaze', 'fade'].includes(key) || key.startsWith('vignette') || key.startsWith('grain')) return 'effects';
  if (key.startsWith('sharpen') || key.startsWith('noise') || key.startsWith('colorNoise') || key.startsWith('denoise')) return 'detail';
  if (key === 'removeCA' || key === 'distortion' || key.startsWith('defringe') || key.startsWith('lens')) return 'optics';
  if (key === 'calibration') return 'calibration';
  if (key === 'skin' || key === 'eyes' || key === 'teeth') return 'retouch';
  return null;
}

const correctionGroup = (k: string): GroupId | null => (k === 'exposure' || k === 'face' ? 'light' : k === 'wbK' ? 'wb' : null);

function pasteKeys<T extends object>(from: T, into: T, groups: GroupId[], group: (k: string) => GroupId | null): T {
  const chosen = (k: string) => {
    const g = group(k);
    return g != null && groups.includes(g);
  };
  const out = Object.fromEntries(Object.entries(into).filter(([k]) => !chosen(k)));
  for (const [k, v] of Object.entries(from)) if (chosen(k)) out[k] = v;
  return out as T;
}

/** `into` with `groups` of `from`'s settings in place of its own (dummy data; Looks aside). */
export function pasteRecipe(from: Recipe, into: Recipe, groups: GroupId[]): Recipe {
  const out: Recipe = {
    ...into,
    global: pasteKeys(from.global, into.global, groups, groupOf),
    correction: pasteKeys(from.correction, into.correction, groups, correctionGroup),
  };
  if (groups.includes('masks')) out.local = from.local;
  if (groups.includes('spots')) (out as { spots?: unknown }).spots = (from as { spots?: unknown }).spots;
  if (groups.includes('crop')) out.crop = from.crop;
  return out;
}

/** "Light, Color and 4 more", for menus and toasts. */
export function groupsLabel(groups: GroupId[]): string {
  if (groups.length === GROUPS.length) return 'all settings';
  const names = GROUPS.filter((g) => groups.includes(g.id)).map((g) => g.label);
  return names.length <= 2 ? names.join(' and ') : `${names.slice(0, 2).join(', ')} and ${names.length - 2} more`;
}
