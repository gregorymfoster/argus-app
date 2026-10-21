// The Heal tool, after Lightroom's Heal and Clone: click a spot on the photo and Argus
// covers it with a nearby patch it picks (the engine's `pick_spot_source`), shown as a
// dashed circle you can drag elsewhere. Heal blends the patch into its surroundings;
// Clone copies it as it is. Spots live in the Recipe (engine `heal::Spot`) on the uncropped
// frame, so the photo shows uncropped while the tool is open. Nothing is generated.

import { useRef, useState } from 'react';
import { RotateCcw, Shuffle, X } from 'lucide-react';
import type { Recipe, Spot } from '../domain/types';
import { useLibrary } from '../store/library';
import { Kbd, Seg, Slider } from './kit';
import { Button } from '@/components/ui/button';

/** Settings for the next Spot; the sliders edit the selected Spot instead when there is one. */
export interface HealBrush {
  mode: Spot['mode'];
  r: number;
  feather: number;
  opacity: number;
}

export const HEAL_BRUSH: HealBrush = { mode: 'heal', r: 0.015, feather: 50, opacity: 100 };
/** The engine's bounds on a Spot's radius (`heal::MIN_R`, `heal::MAX_R`). */
export const SPOT_R = { min: 0.002, max: 0.15 };

export const spotsOf = (r: Recipe) => r.spots ?? [];

/** A Spot id not yet used in this Recipe: `spot`, `spot-2`, ... as the engine names them. */
export function newSpotId(r: Recipe): string {
  const ids = new Set(spotsOf(r).map((s) => s.id));
  for (let n = 1; ; n++) {
    const id = n === 1 ? 'spot' : `spot-${n}`;
    if (!ids.has(id)) return id;
  }
}

