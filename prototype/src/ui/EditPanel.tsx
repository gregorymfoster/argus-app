// The Edit tool (design pass v2): Auto and B&W, Profile, the AI takes with their Amount
// (Recommended Looks moved to Presets, Greg 2026-10-22), one-click fixes, then Lightroom's panels in its order: Light, Tone curve, Color,
// Color mixer and Point color, Color grading, Effects, Lens Blur, Retouch, Detail, Optics and
// Calibration (EditSections.tsx), the rarely used ones closed at the bottom.
// Every change is a Recipe change through the store, so Undo and the engine see it.

import { useState, type ReactNode } from 'react';
import { Check, ChevronDown, Cloud, Contrast, Pipette, RefreshCw, Smile, Sparkles, Sun, Wand2 } from 'lucide-react';
import type { Adjust, HslBand, HslShift, Look, Photo, Recipe } from '../domain/types';
import { HSL_BANDS } from '../domain/types';
import { FIXES, applyPreset, hasFix, removePreset, type Preset } from '../domain/presets';
import { fmt, useLibrary } from '../store/library';
import { Section } from './Develop';
import { Seg, Slider, Tip } from './kit';
import { CalibrationSection, ColorGrading, Detail, Effects, Optics, PointColors, ToneCurve, pointFrom, type Def, type Picked } from './EditSections';
import { LensBlurSection, type LensBlurUi } from './LensBlur';
import { call } from '../data/backend';
import { LIVE } from '../data/live';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';

export type { Def };

// Lightroom's Basic panel, in its order and names.
export const LIGHT: Def[] = [
  { key: 'exposure', label: 'Exposure', min: -5, max: 5, step: 0.05 },
  { key: 'contrast', label: 'Contrast', min: -100, max: 100 },
  { key: 'highlights', label: 'Highlights', min: -100, max: 100 },
  { key: 'shadows', label: 'Shadows', min: -100, max: 100 },
  { key: 'whites', label: 'Whites', min: -100, max: 100 },
  { key: 'blacks', label: 'Blacks', min: -100, max: 100 },
];
export const WB: Def[] = [
  { key: 'temp', label: 'Temp', min: -100, max: 100, track: 'linear-gradient(90deg, #4f7fd6, #b9b9b9 50%, #e0c043)' },
  { key: 'tint', label: 'Tint', min: -100, max: 100, track: 'linear-gradient(90deg, #4fbf5a, #b9b9b9 50%, #c652c6)' },
];
export const PRESENCE: Def[] = [
  { key: 'vibrance', label: 'Vibrance', min: -100, max: 100, track: 'linear-gradient(90deg, #8a8a8a, #c9a04a 50%, #e0543f)' },
  { key: 'saturation', label: 'Saturation', min: -100, max: 100, track: 'linear-gradient(90deg, #8a8a8a, #b06ed0 50%, #e0543f)' },
];
const RETOUCH: Def[] = [
  { key: 'skin', label: 'Skin smooth', min: 0, max: 100 },
  { key: 'eyes', label: 'Eyes', min: 0, max: 100 },
  { key: 'teeth', label: 'Teeth', min: 0, max: 100 },
];
export const GLOBAL: Def[] = [...LIGHT, ...WB, ...PRESENCE, ...RETOUCH];

/** Band hues for the Color mixer swatches and tracks (the engine's band centres). */
const BAND_HUE: Record<HslBand, number> = { red: 0, orange: 30, yellow: 60, green: 120, aqua: 180, blue: 225, purple: 270, magenta: 315 };
const hueTrack = (h: number) => `linear-gradient(90deg, hsl(${h - 30} 65% 55%), hsl(${h} 65% 55%) 50%, hsl(${h + 30} 65% 55%))`;
const satTrack = (h: number) => `linear-gradient(90deg, hsl(${h} 0% 55%), hsl(${h} 80% 55%))`;
const lumTrack = (h: number) => `linear-gradient(90deg, hsl(${h} 60% 15%), hsl(${h} 60% 50%) 50%, hsl(${h} 60% 85%))`;
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** Lightroom's white balance menu (engine/src/white_balance.rs); Kelvin presets need a RAW. */
const WB_PRESETS: { id: string; name: string; k?: number; tint?: number }[] = [
  { id: 'as_shot', name: 'As shot' },
  { id: 'auto', name: 'Auto' },
  { id: 'daylight', name: 'Daylight', k: 5500 },
  { id: 'cloudy', name: 'Cloudy', k: 6500 },
  { id: 'shade', name: 'Shade', k: 7500 },
  { id: 'tungsten', name: 'Tungsten', k: 2850 },
  { id: 'fluorescent', name: 'Fluorescent', k: 3800, tint: 15 },
  { id: 'flash', name: 'Flash', k: 5500 },
];
/** The white balance last picked per Photo, shown while Temp and Tint still match it. */
const wbPicked = new Map<string, { name: string; temp: number; tint: number }>();

