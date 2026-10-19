// The Masking tool (design pass v2), after Lightroom's: Create new mask (AI selections, then
// Brush, Linear, Radial and Hair), the Photo's Masks, Select People for a person Mask, and
// the selected Mask's sliders. Linear and Radial are placed with handles on the photo
// (MaskHandles).

import { useEffect, useState } from 'react';
import { Brush, Circle, Cloud, Eye, Mountain, PersonStanding, Rows3, Users, Wind, X } from 'lucide-react';
import type { LocalAdjust, MaskRef, NumKey, PersonPart, Photo, Recipe } from '../domain/types';
import { fmt, useLibrary } from '../store/library';
import { LIVE } from '../data/live';
import { findFaces, type FoundFace } from '../data/liveEdit';
import { MASK_COLORS } from './PhotoView';
import { Section } from './Develop';
import { Kbd, Slider } from './kit';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import type { Def } from './EditPanel';

const LOCAL: Def[] = [
  { key: 'exposure', label: 'Exposure', min: -2, max: 2, step: 0.05 },
  { key: 'contrast', label: 'Contrast', min: -100, max: 100 },
  { key: 'highlights', label: 'Highlights', min: -100, max: 100 },
  { key: 'shadows', label: 'Shadows', min: -100, max: 100 },
  { key: 'temp', label: 'Temp', min: -100, max: 100, track: 'linear-gradient(90deg, #4f7fd6, #b9b9b9 50%, #e0c043)' },
  { key: 'saturation', label: 'Saturation', min: -100, max: 100 },
];
const PEOPLE_EXTRA: Def[] = [
  { key: 'skin', label: 'Skin smooth', min: 0, max: 100 },
  { key: 'eyes', label: 'Eyes', min: 0, max: 100 },
  { key: 'teeth', label: 'Teeth', min: 0, max: 100 },
];

const PARTS: { part: PersonPart; label: string; short: string }[] = [
  { part: 'face_skin', label: 'Face skin', short: 'Faces' },
  { part: 'body_skin', label: 'Body skin', short: 'Skin' },
  { part: 'eyes', label: 'Eyes', short: 'Eyes' },
  { part: 'teeth', label: 'Teeth', short: 'Teeth' },
  { part: 'hair', label: 'Hair', short: 'Hair' },
  { part: 'whole', label: 'Whole person', short: 'Person' },
];

type Kind = 'subject' | 'sky' | 'background' | 'people' | 'linear' | 'radial' | 'hair';
const CREATE: { kind: Kind | 'brush'; label: string; icon: typeof Users }[] = [
  { kind: 'subject', label: 'Subject', icon: PersonStanding },
  { kind: 'sky', label: 'Sky', icon: Cloud },
  { kind: 'background', label: 'Background', icon: Mountain },
  { kind: 'people', label: 'People', icon: Users },
  { kind: 'brush', label: 'Brush', icon: Brush },
  { kind: 'linear', label: 'Linear', icon: Rows3 },
  { kind: 'radial', label: 'Radial', icon: Circle },
  { kind: 'hair', label: 'Hair', icon: Wind },
];

/** "Faces · Person 1": the first part picked, and whose. */
export function personName(m: Extract<MaskRef, { kind: 'person' }>): string {
  const first = PARTS.find((p) => m.parts.includes(p.part))?.short ?? 'People';
  return `${first} · ${m.person === undefined ? 'Everyone' : `Person ${m.person + 1}`}`;
}

/** Who is in the Photo, left to right, as boxes: the engine's faces live, the dummy's otherwise. */
function usePeople(p: Photo, enabled: boolean): FoundFace[] | null {
  const dummy = p.faces.map((f) => ({ x: f.x - f.r, y: f.y - f.r * 1.4, w: f.r * 2, h: f.r * 2.8 }));
  const [live, setLive] = useState<FoundFace[] | null>(null);
  useEffect(() => {
    if (!LIVE || !enabled) return;
    let on = true;
    setLive(null);
    findFaces(p.id)
      .then((f) => on && setLive(f))
      .catch((e) => {
        console.error('analyze_photo failed', e);
        if (on) setLive([]);
      });
    return () => {
      on = false;
    };
  }, [p.id, enabled]);
  return LIVE ? live : dummy;
}

let seq = 0;
const kindIcon = (m: MaskRef) => ({ subject: PersonStanding, sky: Cloud, background: Mountain, faces: Users, person: Users, skin: Users, hair: Wind, brush: Brush, linear: Rows3, gradient: Rows3, radial: Circle })[m.kind];