export function HealPanel({ photoId, recipe, sel, onSel, brush, onBrush, onRepick }: { photoId: string; recipe: Recipe; sel: string | null; onSel: (id: string | null) => void; brush: HealBrush; onBrush: (b: HealBrush) => void; onRepick: (id: string) => void }) {
  const { dispatch } = useLibrary();
  const spots = spotsOf(recipe);
  const spot = spots.find((s) => s.id === sel);
  const save = (next: Spot[], label: string, key?: string) => dispatch({ type: 'recipe/set', id: photoId, recipe: { ...recipe, spots: next }, label, by: 'you', key });
  // With a Spot selected the controls change it, as Lightroom's do; otherwise the next one.
  const set = (patch: Partial<HealBrush>, label: string, key: string) => {
    onBrush({ ...brush, ...patch });
    if (spot) save(spots.map((s) => (s.id === spot.id ? { ...s, ...patch } : s)), label, `spot:${spot.id}:${key}`);
  };
  const shown = spot ?? brush;
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  return (
    <div className="dev-panel" data-testid="heal-panel">
      <div className="dev-title">
        <span>Heal</span>
        <span className="grow" />
        <Button size="sm" variant="outline" disabled={!spots.length} onClick={() => (onSel(null), save([], 'Removed every Spot'))}>
          <RotateCcw size={12} /> Reset
        </Button>
      </div>
      <Seg
        label="Heal or Clone"
        value={shown.mode}
        onChange={(mode) => set({ mode }, mode === 'heal' ? 'Heal' : 'Clone', 'mode')}
        items={[
          { value: 'heal', label: 'Heal', title: 'Blend the patch into the light and colour around the spot' },
          { value: 'clone', label: 'Clone', title: 'Copy the patch exactly, for edges and patterns' },
        ]}
      />
      <div className="slider-list" style={{ marginTop: 6 }}>
        <Slider label="Size" min={SPOT_R.min} max={SPOT_R.max} step={0.001} def={HEAL_BRUSH.r} value={shown.r} display={pct(shown.r)} onChange={(r) => set({ r }, 'Spot size', 'r')} />
        <Slider label="Feather" min={0} max={100} def={HEAL_BRUSH.feather} value={shown.feather} display={String(Math.round(shown.feather))} onChange={(feather) => set({ feather }, 'Spot feather', 'feather')} />
        <Slider label="Opacity" min={0} max={100} def={100} value={shown.opacity} display={String(Math.round(shown.opacity))} onChange={(opacity) => set({ opacity }, 'Spot opacity', 'opacity')} />
      </div>
      <div className="dev-note">
        Click a spot on the photo to cover it. Drag a circle to move it, or the dashed one to change the source. <Kbd quiet>[</Kbd>&nbsp;and&nbsp;<Kbd quiet>]</Kbd> size the brush, <Kbd quiet>/</Kbd> picks a new source.
      </div>
      <div className="mask-rows" role="listbox" aria-label="Spots" data-testid="spot-rows">
        {!spots.length && <div className="faint" style={{ fontSize: 12, padding: '4px 0' }}>No Spots yet.</div>}
        {spots.map((s, i) => (
          <div key={s.id} role="option" aria-selected={sel === s.id} className={`mask-row ${sel === s.id ? 'sel' : ''}`} onClick={() => onSel(sel === s.id ? null : s.id)}>
            <span className="grow trunc">
              Spot {i + 1} <span className="faint">· {s.mode === 'heal' ? 'Heal' : 'Clone'}</span>
            </span>
            <span className="mono dev-note">{s.source === 'ai' ? 'AI' : ''}</span>
            <button
              className="mask-x"
              aria-label={`New source for Spot ${i + 1}`}
              title="New source (/)"
              onClick={(e) => {
                e.stopPropagation();
                onRepick(s.id);
              }}
            >
              <Shuffle size={12} />
            </button>
            <button
              className="mask-x"
              aria-label={`Remove Spot ${i + 1}`}
              onClick={(e) => {
                e.stopPropagation();
                if (sel === s.id) onSel(null);
                save(spots.filter((x) => x.id !== s.id), `Removed Spot ${i + 1}`);
              }}
            >
              <X size={12} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * The Spots over the uncropped photo: each one's circle, its dashed source and the line
 * between. A click on bare photo adds a Spot there; dragging a circle moves it, dragging the
 * source moves where it copies from. `frame` is the photo's width over height, to keep the
 * circles round.
 */
export function HealOverlay({ spots, sel, frame, r, onAdd, onSel, onChange }: { spots: Spot[]; sel: string | null; frame: number; r: number; onAdd: (x: number, y: number) => void; onSel: (id: string | null) => void; onChange: (s: Spot, done: boolean) => void }) {
  const box = useRef<HTMLDivElement>(null);
  // The brush under the pointer, at the size the next Spot will have.
  const [hover, setHover] = useState<{ x: number; y: number } | null>(null);
  const at = (ev: { clientX: number; clientY: number }) => {
    const b = box.current!.getBoundingClientRect();
    const c = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 1e4) / 1e4;
    return { x: c((ev.clientX - b.left) / b.width), y: c((ev.clientY - b.top) / b.height) };
  };
  const drag = (s: Spot, part: 'spot' | 'from') => (e: React.PointerEvent) => {
    e.stopPropagation();
    e.preventDefault();
    onSel(s.id);
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const start = at(e);
    let last = s;
    const move = (ev: PointerEvent) => {
      const p = at(ev);
      const [dx, dy] = [p.x - start.x, p.y - start.y];
      const c = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 1e4) / 1e4;
      last = part === 'spot' ? { ...s, x: c(s.x + dx), y: c(s.y + dy) } : { ...s, from: { x: c(s.from.x + dx), y: c(s.from.y + dy) } };
      onChange(last, false);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      if (last !== s) onChange(last, true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };
  // A radius on the long edge, as % of the width and of the height.
  const rx = (v: number) => v * Math.max(1, 1 / frame) * 100;
  const ry = (v: number) => v * Math.max(1, frame) * 100;
  const ring = (x: number, y: number, v: number) => ({ left: `${x * 100 - rx(v)}%`, top: `${y * 100 - ry(v)}%`, width: `${2 * rx(v)}%`, height: `${2 * ry(v)}%` });
  return (
    <div
      ref={box}
      className="mask-handles heal-layer"
      data-testid="heal-overlay"
      onPointerMove={(e) => setHover(e.target === e.currentTarget || (e.target as Element).tagName === 'svg' ? at(e) : null)}
      onPointerLeave={() => setHover(null)}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.stopPropagation();
        const p = at(e);
        onAdd(p.x, p.y);
      }}
    >
      <svg className="spot-links" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden>
        {spots.map((s) => (
          <line key={s.id} x1={s.x * 100} y1={s.y * 100} x2={s.from.x * 100} y2={s.from.y * 100} className={s.id === sel ? 'sel' : ''} vectorEffect="non-scaling-stroke" />
        ))}
      </svg>
      {hover && <span className="radial-ring spot-ring brush" style={ring(hover.x, hover.y, r)} />}
      {spots.map((s, i) => (
        <span key={s.id}>
          <span className={`radial-ring spot-ring from ${s.id === sel ? 'sel' : ''}`} style={ring(s.from.x, s.from.y, s.r)} onPointerDown={drag(s, 'from')} title={`Source of Spot ${i + 1}`} />
          <span className={`radial-ring spot-ring ${s.id === sel ? 'sel' : ''}`} style={ring(s.x, s.y, s.r)} onPointerDown={drag(s, 'spot')} title={`Spot ${i + 1}`} data-testid="spot-ring" />
        </span>
      ))}
    </div>
  );
}
