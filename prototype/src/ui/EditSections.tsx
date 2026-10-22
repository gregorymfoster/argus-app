// The rest of Lightroom's Edit panel, below Light and Color: Tone curve, Point color,
// Color grading, Effects, Detail, Optics and Calibration, in Lightroom's order and names.
// Each is a thin view over the Recipe's `global` (engine/src/recipe.rs), so the Agent and
// the person change the same settings.

import { useEffect, useRef, useState } from 'react';
import { Slider as SliderPrimitive } from 'radix-ui';
import { Pipette, Plus, X } from 'lucide-react';
import type { Adjust, Calibration, CurveChannel, GradeRange, Grading, NumKey, Photo, PointColor, Points, Wheel } from '../domain/types';
import { LIVE } from '../data/live';
import { baseChanged } from '../data/liveEdit';
import { usePoll } from '../data/poll';
import { ProgressBar } from './Progress';
import { fmt } from '../store/library';
import { Section } from './Develop';
import { Seg, Slider } from './kit';
import { Slider as UiSlider } from '@/components/ui/slider';
import { Button } from '@/components/ui/button';

export type Def = { key: NumKey; label: string; min: number; max: number; step?: number; track?: string; def?: number; name?: string };
/** Replace the global settings; `key` makes a drag one Undo step. */
export type SetGlobal = (g: Adjust, label: string, key?: string) => void;

export const EFFECTS: Def[] = [
  { key: 'texture', label: 'Texture', min: -100, max: 100 },
  { key: 'clarity', label: 'Clarity', min: -100, max: 100 },
  { key: 'dehaze', label: 'Dehaze', min: -100, max: 100 },
];
export const VIGNETTE: Def[] = [
  { key: 'vignette', label: 'Vignette', min: -100, max: 100 },
  { key: 'vignetteMidpoint', label: 'Midpoint', name: 'Vignette midpoint', min: 0, max: 100, def: 50 },
  { key: 'vignetteRoundness', label: 'Roundness', name: 'Vignette roundness', min: -100, max: 100 },
  { key: 'vignetteFeather', label: 'Feather', name: 'Vignette feather', min: 0, max: 100, def: 50 },
  { key: 'vignetteHighlights', label: 'Highlights', name: 'Vignette highlights', min: 0, max: 100 },
];
export const GRAIN: Def[] = [
  { key: 'grain', label: 'Grain', min: 0, max: 100 },
  { key: 'grainSize', label: 'Size', name: 'Grain size', min: 0, max: 100, def: 25 },
  { key: 'grainRoughness', label: 'Roughness', name: 'Grain roughness', min: 0, max: 100, def: 50 },
];
export const DENOISE: Def[] = [{ key: 'denoise', label: 'Denoise', name: 'AI Denoise amount', min: 0, max: 100 }];
export const SHARPEN: Def[] = [
  { key: 'sharpen', label: 'Sharpening', min: 0, max: 150 },
  { key: 'sharpenRadius', label: 'Radius', name: 'Sharpening radius', min: 0.5, max: 3, step: 0.1, def: 1 },
  { key: 'sharpenDetail', label: 'Detail', name: 'Sharpening detail', min: 0, max: 100, def: 25 },
  { key: 'sharpenMasking', label: 'Masking', name: 'Sharpening masking', min: 0, max: 100 },
];
export const NOISE: Def[] = [
  { key: 'noise', label: 'Noise red.', name: 'Luminance noise reduction', min: 0, max: 100 },
  { key: 'noiseDetail', label: 'Detail', name: 'Noise reduction detail', min: 0, max: 100, def: 50 },
  { key: 'noiseContrast', label: 'Contrast', name: 'Noise reduction contrast', min: 0, max: 100 },
];
export const COLOR_NOISE: Def[] = [
  { key: 'colorNoise', label: 'Color noise', name: 'Color noise reduction', min: 0, max: 100 },
  { key: 'colorNoiseDetail', label: 'Detail', name: 'Color noise detail', min: 0, max: 100, def: 50 },
  { key: 'colorNoiseSmoothness', label: 'Smoothness', name: 'Color noise smoothness', min: 0, max: 100, def: 50 },
];
export const DEFRINGE: Def[] = [
  { key: 'defringePurple', label: 'Purple', name: 'Defringe purple', min: 0, max: 20, track: 'linear-gradient(90deg, #555, #a35ad0)' },
  { key: 'defringeGreen', label: 'Green', name: 'Defringe green', min: 0, max: 20, track: 'linear-gradient(90deg, #555, #4fbf5a)' },
];
export const LENS: Def[] = [
  { key: 'distortion', label: 'Distortion', min: -100, max: 100 },
  { key: 'lensVignette', label: 'Vignetting', name: 'Lens vignetting', min: -100, max: 100 },
  { key: 'lensVignetteMidpoint', label: 'Midpoint', name: 'Lens vignetting midpoint', min: 0, max: 100, def: 50 },
];

