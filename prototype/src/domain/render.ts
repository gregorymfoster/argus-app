// Turns a Recipe into CSS layers. A stand-in for the WGSL pipeline: global
// adjustments on the base layer, then one masked layer per local adjustment.

import type { Adjust, Correction, Look, LocalAdjust, MaskRef, NumKey, Photo, Recipe } from './types';

const NEUTRAL_CORRECTION: Correction = { exposure: 0, wbK: 0, face: 0 };

export function emptyRecipe(look: string | null = null): Recipe {
  return { schema: 1, global: {}, look, correction: { ...NEUTRAL_CORRECTION }, local: [] };
}

export function addAdjust(...parts: (Adjust | undefined)[]): Adjust {
  const out: Adjust = {};
  for (const p of parts) {
    if (!p) continue;
    for (const [k, v] of Object.entries(p) as [NumKey, unknown][]) {
      // The Color mixer, curves and grading are the engine's; dummy painting has only sliders.
      if (typeof v === 'number') out[k] = ((out[k] as number | undefined) ?? 0) + v;
    }
  }
  return out;
}

function correctionAdjust(c: Correction): Adjust {
  return { exposure: c.exposure, temp: c.wbK / 25 };
}

export interface LayerStyle {
  filter: string;
  overlays: { background: string; blend: string; opacity: number }[];
}

export function layerStyle(a: Adjust): LayerStyle {
  const ev = a.exposure ?? 0;
  const shadows = (a.shadows ?? 0) / 100;
  const highlights = (a.highlights ?? 0) / 100;
  const contrast = 1 + (a.contrast ?? 0) / 110 - (a.fade ?? 0) / 300;
  const sat = 1 + (a.saturation ?? 0) / 100;
  const bright = Math.pow(2, ev * 0.6) * (1 + highlights * 0.12) * (1 + Math.abs((a.temp ?? 0) / 100) * 0.3);
  const parts = [`brightness(${bright.toFixed(3)})`, `contrast(${contrast.toFixed(3)})`, `saturate(${sat.toFixed(3)})`];
  if ((a.skin ?? 0) > 0) parts.push(`blur(${((a.skin ?? 0) / 100) * 0.8}px)`);
  const overlays: LayerStyle['overlays'] = [];
  const t = (a.temp ?? 0) / 100;
  if (Math.abs(t) > 0.005) overlays.push({ background: t > 0 ? '#FFC48A' : '#9CC2FF', blend: 'multiply', opacity: Math.min(0.9, Math.abs(t) * 1.6) });
  const tint = (a.tint ?? 0) / 100;
  if (Math.abs(tint) > 0.005) overlays.push({ background: tint > 0 ? '#FF4FD8' : '#4FFF7A', blend: 'soft-light', opacity: Math.min(0.6, Math.abs(tint) * 0.6) });
  if (shadows > 0) overlays.push({ background: '#5A5A5A', blend: 'screen', opacity: shadows * 0.5 });
  if (shadows < 0) overlays.push({ background: '#000', blend: 'soft-light', opacity: -shadows * 0.6 });
  const fade = (a.fade ?? 0) / 100;
  if (fade > 0) overlays.push({ background: '#3A3530', blend: 'lighten', opacity: fade * 0.7 });
  const eyes = (a.eyes ?? 0) / 100;
  if (eyes > 0) overlays.push({ background: '#FFFFFF', blend: 'soft-light', opacity: eyes * 0.25 });
  return { filter: parts.join(' '), overlays };
}

const NONE = 'linear-gradient(transparent, transparent)';

/**
 * CSS mask-image for a Mask on a given Photo: a rough stand-in for dummy Photos. On the
 * real Library the engine renders Masks and the overlay uses its own (useEngineMasks).
 */
function maskImage(mask: MaskRef, photo: Photo): string {
  // Added components join as more layers; CSS masks can't subtract or intersect per layer,
  // so the stand-in leaves those out.
  const added = (mask.components ?? []).filter((c) => c.op === 'add').map((c) => maskImage(c, photo));
  return [shapeImage(mask, photo), ...added].join(', ');
}

