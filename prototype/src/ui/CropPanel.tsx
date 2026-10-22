// The Crop tool (design pass v2), after Lightroom's: aspect ratios, Straighten, Reset, and
// the crop box on the uncropped photo, dragged to move it. Crops are fractions of the
// straightened frame, as the engine's Recipe has them. The Straighten tool (or ⌘-drag on
// the photo) is Lightroom's: draw along something that should be level or plumb, a
// horizon or a wall, and the photo turns to match.

import { useRef, useState, useSyncExternalStore } from 'react';
import { Ruler, RotateCcw } from 'lucide-react';
import type { Crop, Recipe } from '../domain/types';
import { useLibrary } from '../store/library';
import { Slider, Tip } from './kit';
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

// Whether the Straighten tool is out, shared by the panel's button and the overlay.
let ruler = false;
const rulerListeners = new Set<() => void>();
export function setRuler(on: boolean) {
  ruler = on;
  rulerListeners.forEach((l) => l());
}
/** Whether the Straighten tool is out (Esc puts it away first). */
export const rulerOut = () => ruler;
const useRuler = () => useSyncExternalStore((l) => (rulerListeners.add(l), () => void rulerListeners.delete(l)), () => ruler);

/**
 * The turn that makes a line drawn from (x1, y1) to (x2, y2) on screen level or plumb,
 * whichever it is closer to, as a change to the crop angle (positive turns clockwise).
 */
function straightenBy(x1: number, y1: number, x2: number, y2: number): number {
  const deg = (Math.atan2(y1 - y2, x2 - x1) * 180) / Math.PI; // y up: rising to the right is positive
  return deg - 90 * Math.round(deg / 90);
}

/** The largest centred crop of ratio `r` (width over height) in a frame of ratio `frame`. */
function cropTo(r: number | null, frame: number, angle = 0): Crop {
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
  const straighten = useRuler();
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
      <div className="crop-tools">
        <Tip label="Draw along a horizon or a wall to level the photo" keys="⌘ drag">
          <Button size="sm" variant={straighten ? 'default' : 'outline'} aria-pressed={straighten} onClick={() => setRuler(!straighten)} data-testid="crop-straighten">
            <Ruler size={12} /> Straighten tool
          </Button>
        </Tip>
      </div>
      <div className="dev-note">{straighten ? 'Draw along a line that should be level or upright. Esc puts the tool away.' : 'Drag the box on the photo to move the crop.'}</div>
    </div>
  );
}

/**
 * The crop box over the uncropped photo; dragging it moves the crop, committed on release.
 * With the Straighten tool out, or ⌘ held, a drag draws a line instead and turns the photo
 * so that line is level or plumb.
 */
export function CropOverlay({ crop, onChange }: { crop: Crop; onChange: (c: Crop, done: boolean, label?: string) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const straighten = useRuler();
  const [line, setLine] = useState<[number, number, number, number] | null>(null);
  const draw = (e: React.PointerEvent) => {
    e.stopPropagation();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const b = box.current!.getBoundingClientRect();
    const at = (ev: { clientX: number; clientY: number }) => [ev.clientX - b.left, ev.clientY - b.top] as const;
    const [x1, y1] = at(e);
    let end: [number, number, number, number] = [x1, y1, x1, y1];
    setLine(end);
    const move = (ev: PointerEvent) => {
      const [x2, y2] = at(ev);
      end = [x1, y1, x2, y2];
      setLine(end);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      setLine(null);
      const [, , x2, y2] = end;
      if (Math.hypot(x2 - x1, y2 - y1) < 24) return; // a click, not a line
      const angle = Math.round(Math.min(45, Math.max(-45, crop.angle + straightenBy(x1, y1, x2, y2))) * 10) / 10;
      setRuler(false);
      onChange({ ...crop, angle }, true, 'Straightened');
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };
  const down = (e: React.PointerEvent) => {
    if (straighten || e.metaKey || e.ctrlKey) return draw(e);
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
    <div
      ref={box}
      className={`mask-handles ${straighten ? 'straighten' : ''}`}
      data-testid="crop-overlay"
      onPointerDown={straighten ? draw : undefined}
    >
      <span className="crop-shade" style={{ left: pct(crop.x), top: pct(crop.y), width: pct(crop.w), height: pct(crop.h) }} onPointerDown={down}>
        <i style={{ left: '33.33%' }} />
        <i style={{ left: '66.66%' }} />
        <i className="h" style={{ top: '33.33%' }} />
        <i className="h" style={{ top: '66.66%' }} />
      </span>
      {line && (
        <svg className="straighten-line" data-testid="straighten-line">
          <line x1={line[0]} y1={line[1]} x2={line[2]} y2={line[3]} />
        </svg>
      )}
    </div>
  );
}
