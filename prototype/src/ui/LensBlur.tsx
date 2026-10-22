// Lens Blur in Edit, after Effects, as mocked on the design canvas (2026-10-22): Amount,
// Bokeh, Boost, the Focal range (Subject, a Point picked on the photo, or a Custom range
// dragged on the depth histogram), Visualize depth, and the Refine brushes. A thin view over
// the Recipe's `lensBlur` (engine/src/lens_blur.rs); the depth comes from `view_depth`.

import { useEffect, useRef, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Circle, CircleDot, Crosshair, Disc, Pentagon, User } from 'lucide-react';
import type { Bokeh, LensBlur, Photo, Recipe } from '../domain/types';
import { call } from '../data/backend';
import { LIVE } from '../data/live';
import { Section } from './Develop';
import { Kbd, Seg, Slider } from './kit';
import { ProgressBar } from './Progress';
import type { Picked } from './EditSections';
import { Checkbox } from '@/components/ui/checkbox';
import { Button } from '@/components/ui/button';

export type Refine = 'off' | 'focus' | 'blur';

/** What the Edit screen keeps for Lens Blur: the depth view and the Refine brush. */
export interface LensBlurUi {
  visualize: boolean;
  onVisualize: (v: boolean) => void;
  refine: Refine;
  onRefine: (r: Refine) => void;
  /** Brush size, 0..100. */
  size: number;
  onSize: (n: number) => void;
  /** The depth map's histogram, focal range and image ([`useDepth`]). */
  depth: { info: DepthInfo | null; busy: boolean };
}

/** A Refine brush dot's radius (a share of the long edge) for a Size of 0..100. */
export const brushRadius = (size: number) => 0.008 + (size / 100) * 0.09;

export interface DepthInfo {
  /** The focal range in use, 0 (far) .. 100 (near). */
  focus: [number, number];
  /** Share of the frame at each depth, far to near. */
  histogram: number[];
  /** The depth map as an image, cool far to warm near (live only). */
  image?: string;
}