interface Props {
  photo: Photo;
  recipe: Recipe;
  overlay: boolean;
  onOverlay: (v: boolean) => void;
  sel: string | null;
  onSel: (id: string | null) => void;
  brush: boolean;
  onBrush: (v: boolean) => void;
}

export function MaskingPanel({ photo: p, recipe, overlay, onOverlay, sel, onSel, brush, onBrush }: Props) {
  const { dispatch } = useLibrary();
  const selected = recipe.local.find((l) => l.id === sel) ?? null;
  const people = usePeople(p, selected?.mask.kind === 'person' || recipe.local.some((l) => l.mask.kind === 'person'));
  const upsert = (local: LocalAdjust, label: string) => dispatch({ type: 'recipe/local-upsert', id: p.id, local, label });

  const add = (kind: Kind) => {
    const made: Record<Kind, [MaskRef, LocalAdjust['adjust']]> = {
      subject: [{ kind: 'subject' }, { exposure: 0.15 }],
      sky: [{ kind: 'sky' }, { highlights: -15, exposure: -0.1 }],
      background: [{ kind: 'background' }, { exposure: -0.15 }],
      people: [{ kind: 'person', person: 0, parts: ['face_skin', 'eyes'] }, { exposure: 0.2, shadows: 10 }],
      linear: [{ kind: 'linear', from: { x: 0.5, y: 0 }, to: { x: 0.5, y: 0.45 } }, { exposure: -0.3, highlights: -20 }],
      radial: [{ kind: 'radial', x: 0.5, y: 0.45, rx: 0.22, ry: 0.3, feather: 0.5 }, { exposure: 0.25 }],
      hair: [{ kind: 'hair' }, { shadows: 10, saturation: 5 }],
    };
    const [mask, adjust] = made[kind];
    const name = mask.kind === 'person' ? personName(mask) : CREATE.find((c) => c.kind === kind)!.label;
    const local: LocalAdjust = { id: `${kind}${Date.now()}${seq++}`, name, mask, adjust, source: 'you' };
    upsert(local, `Added ${name} Mask`);
    onSel(local.id);
    onOverlay(true);
  };
  const setMask = (l: LocalAdjust, mask: MaskRef) => upsert({ ...l, mask, name: mask.kind === 'person' && l.name === personName(l.mask as typeof mask) ? personName(mask) : l.name }, `${l.name}: changed Mask`);

  // People Masks lead with light and warmth, then retouching, as the design has them.
  const sliders = (l: LocalAdjust) => (l.mask.kind === 'person' || l.mask.kind === 'faces' ? [...LOCAL.filter((d) => ['exposure', 'shadows', 'temp'].includes(d.key)), ...PEOPLE_EXTRA, ...LOCAL.filter((d) => !['exposure', 'shadows', 'temp'].includes(d.key))] : LOCAL);

  return (
    <div className="dev-panel" data-testid="masking-panel">
      <div className="dev-title">
        <span>Masks</span>
        <span className="grow" />
        <Button variant={overlay ? 'default' : 'outline'} size="sm" onClick={() => onOverlay(!overlay)} aria-pressed={overlay}>
          <Eye size={12} /> Overlay <Kbd>O</Kbd>
        </Button>
      </div>
      <div className="dev-note">Create new mask</div>
      <div className="mask-tiles" data-testid="mask-create">
        {CREATE.map((c) => {
          const disabled = c.kind === 'people' && !LIVE && p.faces.length === 0;
          return (
            <button key={c.kind} disabled={disabled} className={c.kind === 'brush' && brush ? 'on' : ''} onClick={() => (c.kind === 'brush' ? onBrush(!brush) : add(c.kind))} title={c.kind === 'brush' ? 'Paint a Mask (B)' : disabled ? 'No people in this Photo' : undefined}>
              <c.icon size={15} strokeWidth={1.6} />
              <span>{c.kind === 'brush' && brush ? 'Painting…' : c.label}</span>
            </button>
          );
        })}
      </div>
      <div className="mask-rows" role="listbox" aria-label="Masks">
        {!recipe.local.length && <div className="faint" style={{ fontSize: 12, padding: '4px 0' }}>No Masks yet. Make one above, or ask the Agent.</div>}
        {recipe.local.map((l, i) => {
          const Icon = kindIcon(l.mask);
          return (
            <div key={l.id} role="option" aria-selected={sel === l.id} className={`mask-row ${sel === l.id ? 'sel' : ''}`} onClick={() => onSel(l.id)}>
              <Icon size={13} style={{ color: MASK_COLORS[i % MASK_COLORS.length] }} />
              <span className="grow trunc">{l.name}</span>
              <span className="mono dev-note">{l.source === 'ai' || ['subject', 'sky', 'background', 'faces', 'person', 'skin', 'hair'].includes(l.mask.kind) ? 'AI' : ''}</span>
              <button
                className="mask-x"
                aria-label={`Remove ${l.name}`}
                onClick={(e) => {
                  e.stopPropagation();
                  if (sel === l.id) onSel(null);
                  dispatch({ type: 'recipe/local-remove', id: p.id, localId: l.id });
                }}
              >
                <X size={12} />
              </button>
            </div>
          );
        })}
      </div>
      {selected?.mask.kind === 'person' && (() => {
        const m = selected.mask;
        return (
          <div className="people-pick" data-testid="people-picker">
            <div className="people">
              {people === null && <span className="dev-note">Finding people…</span>}
              {people?.map((f, i) => {
                const pad = 0.35;
                const box = { x: Math.max(0, f.x - f.w * pad), y: Math.max(0, f.y - f.h * pad), w: Math.min(1, f.w * (1 + 2 * pad)), h: Math.min(1, f.h * (1 + 2 * pad)) };
                const bg = LIVE && p.preview ? { backgroundImage: `url(${p.preview})`, backgroundSize: `${100 / box.w}% ${100 / box.h}%`, backgroundPosition: `${(box.x / Math.max(1e-3, 1 - box.w)) * 100}% ${(box.y / Math.max(1e-3, 1 - box.h)) * 100}%` } : {};
                return (
                  <button key={i} className={`person ${m.person === i ? 'on' : ''}`} onClick={() => setMask(selected, { ...m, person: i })} aria-pressed={m.person === i}>
                    <span className="face" style={bg}>{!bg.backgroundImage && i + 1}</span>
                    <span>Person {i + 1}</span>
                  </button>
                );
              })}
              {people && people.length > 1 && (
                <button className={`person ${m.person === undefined ? 'on' : ''}`} onClick={() => setMask(selected, { kind: 'person', parts: m.parts })} aria-pressed={m.person === undefined}>
                  <span className="face"><Users size={14} /></span>
                  <span>Everyone</span>
                </button>
              )}
              {people && <span className="dev-note people-note">{people.length === 0 ? 'No people found.' : `${people.length} ${people.length === 1 ? 'person' : 'people'} found. Pick what to select.`}</span>}
            </div>
            <div className="parts">
              {PARTS.map(({ part, label }) => (
                <label key={part} className="part">
                  <Checkbox
                    checked={m.parts.includes(part)}
                    onCheckedChange={(v) => {
                      const parts = v ? [...m.parts, part] : m.parts.filter((x) => x !== part);
                      if (parts.length) setMask(selected, { ...m, parts });
                    }}
                  />
                  {label}
                </label>
              ))}
            </div>
          </div>
        );
      })()}
      {selected && (
        <Section id="mask-sliders" title={selected.name}>
          <div className="slider-list">
            {sliders(selected).map((d) => (
              <Slider
                key={d.key}
                label={d.label}
                min={d.min}
                max={d.max}
                step={d.step}
                track={d.track}
                value={(selected.adjust[d.key as NumKey] as number | undefined) ?? 0}
                display={fmt(d.key, (selected.adjust[d.key as NumKey] as number | undefined) ?? 0)}
                ai={selected.source === 'ai'}
                onChange={(v) => dispatch({ type: 'recipe/local-adjust', id: p.id, localId: selected.id, key: d.key, value: v, by: 'you' })}
              />
            ))}
          </div>
          <label className="part" style={{ marginTop: 8 }}>
            <Checkbox checked={Boolean(selected.mask.invert)} onCheckedChange={(v) => upsert({ ...selected, mask: { ...selected.mask, invert: Boolean(v) || undefined } }, `${selected.name}: ${v ? 'inverted' : 'not inverted'}`)} />
            Invert
          </label>
          {(selected.mask.kind === 'linear' || selected.mask.kind === 'radial') && <div className="dev-note" style={{ marginTop: 6 }}>Drag the handles on the photo to place it.</div>}
        </Section>
      )}
    </div>
  );
}
