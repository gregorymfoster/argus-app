// Handles on the photo for placing a Linear or Radial Mask, as Lightroom draws them: a
// Linear Mask's two lines (full at the first, none at the second) and a Radial Mask's
// ellipse with its centre and edge. Drags report the moving Mask, then the final one.

import { useRef } from 'react';
import type { MaskRef } from '../domain/types';

type Placeable = Extract<MaskRef, { kind: 'linear' } | { kind: 'radial' }>;

export function MaskHandles({ mask, onChange }: { mask: Placeable; onChange: (m: Placeable, done: boolean) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const drag = (move: (x: number, y: number, m: Placeable) => Placeable) => (e: React.PointerEvent) => {
    e.stopPropagation();
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const start = mask;
    let last = mask;
    const at = (ev: PointerEvent) => {
      const b = box.current!.getBoundingClientRect();
      return [Math.min(1, Math.max(0, (ev.clientX - b.left) / b.width)), Math.min(1, Math.max(0, (ev.clientY - b.top) / b.height))] as const;
    };
    const onMove = (ev: PointerEvent) => {
      const [x, y] = at(ev);
      last = move(x, y, start);
      onChange(last, false);
    };
    const onUp = () => {
      el.removeEventListener('pointermove', onMove);
      el.removeEventListener('pointerup', onUp);
      onChange(last, true);
    };
    el.addEventListener('pointermove', onMove);
    el.addEventListener('pointerup', onUp);
  };
  const pct = (v: number) => `${v * 100}%`;

  if (mask.kind === 'radial') {
    const { x, y, rx, ry } = mask;
    return (
      <div ref={box} className="mask-handles" data-testid="mask-handles">
        <span className="radial-ring" style={{ left: pct(x - rx), top: pct(y - ry), width: pct(2 * rx), height: pct(2 * ry) }} />
        <span className="handle" title="Move" style={{ left: pct(x), top: pct(y) }} onPointerDown={drag((px, py, m) => ({ ...(m as typeof mask), x: px, y: py }))} />
        <span className="handle edge" title="Width" style={{ left: pct(x + rx), top: pct(y) }} onPointerDown={drag((px, _py, m) => ({ ...(m as typeof mask), rx: Math.max(0.02, Math.abs(px - (m as typeof mask).x)) }))} />
        <span className="handle edge" title="Height" style={{ left: pct(x), top: pct(y - ry) }} onPointerDown={drag((_px, py, m) => ({ ...(m as typeof mask), ry: Math.max(0.02, Math.abs(py - (m as typeof mask).y)) }))} />
      </div>
    );
  }
  const { from, to } = mask;
  // The two lines across the frame, perpendicular to from → to.
  const ang = Math.atan2(to.y - from.y, to.x - from.x) * (180 / Math.PI) + 90;
  return (
    <div ref={box} className="mask-handles" data-testid="mask-handles">
      <span className="linear-line full" style={{ left: pct(from.x), top: pct(from.y), transform: `translate(-50%, -50%) rotate(${ang}deg)` }} />
      <span className="linear-line" style={{ left: pct(to.x), top: pct(to.y), transform: `translate(-50%, -50%) rotate(${ang}deg)` }} />
      <span className="handle" title="Full effect" style={{ left: pct(from.x), top: pct(from.y) }} onPointerDown={drag((px, py, m) => ({ ...(m as typeof mask), from: { x: px, y: py } }))} />
      <span className="handle edge" title="No effect" style={{ left: pct(to.x), top: pct(to.y) }} onPointerDown={drag((px, py, m) => ({ ...(m as typeof mask), to: { x: px, y: py } }))} />
    </div>
  );
}