/** `obj` with `path` set to `v`, dropping it (and emptied parents) when it is `def`. */
function setIn<T extends object>(obj: T, path: string[], v: unknown, def: unknown = 0): T {
  const [k, ...rest] = path;
  const cur = (obj as Record<string, unknown>)[k];
  const next = rest.length ? setIn((cur as object) ?? {}, rest, v, def) : v;
  const out = { ...obj } as Record<string, unknown>;
  const empty = next === undefined || next === def || (Array.isArray(next) ? next.length === 0 : typeof next === 'object' && next !== null && !Object.keys(next).length);
  if (empty) delete out[k];
  else out[k] = next;
  return out as T;
}

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** How many of `defs` are off their neutral value, for a section's summary. */
function changed(g: Adjust, defs: Def[]) {
  return defs.filter((d) => g[d.key] !== undefined && g[d.key] !== (d.def ?? 0)).length;
}

function Rows({ g, defs, set, dimUnless }: { g: Adjust; defs: Def[]; set: SetGlobal; dimUnless?: NumKey }) {
  const off = dimUnless !== undefined && !g[dimUnless];
  return (
    <div className="slider-list">
      {defs.map((d, i) => {
        const v = (g[d.key] as number | undefined) ?? d.def ?? 0;
        return (
          <div key={d.key} className={off && i > 0 ? 'dim-soft' : ''}>
            <Slider label={d.label} name={d.name} min={d.min} max={d.max} step={d.step} def={d.def} track={d.track} value={v} display={fmt(d.key, v)} onChange={(x) => set(setIn(g, [d.key], x, d.def ?? 0), `${d.label} ${fmt(d.key, x)}`, `g:${d.key}`)} />
          </div>
        );
      })}
    </div>
  );
}

function Note({ n }: { n: number }) {
  return n ? <span className="mono dev-note">{n} set</span> : null;
}

// ---------- Tone curve ----------

/** The engine's parametric curve (engine/src/color.rs `parametric`), for drawing it. */
function parametric(s: number, c: NonNullable<Adjust['curve']>) {
  const a = [c.shadows ?? 0, c.darks ?? 0, c.lights ?? 0, c.highlights ?? 0].map((v) => v / 100);
  const s1 = (c.splitShadows ?? 25) / 100;
  const s2 = (c.splitMidtones ?? 50) / 100;
  const s3 = (c.splitHighlights ?? 75) / 100;
  const mid = [s1 / 2, (s1 + s2) / 2, (s2 + s3) / 2, (s3 + 1) / 2];
  const bell = (t: number) => 0.5 - 0.5 * Math.cos(Math.min(1, Math.max(0, t)) * Math.PI);
  let out = s;
  a.forEach((v, k) => {
    if (!v) return;
    const lo = k === 0 ? 0 : mid[k - 1];
    const hi = k === 3 ? 1 : mid[k + 1];
    const w = s <= mid[k] ? bell((s - lo) / Math.max(1e-4, mid[k] - lo)) : bell((hi - s) / Math.max(1e-4, hi - mid[k]));
    out += v * 0.12 * w;
  });
  return Math.min(1, Math.max(0, out));
}

/** Points with the ends a curve keeps when it has no point there. */
function withEnds(p: Points): Points {
  const s = [...p].sort((a, b) => a[0] - b[0]);
  if (!s.length || s[0][0] > 0) s.unshift([0, 0]);
  if (s[s.length - 1][0] < 255) s.push([255, 255]);
  return s;
}

/** Monotone cubic through the points (Fritsch-Carlson), as the engine draws it. */
function pointCurve(p: Points): (x: number) => number {
  const q = withEnds(p);
  const n = q.length;
  const d = q.slice(0, -1).map((a, i) => (q[i + 1][1] - a[1]) / Math.max(1e-6, q[i + 1][0] - a[0]));
  const m = q.map((_, i) => (i === 0 ? d[0] : i === n - 1 ? d[n - 2] : d[i - 1] * d[i] <= 0 ? 0 : (d[i - 1] + d[i]) / 2));
  for (let i = 0; i < n - 1; i++) {
    if (d[i] === 0) {
      m[i] = m[i + 1] = 0;
      continue;
    }
    const a = m[i] / d[i];
    const b = m[i + 1] / d[i];
    const r = a * a + b * b;
    if (r > 9) {
      const t = 3 / Math.sqrt(r);
      m[i] = t * a * d[i];
      m[i + 1] = t * b * d[i];
    }
  }
  return (x) => {
    let i = 0;
    while (i < n - 2 && q[i + 1][0] <= x) i++;
    const h = q[i + 1][0] - q[i][0];
    const t = Math.min(1, Math.max(0, (x - q[i][0]) / h));
    const t2 = t * t;
    const t3 = t2 * t;
    const y = (2 * t3 - 3 * t2 + 1) * q[i][1] + (t3 - 2 * t2 + t) * h * m[i] + (-2 * t3 + 3 * t2) * q[i + 1][1] + (t3 - t2) * h * m[i + 1];
    return Math.min(255, Math.max(0, y));
  };
}

