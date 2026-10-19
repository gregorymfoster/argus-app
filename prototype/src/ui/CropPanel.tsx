// The Crop tool (design pass v2), after Lightroom's: aspect ratios, Straighten, Reset, and
// the crop box on the uncropped photo, dragged to move it. Crops are fractions of the
// straightened frame, as the engine's Recipe has them.

import { useRef } from 'react';
import { RotateCcw } from 'lucide-react';
import type { Crop, Recipe } from '../domain/types';
import { useLibrary } from '../store/library';
import { Slider } from './kit';
import { Button } from '@/components/ui/button';

const RATIOS: { label: string; r: number | null }[] = [
  { label: 'Original', r: null },
  { label: '1:1', r: 1 },
  { label: '4:5', r: 4 / 5 },
  { label: '5:4', r: 5 / 4 },
  { label: '2:3', r: 2 / 3 },
  { label: '3:2', r: 3 / 2 },
  { label: '16:9', r: 16 / 9 },
  { label: '9:16', r: 9 / 16 },
];

export const FULL: Crop = { angle: 0, x: 0, y: 0, w: 1, h: 1 };

/** The largest centred crop of ratio `r` (width over height) in a frame of ratio `frame`. */
export function cropTo(r: number | null, frame: number, angle = 0): Crop {
  if (!r) return { ...FULL, angle };
  const w = r < frame ? r / frame : 1;
  const h = r < frame ? 1 : frame / r;
  const round = (v: number) => Math.round(v * 1e4) / 1e4;
  return { angle, x: round((1 - w) / 2), y: round((1 - h) / 2), w: round(w), h: round(h) };
}

/** Which ratio a crop has, if one of ours. */
function ratioOf(c: Crop, frame: number): string | null {
  if (c.w >= 0.999 && c.h >= 0.999) return 'Original';
  const r = (c.w * frame) / c.h;
  return RATIOS.find((x) => x.r && Math.abs(x.r - r) < 0.01)?.label ?? null;
}

export function CropPanel({ photoId, recipe, frame }: { photoId: string; recipe: Recipe; frame: number }) {
  const { dispatch } = useLibrary();
  const crop = recipe.crop ?? FULL;
  const set = (c: Crop, label: string, key?: string) => dispatch({ type: 'recipe/set', id: photoId, recipe: { ...recipe, crop: c }, label, by: 'you', key });
  const current = ratioOf(crop, frame);
  return (
    <div className="dev-panel" data-testid="crop-panel">
      <div className="dev-title">
        <span>Crop</span>
        <span className="grow" />
        <Button size="sm" variant="outline" disabled={crop.angle === 0 && current === 'Original'} onClick={() => set(FULL, 'Reset crop')}>
          <RotateCcw size={12} /> Reset
        </Button>
      </div>
      <div className="dev-note">Aspect</div>
      <div className="ratio-grid" role="radiogroup" aria-label="Aspect ratio">
        {RATIOS.map((x) => (
          <button key={x.label} role="radio" aria-checked={current === x.label} className={current === x.label ? 'on' : ''} onClick={() => set(cropTo(x.r, frame, crop.angle), `Crop ${x.label}`)}>
            {x.label}
          </button>
        ))}
      </div>
      <div className="slider-list" style={{ marginTop: 6 }}>
        <Slider label="Straighten" min={-45} max={45} step={0.1} value={crop.angle} display={`${crop.angle.toFixed(1)}°`} onChange={(v) => set({ ...crop, angle: Math.round(v * 10) / 10 }, 'Straighten', 'angle')} />
      </div>
      <div className="dev-note">Drag the box on the photo to move the crop.</div>
    </div>
  );
}

/** The crop box over the uncropped photo; dragging it moves the crop, committed on release. */
export function CropOverlay({ crop, onChange }: { crop: Crop; onChange: (c: Crop, done: boolean) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const down = (e: React.PointerEvent) => {
    e.stopPropagation();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const b = box.current!.getBoundingClientRect();
    const [sx, sy] = [e.clientX, e.clientY];
    let last = crop;
    const move = (ev: PointerEvent) => {
      const x = Math.min(1 - crop.w, Math.max(0, crop.x + (ev.clientX - sx) / b.width));
      const y = Math.min(1 - crop.h, Math.max(0, crop.y + (ev.clientY - sy) / b.height));
      last = { ...crop, x: Math.round(x * 1e4) / 1e4, y: Math.round(y * 1e4) / 1e4 };
      onChange(last, false);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      onChange(last, true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };
  const pct = (v: number) => `${v * 100}%`;
  return (
    <div ref={box} className="mask-handles" data-testid="crop-overlay">
      <span className="crop-shade" style={{ left: pct(crop.x), top: pct(crop.y), width: pct(crop.w), height: pct(crop.h) }} onPointerDown={down}>
        <i style={{ left: '33.33%' }} />
        <i style={{ left: '66.66%' }} />
        <i className="h" style={{ top: '33.33%' }} />
        <i className="h" style={{ top: '66.66%' }} />
      </span>
    </div>
  );
}