/** Lens Blur's depth for the open Photo, refreshed as the focal range or brushes change. */
export function useDepth(photoId: string, lb: LensBlur | undefined, on: boolean): { info: DepthInfo | null; busy: boolean } {
  const [info, setInfo] = useState<DepthInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const key = JSON.stringify([photoId, lb?.focus ?? null, on]);
  useEffect(() => {
    if (!on) return;
    if (!LIVE) {
      setInfo({ focus: lb?.focus ?? [62, 96], histogram: DUMMY_HISTOGRAM });
      return;
    }
    let live = true;
    setBusy(true);
    const t = setTimeout(() => {
      call<{ focus: [number, number]; histogram: number[]; images?: string[] }>('view_depth', { photo_id: Number(photoId), long_edge: 1600 })
        .then((r) => live && setInfo({ focus: r.focus, histogram: r.histogram, image: r.images?.[0] }))
        .catch((e) => console.error('view_depth failed', e))
        .finally(() => live && setBusy(false));
    }, 150);
    return () => {
      live = false;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return { info, busy };
}

const DUMMY_HISTOGRAM = [0, 0, 0, 0, 16, 63, 47, 63, 63, 16, 4, 7, 10, 15, 17, 23, 28, 35, 46, 57, 57, 57, 56, 52, 51, 45, 43, 38, 34, 27, 21, 10].map((v) => v / 1000);

const BOKEH: { value: Bokeh; label: string; title: string; Icon: typeof Circle }[] = [
  { value: 'circle', label: 'Circle', title: 'A clean disc, like a modern fast prime', Icon: Circle },
  { value: 'bubble', label: 'Bubble', title: 'Soap bubble: a disc with a bright rim, like older lenses', Icon: CircleDot },
  { value: 'blade', label: 'Blade', title: 'A five-blade aperture, like a stopped-down kit lens', Icon: Pentagon },
  { value: 'ring', label: 'Ring', title: 'A ring, like a mirror lens', Icon: Disc },
];

interface Props {
  photo: Photo;
  recipe: Recipe;
  set: (r: Recipe, label: string, key?: string) => void;
  onPick: (label: string, then: (p: Picked) => void) => void;
  ui: LensBlurUi;
}

export function LensBlurSection({ photo, recipe, set, onPick, ui }: Props) {
  const lb = recipe.lensBlur ?? {};
  const amount = lb.amount ?? 0;
  const on = amount > 0;
  const { info, busy } = ui.depth;
  const put = (next: LensBlur, label: string, key?: string) => {
    const clean: LensBlur = { ...next };
    if (clean.bokeh === 'circle') delete clean.bokeh;
    if (clean.boost === 50) delete clean.boost;
    if (!clean.focus) delete clean.focus;
    if (!clean.focusBrush?.length) delete clean.focusBrush;
    if (!clean.blurBrush?.length) delete clean.blurBrush;
    const r: Recipe = { ...recipe, lensBlur: clean };
    if (!clean.amount && Object.keys(clean).length <= 1) delete r.lensBlur;
    set(r, label, key);
  };
  const mode = lb.focus ? 'custom' : 'subject';
  const pickPoint = () =>
    onPick('Click where to focus', async ({ x, y }) => {
      // The click is on the cropped photo; the depth map covers the whole frame.
      const c = recipe.crop;
      const fx = c ? c.x + x * c.w : x;
      const fy = c ? c.y + y * c.h : y;
      let at: [number, number] = [Math.max(0, 100 - fy * 100 - 6), Math.min(100, 100 - fy * 100 + 6)];
      if (LIVE) {
        const r = await call<{ at: [number, number] }>('view_depth', { photo_id: Number(photo.id), x: fx, y: fy, long_edge: 64 });
        at = r.at;
      }
      put({ ...lb, focus: at }, 'Lens Blur focus on a point');
    });
  const brushes = (lb.focusBrush?.length ?? 0) + (lb.blurBrush?.length ?? 0);
  // F picks a point to focus on, ⇧Z shows the depth (Edit only, once Lens Blur is on).
  useHotkeys('f', () => on && pickPoint(), [on, recipe]);
  useHotkeys('shift+z', () => on && ui.onVisualize(!ui.visualize), [on, ui.visualize]);

  return (
    <Section id="lensblur" title="Lens Blur" closed aside={on ? <span className="mono dev-note">{amount}</span> : null}>
      <div className="slider-list">
        <Slider label="Amount" name="Lens Blur amount" min={0} max={100} value={amount} display={String(Math.round(amount))} onChange={(v) => put({ ...lb, amount: Math.round(v) }, `Lens Blur ${Math.round(v)}`, 'lensBlur.amount')} />
      </div>
      {on && busy && !info && (
        <div style={{ margin: '4px 0 8px' }}>
          <ProgressBar label="Finding depth" eta={3} testId="depth-progress" />
        </div>
      )}
      {on && (
        <div data-testid="lens-blur">
          <div className="dev-sub faint sub-head">Bokeh</div>
          <Seg
            value={lb.bokeh ?? 'circle'}
            onChange={(v) => put({ ...lb, bokeh: v }, `Bokeh: ${BOKEH.find((b) => b.value === v)!.label}`)}
            label="Bokeh shape"
            className="bokeh-seg"
            items={BOKEH.map(({ value, label, title, Icon }) => ({ value, title, label: <><Icon size={13} /> {label}</> }))}
          />
          <div className="slider-list" style={{ marginTop: 6 }}>
            <Slider label="Boost" name="Lens Blur boost" min={0} max={100} def={50} value={lb.boost ?? 50} display={String(Math.round(lb.boost ?? 50))} onChange={(v) => put({ ...lb, boost: Math.round(v) }, `Boost ${Math.round(v)}`, 'lensBlur.boost')} />
          </div>
          <div className="dev-sub" style={{ marginTop: 6 }}>
            <span className="faint">Focal range</span>
            <span className="grow" />
            <Seg
              value={mode}
              onChange={(v) => (v === 'subject' ? put({ ...lb, focus: undefined }, 'Lens Blur focus on the subject') : v === 'custom' && !lb.focus && info ? put({ ...lb, focus: info.focus }, 'Lens Blur custom range') : undefined)}
              label="Focal range"
              items={[
                { value: 'subject', title: 'Focus on the people the AI finds', label: <><User size={12} /> Subject</> },
                { value: 'custom', title: 'Your own range on the depth below', label: 'Custom' },
              ]}
            />
            <Button size="icon" variant="ghost" className="size-6" onClick={pickPoint} aria-label="Focus on a point (F)" title="Focus on a point: click the photo (F)" data-testid="lens-blur-point">
              <Crosshair size={12} />
            </Button>
          </div>
          <DepthBar histogram={info?.histogram ?? []} focus={info ? (lb.focus ?? info.focus) : null} onChange={(f) => put({ ...lb, focus: f }, 'Lens Blur focal range')} />
          <label className="dev-row" style={{ gap: 8 }}>
            <Checkbox checked={ui.visualize} onCheckedChange={(c) => ui.onVisualize(c === true)} data-testid="visualize-depth" />
            Visualize depth
            <span className="grow" />
            <Kbd quiet>⇧Z</Kbd>
          </label>
          <div className="dev-sub" style={{ marginTop: 4 }}>
            <span className="faint">Refine</span>
            <span className="grow" />
            {brushes > 0 && (
              <Button size="sm" variant="ghost" onClick={() => put({ ...lb, focusBrush: undefined, blurBrush: undefined }, 'Cleared Lens Blur brushes')}>
                Clear
              </Button>
            )}
            <Seg
              value={ui.refine}
              onChange={ui.onRefine}
              label="Refine brush"
              items={[
                { value: 'off', label: 'Off', title: 'No brush' },
                { value: 'focus', label: 'Focus', title: 'Paint what should stay sharp' },
                { value: 'blur', label: 'Blur', title: 'Paint what should blur as background' },
              ]}
            />
          </div>
          {ui.refine !== 'off' && (
            <>
              <div className="slider-list">
                <Slider label="Size" name="Refine brush size" min={0} max={100} def={30} value={ui.size} display={String(Math.round(ui.size))} onChange={ui.onSize} />
              </div>
              <div className="dev-note faint" style={{ marginTop: 2 }}>
                Paint on the photo. Hold ⌥ for {ui.refine === 'focus' ? 'Blur' : 'Focus'}, <Kbd quiet>[</Kbd> <Kbd quiet>]</Kbd> size the brush.
              </div>
            </>
          )}
        </div>
      )}
    </Section>
  );
}

/** The depth histogram far to near with the focal range's two handles, Lightroom's range picker. */
function DepthBar({ histogram, focus, onChange }: { histogram: number[]; focus: [number, number] | null; onChange: (f: [number, number]) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<{ end: 0 | 1; f: [number, number] } | null>(null);
  const shown = drag?.f ?? focus;
  // Square root, so a sky that fills half the frame doesn't flatten every other depth.
  const max = Math.sqrt(Math.max(1e-6, ...histogram));
  const at = (e: React.PointerEvent) => {
    const b = ref.current!.getBoundingClientRect();
    return Math.round(Math.min(100, Math.max(0, ((e.clientX - b.left) / b.width) * 100)));
  };
  return (
    <div className="depth-range" data-testid="depth-bar">
      <div
        ref={ref}
        className="depth-bar"
        onPointerDown={(e) => {
          if (!shown) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          const v = at(e);
          const end = Math.abs(v - shown[0]) <= Math.abs(v - shown[1]) ? 0 : 1;
          const f: [number, number] = [...shown];
          f[end] = v;
          setDrag({ end, f: [Math.min(f[0], f[1]), Math.max(f[0], f[1])] });
        }}
        onPointerMove={(e) => {
          if (!drag) return;
          const f: [number, number] = [...drag.f];
          f[drag.end] = at(e);
          if (f[0] > f[1]) return;
          setDrag({ ...drag, f });
        }}
        onPointerUp={() => {
          if (drag) onChange(drag.f);
          setDrag(null);
        }}
      >
        <div className="depth-hist" aria-hidden="true">
          {histogram.map((v, i) => (
            <i key={i} style={{ height: `${Math.max(4, (Math.sqrt(v) / max) * 100)}%` }} />
          ))}
        </div>
        {shown && (
          <>
            <div className="depth-out" style={{ left: 0, width: `${shown[0]}%` }} />
            <div className="depth-out" style={{ left: `${shown[1]}%`, right: 0 }} />
            <span className="depth-handle" style={{ left: `${shown[0]}%` }} role="slider" aria-label="Focal range far end" aria-valuenow={shown[0]} aria-valuemin={0} aria-valuemax={100} />
            <span className="depth-handle" style={{ left: `${shown[1]}%` }} role="slider" aria-label="Focal range near end" aria-valuenow={shown[1]} aria-valuemin={0} aria-valuemax={100} />
          </>
        )}
      </div>
      <div className="dev-sub faint depth-scale">
        <span>Far</span>
        <span className="grow" />
        <span className="mono">{shown ? `${Math.round(shown[0])} – ${Math.round(shown[1])}` : ''}</span>
        <span className="grow" />
        <span>Near</span>
      </div>
    </div>
  );
}