const CHANNEL_COLOR: Record<CurveChannel, string> = { rgb: 'var(--text)', red: '#e0604f', green: '#58c26b', blue: '#5b8de0' };
type CurveMode = 'parametric' | CurveChannel;

function CurveGraph({ mode, curve, onPoints }: { mode: CurveMode; curve: NonNullable<Adjust['curve']>; onPoints: (p: Points, done: boolean) => void }) {
  const svg = useRef<SVGSVGElement>(null);
  const pts = mode === 'parametric' ? [] : withEnds(curve[mode] ?? []);
  const f = mode === 'parametric' ? (x: number) => 255 * parametric(x / 255, curve) : pointCurve(curve[mode] ?? []);
  const path = Array.from({ length: 65 }, (_, i) => {
    const x = (i / 64) * 255;
    return `${i ? 'L' : 'M'}${x.toFixed(1)},${(255 - f(x)).toFixed(1)}`;
  }).join('');
  const at = (e: { clientX: number; clientY: number }): [number, number] => {
    const b = svg.current!.getBoundingClientRect();
    const x = Math.round(Math.min(255, Math.max(0, ((e.clientX - b.left) / b.width) * 255)));
    const y = Math.round(Math.min(255, Math.max(0, 255 - ((e.clientY - b.top) / b.height) * 255)));
    return [x, y];
  };
  const drag = (start: Points, i: number) => (e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const el = svg.current!;
    el.setPointerCapture(e.pointerId);
    let last = start;
    const lo = i > 0 ? start[i - 1][0] + 2 : 0;
    const hi = i < start.length - 1 ? start[i + 1][0] - 2 : 255;
    const move = (ev: PointerEvent) => {
      const [x, y] = at(ev);
      last = start.map((p, j) => (j === i ? [Math.min(hi, Math.max(lo, x)), y] : p)) as Points;
      onPoints(last, false);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      onPoints(last, true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };
  const add = (e: React.PointerEvent) => {
    if (mode === 'parametric') return;
    const [x] = at(e);
    if (pts.some((p) => Math.abs(p[0] - x) < 6)) return;
    // A new point lands on the curve, then follows the drag.
    const next = [...pts, [x, Math.round(f(x))] as [number, number]].sort((a, b) => a[0] - b[0]) as Points;
    onPoints(next, false);
    drag(next, next.findIndex((p) => p[0] === x))(e);
  };
  const remove = (i: number) => {
    if (i === 0 || i === pts.length - 1) return;
    onPoints(pts.filter((_, j) => j !== i), true);
  };
  return (
    <svg ref={svg} className="curve-graph" viewBox="-4 -4 263 263" onPointerDown={add} data-testid="curve-graph" role="img" aria-label="Tone curve">
      <rect x="0" y="0" width="255" height="255" className="curve-bg" />
      {[64, 128, 191].map((v) => (
        <g key={v} className="curve-grid">
          <line x1={v} y1="0" x2={v} y2="255" />
          <line x1="0" y1={v} x2="255" y2={v} />
        </g>
      ))}
      <line x1="0" y1="255" x2="255" y2="0" className="curve-diag" />
      <path d={path} fill="none" stroke={CHANNEL_COLOR[mode === 'parametric' ? 'rgb' : mode]} strokeWidth="2" />
      {pts.map((p, i) => (
        <circle key={i} cx={p[0]} cy={255 - p[1]} r="5" className="curve-pt" onPointerDown={drag(pts, i)} onDoubleClick={() => remove(i)} />
      ))}
    </svg>
  );
}

export function ToneCurve({ g, set }: { g: Adjust; set: SetGlobal }) {
  const [mode, setMode] = useState<CurveMode>('parametric');
  const c = g.curve ?? {};
  const [draft, setDraft] = useState<Points | null>(null);
  const shown = draft && mode !== 'parametric' ? { ...c, [mode]: draft } : c;
  const region = (k: 'highlights' | 'lights' | 'darks' | 'shadows', label: string) => {
    const v = c[k] ?? 0;
    return <Slider key={k} label={label} min={-100} max={100} value={v} display={fmt(k, v)} onChange={(x) => set(setIn(g, ['curve', k], x), `Curve ${label} ${fmt(k, x)}`, `curve.${k}`)} />;
  };
  const splits = [c.splitShadows ?? 25, c.splitMidtones ?? 50, c.splitHighlights ?? 75];
  const setSplits = (v: number[]) => {
    let next = g;
    (['splitShadows', 'splitMidtones', 'splitHighlights'] as const).forEach((k, i) => (next = setIn(next, ['curve', k], Math.round(v[i]), [25, 50, 75][i])));
    set(next, 'Curve splits', 'curve.splits');
  };
  const n = ['highlights', 'lights', 'darks', 'shadows', 'rgb', 'red', 'green', 'blue'].filter((k) => {
    const v = c[k as keyof typeof c];
    return Array.isArray(v) ? v.length : v;
  }).length;
  return (
    <Section id="curve" title="Tone curve" closed aside={<Note n={n} />}>
      <Seg
        value={mode}
        onChange={setMode}
        label="Curve"
        className="curve-modes"
        items={[
          { value: 'parametric', label: 'Region', title: 'Parametric curve' },
          { value: 'rgb', label: 'RGB', title: 'Point curve' },
          { value: 'red', label: <span style={{ color: CHANNEL_COLOR.red }}>R</span>, title: 'Red channel' },
          { value: 'green', label: <span style={{ color: CHANNEL_COLOR.green }}>G</span>, title: 'Green channel' },
          { value: 'blue', label: <span style={{ color: CHANNEL_COLOR.blue }}>B</span>, title: 'Blue channel' },
        ]}
      />
      <CurveGraph
        mode={mode}
        curve={shown}
        onPoints={(p, done) => {
          if (mode === 'parametric') return;
          if (!done) return setDraft(p);
          setDraft(null);
          const straight = p.length === 2 && p[0][0] === 0 && p[0][1] === 0 && p[1][0] === 255 && p[1][1] === 255;
          set(setIn(g, ['curve', mode], straight ? [] : p), `Curve ${mode === 'rgb' ? 'RGB' : cap(mode)}`);
        }}
      />
      {mode === 'parametric' ? (
        <>
          <SliderPrimitive.Root className="splits" min={5} max={95} step={1} minStepsBetweenThumbs={5} value={splits} onValueChange={setSplits} aria-label="Region splits">
            <SliderPrimitive.Track className="splits-track" />
            {splits.map((_, i) => (
              <SliderPrimitive.Thumb key={i} className="splits-thumb" aria-label={['Shadows split', 'Midtones split', 'Highlights split'][i]} />
            ))}
          </SliderPrimitive.Root>
          <div className="slider-list">
            {region('highlights', 'Highlights')}
            {region('lights', 'Lights')}
            {region('darks', 'Darks')}
            {region('shadows', 'Shadows')}
          </div>
        </>
      ) : (
        <div className="slider-list">
          <div className="dev-note faint" style={{ margin: '2px 0 4px' }}>
            Click to add a point, drag to move, double-click to remove.
          </div>
          <Slider label="Refine sat." min={0} max={100} def={100} value={c.refineSat ?? 100} display={`${Math.round(c.refineSat ?? 100)}`} onChange={(x) => set(setIn(g, ['curve', 'refineSat'], x, 100), `Refine saturation ${Math.round(x)}`, 'curve.refineSat')} />
        </div>
      )}
    </Section>
  );
}

// ---------- Color grading ----------

/** A colour wheel: angle is hue (0 = red, at 3 o'clock, going clockwise), distance is saturation. */
function ColorWheel({ w, size, label, onChange }: { w: Wheel; size: number; label: string; onChange: (w: Wheel, done: boolean) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const hue = w.hue ?? 0;
  const sat = w.sat ?? 0;
  const r = (sat / 100) * (size / 2 - 5);
  const x = size / 2 + r * Math.cos((hue * Math.PI) / 180);
  const y = size / 2 + r * Math.sin((hue * Math.PI) / 180);
  const down = (e: React.PointerEvent) => {
    e.preventDefault();
    const el = box.current!;
    el.setPointerCapture(e.pointerId);
    let last = w;
    const at = (ev: { clientX: number; clientY: number }) => {
      const b = el.getBoundingClientRect();
      const dx = ev.clientX - b.left - b.width / 2;
      const dy = ev.clientY - b.top - b.height / 2;
      const s = Math.min(100, (Math.hypot(dx, dy) / (b.width / 2 - 5)) * 100);
      last = { ...w, hue: Math.round(((Math.atan2(dy, dx) * 180) / Math.PI + 360) % 360), sat: Math.round(s) };
      onChange(last, false);
    };
    at(e);
    const move = (ev: PointerEvent) => at(ev);
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      onChange(last, true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };
  return (
    <div ref={box} className="wheel" style={{ width: size, height: size }} onPointerDown={down} onDoubleClick={() => onChange({ lum: w.lum }, true)} role="slider" aria-label={`${label} hue and saturation`} aria-valuetext={`hue ${hue}, saturation ${sat}`} aria-valuenow={sat} title={`${label}: hue ${hue}°, saturation ${sat} · double-click to reset`}>
      <span className="wheel-puck" style={{ left: x, top: y, background: sat ? `hsl(${hue} ${30 + sat * 0.6}% 55%)` : 'transparent' }} />
    </div>
  );
}

const RANGES: { k: GradeRange; label: string }[] = [
  { k: 'shadows', label: 'Shadows' },
  { k: 'midtones', label: 'Midtones' },
  { k: 'highlights', label: 'Highlights' },
  { k: 'global', label: 'Global' },
];

const SHORT: Record<GradeRange, string> = { shadows: 'Dark', midtones: 'Mid', highlights: 'Light', global: 'Global' };

export function ColorGrading({ g, set }: { g: Adjust; set: SetGlobal }) {
  const [view, setView] = useState<'all' | GradeRange>('all');
  const gr: Grading = g.grading ?? {};
  const [draft, setDraft] = useState<{ k: GradeRange; w: Wheel } | null>(null);
  const wheelOf = (k: GradeRange) => (draft?.k === k ? draft.w : (gr[k] ?? {}));
  const setWheel = (k: GradeRange, w: Wheel, label: string, key?: string) => {
    const clean: Wheel = {};
    if (w.sat) (clean.hue = w.hue ?? 0), (clean.sat = w.sat);
    if (w.lum) clean.lum = w.lum;
    set(setIn(g, ['grading', k], clean), label, key);
  };
  const onWheel = (k: GradeRange, label: string) => (w: Wheel, done: boolean) => {
    if (!done) return setDraft({ k, w });
    setDraft(null);
    setWheel(k, w, `Grading ${label}`);
  };
  const lum = (k: GradeRange, label: string) => {
    return (v: number) => setWheel(k, { ...gr[k], lum: v }, `Grading ${label} luminance ${fmt('x', v)}`, `grading.${k}.lum`);
  };
  const set1 = (k: GradeRange, part: 'hue' | 'sat', v: number, label: string) => setWheel(k, { ...gr[k], [part]: v }, `Grading ${label} ${part}`, `grading.${k}.${part}`);
  const n = RANGES.filter(({ k }) => gr[k]?.sat || gr[k]?.lum).length;
  return (
    <Section id="grading" title="Color grading" closed aside={n ? <span className="mono dev-note">{RANGES.filter(({ k }) => gr[k]?.sat || gr[k]?.lum).map((r) => r.label.toLowerCase()).join(' · ')}</span> : null}>
      <Seg value={view} onChange={setView} label="Grading range" className="curve-modes" items={[{ value: 'all', label: 'All', title: 'All four wheels' }, ...RANGES.map((r) => ({ value: r.k, label: SHORT[r.k], title: r.label }))]} />
      {view === 'all' ? (
        <div className="wheels" data-testid="grading-wheels">
          {RANGES.map(({ k, label }) => (
            <div key={k} className="wheel-cell">
              <span className="faint">{label}</span>
              <ColorWheel w={wheelOf(k)} size={86} label={label} onChange={onWheel(k, label)} />
              <UiSlider min={-100} max={100} value={[gr[k]?.lum ?? 0]} onValueChange={([v]) => lum(k, label)(v)} aria-label={`${label} luminance`} bipolar />
            </div>
          ))}
        </div>
      ) : (
        (() => {
          const r = RANGES.find((x) => x.k === view)!;
          const w = wheelOf(view);
          return (
            <>
              <div className="wheel-one">
                <ColorWheel w={w} size={150} label={r.label} onChange={onWheel(view, r.label)} />
              </div>
              <div className="slider-list">
                <Slider label="Hue" min={0} max={359} value={w.hue ?? 0} display={`${Math.round(w.hue ?? 0)}°`} track="linear-gradient(90deg, hsl(0 65% 55%), hsl(60 65% 55%), hsl(120 65% 55%), hsl(180 65% 55%), hsl(240 65% 55%), hsl(300 65% 55%), hsl(359 65% 55%))" onChange={(v) => set1(view, 'hue', v, r.label)} />
                <Slider label="Saturation" min={0} max={100} value={w.sat ?? 0} display={`${Math.round(w.sat ?? 0)}`} onChange={(v) => set1(view, 'sat', v, r.label)} />
                <Slider label="Luminance" min={-100} max={100} value={w.lum ?? 0} display={fmt('x', w.lum ?? 0)} onChange={lum(view, r.label)} />
              </div>
            </>
          );
        })()
      )}
      <div className="slider-list" style={{ marginTop: 6 }}>
        <Slider label="Blending" min={0} max={100} def={50} value={gr.blending ?? 50} display={`${Math.round(gr.blending ?? 50)}`} onChange={(v) => set(setIn(g, ['grading', 'blending'], v, 50), `Grading blending ${Math.round(v)}`, 'grading.blending')} />
        <Slider label="Balance" min={-100} max={100} value={gr.balance ?? 0} display={fmt('x', gr.balance ?? 0)} onChange={(v) => set(setIn(g, ['grading', 'balance'], v), `Grading balance ${fmt('x', v)}`, 'grading.balance')} />
      </div>
    </Section>
  );
}

// ---------- Point color ----------

export function PointColors({ g, set, onPick }: { g: Adjust; set: SetGlobal; onPick: () => void }) {
  const list = g.pointColor ?? [];
  const [sel, setSel] = useState(list.length - 1);
  // A newly picked colour becomes the one being adjusted.
  const [seen, setSeen] = useState(list.length);
  if (seen !== list.length) {
    setSeen(list.length);
    if (list.length > seen) setSel(list.length - 1);
  }
  const i = sel >= 0 && sel < list.length ? sel : list.length - 1;
  const p = list[i];
  const put = (q: PointColor | null, label: string, key?: string) => {
    const next = q ? list.map((x, j) => (j === i ? q : x)) : list.filter((_, j) => j !== i);
    set(setIn(g, ['pointColor'], next), label, key);
    if (!q) setSel(Math.max(0, i - 1));
  };
  const swatch = (q: PointColor) => `hsl(${q.hue} ${q.sat}% ${q.lum}%)`;
  return (
    <div data-testid="point-color">
      <div className="dev-sub">
        <div className="point-swatches">
          {list.map((q, j) => (
            <button key={j} className={`point-swatch ${j === i ? 'on' : ''}`} style={{ background: swatch(q) }} onClick={() => setSel(j)} aria-label={`Point color ${j + 1}`} aria-pressed={j === i} />
          ))}
        </div>
        <span className="grow" />
        <Button size="sm" variant="ghost" onClick={onPick} data-testid="point-pick" title="Pick a colour from the photo">
          <Pipette size={12} /> {list.length ? <Plus size={11} /> : 'Pick a color'}
        </Button>
      </div>
      {p ? (
        <div className="slider-list">
          <Slider label="Hue shift" min={-100} max={100} value={p.hueShift ?? 0} display={fmt('x', p.hueShift ?? 0)} track={`linear-gradient(90deg, hsl(${p.hue - 30} 65% 55%), hsl(${p.hue} 65% 55%) 50%, hsl(${p.hue + 30} 65% 55%))`} onChange={(v) => put({ ...p, hueShift: v }, 'Point color hue', `pc.${i}.hue`)} />
          <Slider label="Sat shift" min={-100} max={100} value={p.satShift ?? 0} display={fmt('x', p.satShift ?? 0)} onChange={(v) => put({ ...p, satShift: v }, 'Point color saturation', `pc.${i}.sat`)} />
          <Slider label="Lum shift" min={-100} max={100} value={p.lumShift ?? 0} display={fmt('x', p.lumShift ?? 0)} onChange={(v) => put({ ...p, lumShift: v }, 'Point color luminance', `pc.${i}.lum`)} />
          <Slider label="Range" min={0} max={100} def={50} value={p.range ?? 50} display={`${Math.round(p.range ?? 50)}`} onChange={(v) => put({ ...p, range: v }, 'Point color range', `pc.${i}.range`)} />
          <div className="dev-sub">
            <span className="grow" />
            <Button size="sm" variant="ghost" onClick={() => put(null, 'Removed point color')}>
              <X size={12} /> Remove
            </Button>
          </div>
        </div>
      ) : (
        <div className="dev-note faint">Pick a colour in the photo, then shift just that colour.</div>
      )}
    </div>
  );
}

/** A new Point Color from a picked pixel (sRGB 0..255). */
export function pointFrom([r, g, b]: [number, number, number]): PointColor {
  const [R, G, B] = [r / 255, g / 255, b / 255];
  const max = Math.max(R, G, B);
  const min = Math.min(R, G, B);
  const l = (max + min) / 2;
  const d = max - min;
  const s = d ? d / (1 - Math.abs(2 * l - 1)) : 0;
  const h = !d ? 0 : max === R ? ((G - B) / d + 6) % 6 : max === G ? (B - R) / d + 2 : (R - G) / d + 4;
  return { hue: Math.round(h * 60), sat: Math.round(s * 100), lum: Math.round(l * 100) };
}

// ---------- Effects, Detail, Optics, Calibration ----------

export function Effects({ g, set }: { g: Adjust; set: SetGlobal }) {
  return (
    <Section id="effects" title="Effects" closed aside={<Note n={changed(g, [...EFFECTS, ...VIGNETTE, ...GRAIN])} />}>
      <Rows g={g} defs={EFFECTS} set={set} />
      <div className="dev-sub faint sub-head">Vignette</div>
      <Rows g={g} defs={VIGNETTE} set={set} dimUnless="vignette" />
      <div className="dev-sub faint sub-head">Grain</div>
      <Rows g={g} defs={GRAIN} set={set} dimUnless="grain" />
    </Section>
  );
}

interface DenoiseStatus {
  raw: boolean;
  ready: boolean;
  job: { status: 'queued' | 'running'; done: number; total: number; eta_s: number | null } | null;
}

/** AI Denoise's base for the open Photo: a bar with an ETA while the model runs over it. */
function DenoiseProgress({ photo, on }: { photo: Photo; on: boolean }) {
  const s = usePoll<DenoiseStatus>(LIVE && on ? 'denoise_status' : null, { photo_id: Number(photo.id) }, 1000);
  const ready = Boolean(s?.ready);
  const was = useRef(ready);
  useEffect(() => {
    if (ready && !was.current) baseChanged();
    was.current = ready;
  }, [ready]);
  if (!on || !s?.job) return null;
  const j = s.job;
  const pct = j.total ? Math.floor((j.done / j.total) * 100) : 0;
  return (
    <div style={{ margin: '2px 0 10px' }}>
      <ProgressBar label={j.status === 'running' ? `Denoising ${pct}%` : 'Denoise queued'} done={j.total ? j.done : undefined} total={j.total || undefined} eta={j.eta_s} count={false} testId="denoise-progress" />
    </div>
  );
}

export function Detail({ g, set, photo }: { g: Adjust; set: SetGlobal; photo: Photo }) {
  // Lightroom's Denoise works on RAWs only.
  const raw = !/\.jpe?g$/i.test(photo.file);
  return (
    <Section id="detail" title="Detail" closed aside={<Note n={changed(g, [...(raw ? DENOISE : []), ...SHARPEN, ...NOISE, ...COLOR_NOISE])} />}>
      {raw ? (
        <>
          <Rows g={g} defs={DENOISE} set={set} />
          <DenoiseProgress photo={photo} on={Boolean(g.denoise)} />
        </>
      ) : null}
      <Rows g={g} defs={SHARPEN} set={set} dimUnless="sharpen" />
      <div className="dev-sub faint sub-head">Noise reduction</div>
      <Rows g={g} defs={NOISE} set={set} dimUnless="noise" />
      <Rows g={g} defs={COLOR_NOISE} set={set} dimUnless="colorNoise" />
      <div className="dev-note faint" style={{ marginTop: 4 }}>
        Detail shows best zoomed to 100%.
      </div>
    </Section>
  );
}

const LENS_PROFILE: { value: NonNullable<Adjust['lensCorrections']>; label: string; title: string; undo: string }[] = [
  { value: 'camera', label: 'As camera', title: 'What the camera corrected in its own JPEG', undo: 'Lens corrections as the camera' },
  { value: 'on', label: 'On', title: 'Distortion, vignetting and chromatic aberration from the lens data in the RAW', undo: 'Lens corrections on' },
  { value: 'off', label: 'Off', title: 'No lens corrections', undo: 'Lens corrections off' },
];

export function Optics({ g, set }: { g: Adjust; set: SetGlobal }) {
  const ca = Boolean(g.removeCA);
  const profile = g.lensCorrections ?? 'camera';
  const setProfile = (v: (typeof LENS_PROFILE)[number]['value']) => {
    const next = { ...g };
    if (v === 'camera') delete next.lensCorrections;
    else next.lensCorrections = v;
    set(next, LENS_PROFILE.find((p) => p.value === v)!.undo, 'lensCorrections');
  };
  return (
    <Section id="optics" title="Optics" closed aside={<Note n={changed(g, [...DEFRINGE, ...LENS]) + (ca ? 1 : 0) + (profile !== 'camera' ? 1 : 0)} />}>
      <div className="dev-sub faint sub-head">Lens corrections</div>
      <Seg value={profile} onChange={setProfile} label="Lens corrections" className="lens-profile" items={LENS_PROFILE} />
      <div className="dev-note faint">From the lens data Sony cameras write into each RAW.</div>
      <label className="dev-check">
        <input type="checkbox" checked={ca} onChange={(e) => set(setIn(g, ['removeCA'], e.target.checked ? 1 : 0), e.target.checked ? 'Remove chromatic aberration' : 'Keep chromatic aberration')} />
        Remove chromatic aberration
      </label>
      <div className="dev-sub faint sub-head">Defringe</div>
      <Rows g={g} defs={DEFRINGE} set={set} />
      <div className="dev-sub faint sub-head">Manual lens</div>
      <Rows g={g} defs={LENS} set={set} />
    </Section>
  );
}

const PRIMARY: { k: 'red' | 'green' | 'blue'; h: number }[] = [
  { k: 'red', h: 0 },
  { k: 'green', h: 120 },
  { k: 'blue', h: 240 },
];

export function CalibrationSection({ g, set }: { g: Adjust; set: SetGlobal }) {
  const c: Calibration = g.calibration ?? {};
  const one = (path: string[], label: string, v: number, track?: string) => (
    <Slider key={path.join('.')} label={label} name={path.length > 1 ? `${cap(path[0])} ${label.toLowerCase()}` : undefined} min={-100} max={100} value={v} track={track} display={fmt('x', v)} onChange={(x) => set(setIn(g, ['calibration', ...path], x), `Calibration ${label.toLowerCase()} ${fmt('x', x)}`, `calibration.${path.join('.')}`)} />
  );
  const n = (c.shadowsTint ? 1 : 0) + PRIMARY.filter((p) => c[p.k]?.hue || c[p.k]?.sat).length;
  return (
    <Section id="calibration" title="Calibration" closed aside={<Note n={n} />}>
      <div className="slider-list">{one(['shadowsTint'], 'Shadows tint', c.shadowsTint ?? 0, 'linear-gradient(90deg, #4fbf5a, #b9b9b9 50%, #c652c6)')}</div>
      {PRIMARY.map(({ k, h }) => (
        <div key={k}>
          <div className="dev-sub faint sub-head">{cap(k)} primary</div>
          <div className="slider-list">
            {one([k, 'hue'], 'Hue', c[k]?.hue ?? 0, `linear-gradient(90deg, hsl(${h - 40} 65% 50%), hsl(${h} 65% 50%) 50%, hsl(${h + 40} 65% 50%))`)}
            {one([k, 'sat'], 'Saturation', c[k]?.sat ?? 0, `linear-gradient(90deg, hsl(${h} 0% 50%), hsl(${h} 75% 50%))`)}
          </div>
        </div>
      ))}
    </Section>
  );
}


/** Where the person clicked on the photo (0..1 of the frame) and the colour there, if readable. */
export type Picked = { x: number; y: number; rgb: [number, number, number] | null };

/** Over the photo while picking a colour (white balance or Point Color): click to pick, Esc to stop. */
export function PickOverlay({ label, onPick, onCancel }: { label: string; onPick: (p: Picked) => void; onCancel: () => void }) {
  const click = (e: React.MouseEvent<HTMLDivElement>) => {
    e.stopPropagation();
    const b = e.currentTarget.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (e.clientX - b.left) / b.width));
    const y = Math.min(1, Math.max(0, (e.clientY - b.top) / b.height));
    onPick({ x, y, rgb: sample(e.currentTarget.parentElement?.querySelector('img') ?? null, x, y) });
  };
  return (
    <div className="pick-overlay" onClick={click} onPointerDown={(e) => e.stopPropagation()} onPointerUp={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === 'Escape' && onCancel()} tabIndex={-1} ref={(el) => el?.focus()} data-testid="pick-overlay">
      <span className="compare-flag">{label} · Esc to stop</span>
    </div>
  );
}

/** The mean colour of a small patch of an image, or null when the browser won't let us read it. */
function sample(img: HTMLImageElement | null, x: number, y: number): [number, number, number] | null {
  if (!img?.naturalWidth) return null;
  try {
    const c = document.createElement('canvas');
    const w = (c.width = Math.min(512, img.naturalWidth));
    const h = (c.height = Math.round((w * img.naturalHeight) / img.naturalWidth));
    const ctx = c.getContext('2d')!;
    ctx.filter = getComputedStyle(img).filter;
    ctx.drawImage(img, 0, 0, w, h);
    const px = ctx.getImageData(Math.max(0, Math.round(x * w) - 2), Math.max(0, Math.round(y * h) - 2), 5, 5).data;
    const sum = [0, 0, 0];
    for (let i = 0; i < px.length; i += 4) for (let k = 0; k < 3; k++) sum[k] += px[i + k];
    const n = px.length / 4;
    return [sum[0] / n, sum[1] / n, sum[2] / n];
  } catch {
    return null;
  }
}