interface Props {
  photo: Photo;
  recipe: Recipe;
  look: Look | null;
  versions: number;
  /** Light and Color as shown in the preview (the section eyes). */
  shown: { light: boolean; color: boolean };
  onShown: (s: { light: boolean; color: boolean }) => void;
  onHover: (r: Recipe | null) => void;
  onAuto: () => void;
  onSync: () => void;
  /** Ask for a click on the photo (white balance eyedropper, Point Color). */
  onPick: (label: string, then: (p: Picked) => void) => void;
  /** The AI takes, first in the panel (Suggestions.tsx TakesPanel). */
  takes?: ReactNode;
  /** Lens Blur's depth view and Refine brush, which live on the stage. */
  lensBlur: LensBlurUi;
}

export function EditPanel({ photo: p, recipe, look, versions, shown, onShown, onHover, onAuto, onSync, onPick, takes, lensBlur }: Props) {
  const { dispatch } = useLibrary();
  const [band, setBand] = useState<HslBand>(() => HSL_BANDS.find((b) => recipe.global.hsl?.[b]) ?? 'orange');
  const set = (r: Recipe, label: string, key?: string) => dispatch({ type: 'recipe/set', id: p.id, recipe: r, label, by: 'you', key });
  const setGlobal = (g: Adjust, label: string, key?: string) => set({ ...recipe, global: g }, label, key);
  const [mixTab, setMixTab] = useState<'mixer' | 'point'>(() => (recipe.global.pointColor?.length ? 'point' : 'mixer'));

  const bw = (recipe.global.saturation ?? 0) <= -100;
  const toggleBw = () => dispatch({ type: 'recipe/global', id: p.id, key: 'saturation', value: bw ? 0 : -100, by: 'you' });
  const edited = Object.values(recipe.global).some((v) => v && (typeof v === 'number' || Object.keys(v).length)) || recipe.local.length > 0;

  const toggleFix = (f: Preset) => (hasFix(recipe, f) ? set(removePreset(recipe, f), `Removed ${f.name}`) : set(applyPreset(recipe, f), f.name));

  const row = (d: Def) => (
    <Slider key={d.key} label={d.label} min={d.min} max={d.max} step={d.step} track={d.track} value={(recipe.global[d.key] as number | undefined) ?? 0} display={fmt(d.key, (recipe.global[d.key] as number | undefined) ?? 0)} onChange={(v) => dispatch({ type: 'recipe/global', id: p.id, key: d.key, value: v, by: 'you' })} />
  );
  const setWb = (temp: number, tint: number, name: string) => {
    const g: Adjust = { ...recipe.global, temp: Math.round(temp), tint: Math.round(tint) };
    if (!g.temp) delete g.temp;
    if (!g.tint) delete g.tint;
    wbPicked.set(p.id, { name, temp: g.temp ?? 0, tint: g.tint ?? 0 });
    set({ ...recipe, global: g }, `White balance: ${name}`);
  };
  const wbName = (() => {
    const t = recipe.global.temp ?? 0;
    const n = recipe.global.tint ?? 0;
    const w = wbPicked.get(p.id);
    if (w && w.temp === t && w.tint === n) return w.name;
    return t || n ? 'Custom' : 'As shot';
  })();
  const engineWb = async (args: Record<string, unknown>) => {
    const r = await call<{ photos: Record<string, { temp: number; tint: number }> }>('white_balance', { photo_ids: [Number(p.id)], dry_run: true, ...args });
    return r.photos[String(Number(p.id))];
  };
  const fail = (e: unknown) => dispatch({ type: 'toast', text: String(e).replace(/^Error: /, '') });
  const wbPreset = (w: (typeof WB_PRESETS)[number]) => {
    if (w.id === 'as_shot') return setWb(0, 0, w.name);
    if (LIVE) return engineWb({ preset: w.id }).then((r) => setWb(r.temp, r.tint, w.name), fail);
    // Dummy Photos: as if the camera balanced for daylight.
    if (w.k) return setWb(0.75 * (1e6 / 5500 - 1e6 / w.k), w.tint ?? 0, w.name);
    setWb(0, 0, w.name);
  };
  const eyedropper = () =>
    onPick('Click something white or grey', ({ x, y, rgb }) => {
      if (LIVE) return void engineWb({ at: { x, y } }).then((r) => setWb(r.temp, r.tint, 'Custom'), fail);
      if (!rgb || rgb.some((v) => v < 2)) return fail('Pick a grey or white area');
      const [r, gr, b] = rgb;
      setWb((recipe.global.temp ?? 0) + 100 * Math.log2(b / r), (recipe.global.tint ?? 0) + 200 * Math.log2(gr / Math.sqrt(r * b)), 'Custom');
    });
  const pickPoint = () =>
    onPick('Click a colour to adjust', ({ rgb }) => {
      if (!rgb) return fail("Couldn't read that colour");
      const list = [...(recipe.global.pointColor ?? []), pointFrom(rgb)];
      setGlobal({ ...recipe.global, pointColor: list }, 'Point color added');
    });
  const mix = recipe.global.hsl ?? {};
  const setMix = (k: 'hue' | 'sat' | 'lum', v: number) => {
    const cur: HslShift = { ...mix[band], [k]: v };
    if (!v) delete cur[k];
    const hsl = { ...mix, [band]: cur };
    if (!Object.keys(cur).length) delete hsl[band];
    const g: Adjust = { ...recipe.global, hsl };
    if (!Object.keys(hsl).length) delete g.hsl;
    set({ ...recipe, global: g }, `Color mixer: ${cap(band)}`, `hsl.${band}.${k}`);
  };
  const mixed = HSL_BANDS.filter((b) => mix[b] && Object.values(mix[b]!).some((v) => v));
  const retouched = RETOUCH.filter((d) => recipe.global[d.key]).map((d) => `${d.key} ${recipe.global[d.key]}`);
  const fixIcon = { sky: Cloud, face: Smile, skin: Sparkles, shadows: Sun };

  return (
    <div className="dev-panel" data-testid="edit-panel">
      <div className="dev-title">
        <span>Edit</span>
        <span className="grow" />
        <Tip label="Set light and colour from this Photo, like Lightroom's Auto" keys="⇧A">
          <Button size="sm" variant="ghost" onClick={onAuto} data-testid="auto">
            <Wand2 size={12} /> Auto
          </Button>
        </Tip>
        <Tip label="Black and white" keys="V">
          <Button size="sm" variant={bw ? 'default' : 'ghost'} onClick={toggleBw} aria-pressed={bw}>
            <Contrast size={12} /> B&amp;W
          </Button>
        </Tip>
      </div>
      <div className="dev-row">
        <span className="faint">Profile</span>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button className="dev-select" data-testid="profile">
              {bw ? 'Argus Monochrome' : 'Argus Color'} <ChevronDown size={11} />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem onSelect={() => bw && toggleBw()}>Argus Color</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => !bw && toggleBw()}>Argus Monochrome</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <span className="grow" />
        <span className="mono dev-note">{edited ? `edited${versions ? ` · v${versions}` : ''}` : 'as shot'}</span>
      </div>
      {takes}
      <div className="dev-block" data-testid="quick-fixes" onMouseLeave={() => onHover(null)}>
        <div className="dev-sub">
          <span style={{ fontWeight: 500 }}>Quick fixes</span>
          <span className="grow" />
          <span className="dev-note">stack on your edit</span>
        </div>
        <div className="fix-chips" data-testid="fixes">
          {FIXES.map((f) => {
            const Icon = fixIcon[f.icon];
            const on = hasFix(recipe, f);
            return (
              <button key={f.id} className={`fix ${on ? 'on' : ''}`} aria-pressed={on} onClick={() => toggleFix(f)} onMouseEnter={() => onHover(on ? removePreset(recipe, f) : applyPreset(recipe, f))} onMouseLeave={() => onHover(null)} data-testid={f.id}>
                <Icon size={12} /> {f.name}
                {on && <Check size={11} />}
              </button>
            );
          })}
        </div>
      </div>
      <Section id="light" title="Light" aside={look ? <span className="faint dev-note">on {look.name}</span> : null} shown={shown.light} onShown={(v) => onShown({ ...shown, light: v })}>
        <div className="slider-list">{LIGHT.map(row)}</div>
      </Section>
      <ToneCurve g={recipe.global} set={setGlobal} />
      <Section id="color" title="Color" shown={shown.color} onShown={(v) => onShown({ ...shown, color: v })}>
        <div className="dev-sub">
          <span className="faint">White balance</span>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button className="dev-select" style={{ marginLeft: 10 }} data-testid="wb-menu">
                {wbName} <ChevronDown size={11} />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              {WB_PRESETS.map((w) => (
                <DropdownMenuItem key={w.id} disabled={Boolean(w.k) && /\.jpe?g$/i.test(p.file)} onSelect={() => wbPreset(w)}>
                  {w.name}
                  {w.k ? <span className="grow mono faint" style={{ textAlign: 'right', paddingLeft: 16 }}>{w.k} K</span> : null}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <span className="grow" />
          <Tip label="White balance eyedropper: click something neutral">
            <Button size="icon" variant="ghost" className="size-6" onClick={eyedropper} aria-label="White balance eyedropper" data-testid="wb-eyedropper">
              <Pipette size={12} />
            </Button>
          </Tip>
        </div>
        <div className="slider-list">{WB.map(row)}</div>
        <div className="slider-list" style={{ marginTop: 6 }}>{PRESENCE.map(row)}</div>
      </Section>
      <Section id="mixer" title="Color mixer" closed aside={mixed.length || recipe.global.pointColor?.length ? <span className="mono dev-note">{[...mixed, ...(recipe.global.pointColor?.length ? [`${recipe.global.pointColor.length} point`] : [])].join(' · ')}</span> : null}>
        <Seg value={mixTab} onChange={setMixTab} label="Color mixer or Point color" className="curve-modes" items={[{ value: 'mixer', label: 'Mixer', title: 'Hue, saturation and luminance per colour band' }, { value: 'point', label: 'Point color', title: 'Shift one exact colour picked from the photo' }]} />
        {mixTab === 'point' ? <PointColors g={recipe.global} set={setGlobal} onPick={pickPoint} /> : <>
        <div className="bands" role="radiogroup" aria-label="Color band">
          {HSL_BANDS.map((b) => (
            <button key={b} role="radio" aria-checked={band === b} aria-label={cap(b)} title={cap(b)} className={`band ${band === b ? 'on' : ''} ${mixed.includes(b) ? 'set' : ''}`} style={{ background: `hsl(${BAND_HUE[b]} 70% 55%)` }} onClick={() => setBand(b)} />
          ))}
        </div>
        <div className="slider-list">
          {(['hue', 'sat', 'lum'] as const).map((k) => {
            const h = BAND_HUE[band];
            const v = mix[band]?.[k] ?? 0;
            return <Slider key={k} label={{ hue: 'Hue', sat: 'Saturation', lum: 'Luminance' }[k]} min={-100} max={100} value={v} display={fmt('x', v)} track={{ hue: hueTrack, sat: satTrack, lum: lumTrack }[k](h)} onChange={(x) => setMix(k, x)} />;
          })}
        </div>
        </>}
      </Section>
      <ColorGrading g={recipe.global} set={setGlobal} />
      <Effects g={recipe.global} set={setGlobal} />
      <LensBlurSection photo={p} recipe={recipe} set={set} onPick={onPick} ui={lensBlur} />
      <Section id="retouch" title="Retouch" closed aside={retouched.length ? <span className="mono dev-note">{retouched.join(' · ')}</span> : null}>
        <div className="slider-list">{RETOUCH.map(row)}</div>
      </Section>
      <Detail g={recipe.global} set={setGlobal} photo={p} />
      <Optics g={recipe.global} set={setGlobal} />
      <CalibrationSection g={recipe.global} set={setGlobal} />
      <div className="dev-foot">
        <Tip label={look ? `Fold these into ${look.name}, for every Pick` : 'Make these the Look for every Pick'} keys="⇧S">
          <Button size="sm" variant="outline" disabled={!edited} onClick={onSync}>
            <RefreshCw size={12} /> Sync to Look
          </Button>
        </Tip>
      </div>
    </div>
  );
}