function shapeImage(mask: MaskRef, photo: Photo): string {
  switch (mask.kind) {
    case 'luminance':
    case 'color':
      // Tones and colours need the pixels: a soft wash stands in for them.
      return 'linear-gradient(rgb(0 0 0 / 0.5), rgb(0 0 0 / 0.5))';
    case 'subject':
    case 'skin':
    case 'hair':
      if (!photo.faces.length) return NONE;
      return photo.faces.map((f) => `radial-gradient(${f.r * 220}% ${f.r * 420}% at ${f.x * 100}% ${(f.y + f.r * 1.6) * 100}%, #000 50%, transparent 100%)`).join(', ');
    case 'background':
      return 'linear-gradient(#000, #000)';
    case 'linear': {
      const deg = (Math.atan2(mask.to.x - mask.from.x, -(mask.to.y - mask.from.y)) * 180) / Math.PI;
      return `linear-gradient(${deg}deg, #000 0%, transparent 100%)`;
    }
    case 'radial':
      return `radial-gradient(${mask.rx * 100}% ${mask.ry * 100}% at ${mask.x * 100}% ${mask.y * 100}%, #000 ${(1 - (mask.feather ?? 0.5)) * 100}%, transparent 100%)`;
    case 'person':
    case 'faces': {
      const faces = mask.kind === 'person' && mask.person !== undefined ? photo.faces.filter((_, i) => i === mask.person) : mask.kind === 'faces' && mask.face !== undefined ? photo.faces.filter((_, i) => i === mask.face) : photo.faces;
      if (!faces.length) return NONE;
      if (mask.kind === 'person' && mask.parts.some((x) => x === 'whole' || x === 'body_skin'))
        return faces.map((f) => `radial-gradient(${f.r * 220}% ${f.r * 420}% at ${f.x * 100}% ${(f.y + f.r * 1.6) * 100}%, #000 50%, transparent 100%)`).join(', ');
      return faces
        .map((f) => {
          const rx = f.r * 100 * 1.25;
          const ry = f.r * 1.5 * 100 * 1.25;
          return `radial-gradient(${rx}% ${ry}% at ${f.x * 100}% ${f.y * 100}%, #000 55%, transparent 100%)`;
        })
        .join(', ');
    }
    case 'sky': {
      const end = Math.max(0.05, photo.horizon) * 100;
      return `linear-gradient(to bottom, #000 0%, #000 ${end * 0.8}%, transparent ${end + 4}%)`;
    }
    case 'gradient': {
      const dir = { left: 'to right', right: 'to left', bottom: 'to top' }[mask.side];
      return `linear-gradient(${dir}, #000 0%, transparent ${mask.extent * 100}%)`;
    }
    case 'brush':
      if (!mask.dots.length) return NONE;
      return mask.dots.map((d) => `radial-gradient(${d.r * 100}% ${d.r * 150}% at ${d.x * 100}% ${d.y * 100}%, #000 40%, transparent 100%)`).join(', ');
  }
}

/** The plain sliders of an adjustment times `k`: a Mask's Amount, roughly, for dummy Photos. */
function scaled(a: Adjust, k: number): Adjust {
  if (k === 1) return a;
  return Object.fromEntries(Object.entries(a).map(([key, v]) => [key, typeof v === 'number' ? v * k : v])) as Adjust;
}

export interface RenderPlan {
  base: Adjust;
  locals: { local: LocalAdjust; mask: string; adjust: Adjust }[];
  grain: number;
}

/** Everything a PhotoView needs: Look + Correction + Recipe flattened. */
export function plan(photo: Photo, recipe: Recipe, look: Look | null): RenderPlan {
  const base = addAdjust(look?.adjust, correctionAdjust(recipe.correction), recipe.global);
  const locals: RenderPlan['locals'] = [];
  if (recipe.correction.face) {
    const local: LocalAdjust = { id: 'corr-face', name: 'Face light', mask: { kind: 'faces' }, adjust: { exposure: recipe.correction.face }, source: 'ai' };
    locals.push({ local, mask: maskImage(local.mask, photo), adjust: addAdjust(base, local.adjust) });
  }
  for (const local of recipe.local) locals.push({ local, mask: maskImage(local.mask, photo), adjust: addAdjust(base, scaled(local.adjust, (local.amount ?? 100) / 100)) });
  return { base, locals, grain: base.grain ?? 0 };
}
