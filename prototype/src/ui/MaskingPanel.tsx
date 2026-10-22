// The Masking tool (design pass v2), after Lightroom's: Create new mask (AI selections, then
// Brush, Linear, Radial, Hair and the Color and Luminance ranges), the Photo's Masks, Select
// People for a person Mask, and the selected Mask's Amount and sliders. A Mask grows with
// Add, Subtract and Intersect, listed under it as its components; a selected component's
// range or handles are what the panel and the photo edit. Linear and Radial are placed
// with handles on the photo (MaskHandles), a Color range by picking colours on it.

import { useEffect, useState } from 'react';
import { Brush, Circle, Cloud, Eye, Mountain, PersonStanding, Pipette, Rows3, SquaresIntersect, SquaresSubtract, SquaresUnite, SunMedium, Users, Wind, X } from 'lucide-react';
import type { LocalAdjust, MaskComponent, MaskOp, MaskRef, NumKey, PersonPart, Photo, Recipe } from '../domain/types';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';
import type { Picked } from './EditSections';
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

type Kind = 'subject' | 'sky' | 'background' | 'people' | 'linear' | 'radial' | 'hair' | 'luminance' | 'color';
const CREATE: { kind: Kind | 'brush'; label: string; icon: typeof Users; title?: string }[] = [
  { kind: 'subject', label: 'Subject', icon: PersonStanding },
  { kind: 'sky', label: 'Sky', icon: Cloud },
  { kind: 'background', label: 'Background', icon: Mountain },
  { kind: 'people', label: 'People', icon: Users },
  { kind: 'hair', label: 'Hair', icon: Wind },
  { kind: 'brush', label: 'Brush', icon: Brush },
  { kind: 'linear', label: 'Linear', icon: Rows3 },
  { kind: 'radial', label: 'Radial', icon: Circle },
  { kind: 'color', label: 'Color', icon: Pipette, title: 'Color range: pick a colour on the photo' },
  { kind: 'luminance', label: 'Luminance', icon: SunMedium, title: 'Luminance range: a band of tones' },
];

/** What Add, Subtract and Intersect offer: the kinds that need no setup of their own. */
type PartKind = 'subject' | 'sky' | 'background' | 'linear' | 'radial' | 'color' | 'luminance';
const PART_KINDS: PartKind[] = ['subject', 'sky', 'background', 'linear', 'radial', 'color', 'luminance'];
const OPS: { op: MaskOp; label: string; icon: typeof Users }[] = [
  { op: 'add', label: 'Add', icon: SquaresUnite },
  { op: 'subtract', label: 'Subtract', icon: SquaresSubtract },
  { op: 'intersect', label: 'Intersect', icon: SquaresIntersect },
];
const KIND_LABEL: Record<string, string> = { subject: 'Subject', sky: 'Sky', background: 'Background', linear: 'Linear', radial: 'Radial', color: 'Color range', luminance: 'Luminance range', hair: 'Hair', faces: 'Faces', skin: 'Skin', person: 'People', brush: 'Brush', gradient: 'Gradient' };

/** A fresh Mask of `kind` with Lightroom-like defaults; a Color range starts at `at`. */
function shapeOf(kind: PartKind, at?: { x: number; y: number }): MaskRef {
  switch (kind) {
    case 'linear':
      return { kind, from: { x: 0.5, y: 0 }, to: { x: 0.5, y: 0.45 } };
    case 'radial':
      return { kind, x: 0.5, y: 0.45, rx: 0.22, ry: 0.3, feather: 0.5 };
    case 'luminance':
      return { kind, lo: 0.6, hi: 1, smooth: 0.25 };
    case 'color':
      return { kind, points: at ? [at] : [], range: 0.5 };
    default:
      return { kind };
  }
}

/** The Mask itself (part null) or one of its components. */
export function maskPart(m: MaskRef, part: number | null): MaskRef | undefined {
  return part === null ? m : m.components?.[part];
}

/** `m` with the Mask itself (part null) or one component replaced. */
export function withPart(m: MaskRef, part: number | null, next: MaskRef): MaskRef {
  if (part === null) return { ...next, components: m.components } as MaskRef;
  return { ...m, components: (m.components ?? []).map((c, i) => (i === part ? ({ ...next, op: c.op } as MaskComponent) : c)) };
}

/** "Faces · Person 1": the first part picked, and whose. */
function personName(m: Extract<MaskRef, { kind: 'person' }>): string {
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
const kindIcon = (m: MaskRef) => ({ subject: PersonStanding, sky: Cloud, background: Mountain, faces: Users, person: Users, skin: Users, hair: Wind, brush: Brush, linear: Rows3, gradient: Rows3, radial: Circle, luminance: SunMedium, color: Pipette })[m.kind];
const AI_KINDS = ['subject', 'sky', 'background', 'faces', 'person', 'skin', 'hair'];
const pct = (v: number) => `${Math.round(v * 100)}`;

interface Props {
  photo: Photo;
  recipe: Recipe;
  overlay: boolean;
  onOverlay: (v: boolean) => void;
  sel: string | null;
  onSel: (id: string | null) => void;
  brush: boolean;
  onBrush: (v: boolean) => void;
  /** The selected component of the selected Mask, null for the Mask itself. */
  part: number | null;
  onPart: (i: number | null) => void;
  /** Ask for a click on the photo (the colour picker). */
  onPick: (label: string, then: (p: Picked) => void) => void;
}

export function MaskingPanel({ photo: p, recipe, overlay, onOverlay, sel, onSel, brush, onBrush, part, onPart, onPick }: Props) {
  const { dispatch } = useLibrary();
  const selected = recipe.local.find((l) => l.id === sel) ?? null;
  const people = usePeople(p, selected?.mask.kind === 'person' || recipe.local.some((l) => l.mask.kind === 'person'));
  const upsert = (local: LocalAdjust, label: string, key?: string) => dispatch({ type: 'recipe/local-upsert', id: p.id, local, label, key });

  const add = (kind: Kind, at?: { x: number; y: number }) => {
    if (kind === 'color' && !at) return onPick('Click a colour to select', (c) => add('color', c));
    const made: Record<Kind, [MaskRef, LocalAdjust['adjust']]> = {
      subject: [{ kind: 'subject' }, { exposure: 0.15 }],
      sky: [{ kind: 'sky' }, { highlights: -15, exposure: -0.1 }],
      background: [{ kind: 'background' }, { exposure: -0.15 }],
      people: [{ kind: 'person', person: 0, parts: ['face_skin', 'eyes'] }, { exposure: 0.2, shadows: 10 }],
      linear: [{ kind: 'linear', from: { x: 0.5, y: 0 }, to: { x: 0.5, y: 0.45 } }, { exposure: -0.3, highlights: -20 }],
      radial: [{ kind: 'radial', x: 0.5, y: 0.45, rx: 0.22, ry: 0.3, feather: 0.5 }, { exposure: 0.25 }],
      hair: [{ kind: 'hair' }, { shadows: 10, saturation: 5 }],
      luminance: [shapeOf('luminance'), { highlights: -20 }],
      color: [shapeOf('color', at), { saturation: 10 }],
    };
    const [mask, adjust] = made[kind];
    const name = mask.kind === 'person' ? personName(mask) : KIND_LABEL[kind];
    const local: LocalAdjust = { id: `${kind}${Date.now()}${seq++}`, name, mask, adjust, source: 'you' };
    upsert(local, `Added ${name} Mask`);
    onSel(local.id);
    onPart(null);
    onOverlay(true);
  };
  // Lightroom's Add, Subtract and Intersect: a component joins the selected Mask.
  const combine = (l: LocalAdjust, op: MaskOp, kind: PartKind, at?: { x: number; y: number }) => {
    if (kind === 'color' && !at) return onPick('Click a colour to select', (c) => combine(l, op, kind, c));
    const components = [...(l.mask.components ?? []), { ...shapeOf(kind, at), op } as MaskComponent];
    upsert({ ...l, mask: { ...l.mask, components } }, `${l.name}: ${OPS.find((o) => o.op === op)!.label.toLowerCase()} ${KIND_LABEL[kind]}`);
    onPart(components.length - 1);
    onOverlay(true);
  };
  const dropPart = (l: LocalAdjust, i: number) => {
    const components = (l.mask.components ?? []).filter((_, k) => k !== i);
    upsert({ ...l, mask: { ...l.mask, components: components.length ? components : undefined } }, `${l.name}: removed a component`);
    onPart(null);
  };
  const setPart = (l: LocalAdjust, i: number | null, next: MaskRef, label: string, key?: string) => upsert({ ...l, mask: withPart(l.mask, i, next) }, label, key);
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
            <button key={c.kind} disabled={disabled} className={c.kind === 'brush' && brush ? 'on' : ''} onClick={() => (c.kind === 'brush' ? onBrush(!brush) : add(c.kind))} title={c.kind === 'brush' ? 'Paint a Mask (B)' : disabled ? 'No people in this Photo' : c.title} data-testid={`mask-new-${c.kind}`}>
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
          const comps = l.mask.components ?? [];
          const open = sel === l.id && comps.length > 0;
          return (
            <div key={l.id} className="mask-group">
            <div role="option" aria-selected={sel === l.id} className={`mask-row ${sel === l.id && (part === null || !open) ? 'sel' : ''}`} onClick={() => (onSel(l.id), onPart(null))} data-testid={`mask-row-${i}`}>
              <Icon size={13} style={{ color: MASK_COLORS[i % MASK_COLORS.length] }} />
              <span className="grow trunc">{l.name}</span>
              {comps.length > 0 && sel !== l.id && <span className="mono dev-note">+{comps.length}</span>}
              <span className="mono dev-note">{l.source === 'ai' || AI_KINDS.includes(l.mask.kind) ? 'AI' : ''}</span>
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
            {open && comps.map((c, k) => {
              const CIcon = kindIcon(c);
              const o = OPS.find((x) => x.op === c.op)!;
              return (
                <div key={k} role="option" aria-selected={part === k} className={`mask-row mask-part ${part === k ? 'sel' : ''}`} onClick={() => onPart(k)} data-testid={`mask-part-${k}`}>
                  <o.icon size={12} className="mask-op" aria-label={o.label} />
                  <CIcon size={12} className="faint" />
                  <span className="grow trunc">{KIND_LABEL[c.kind] ?? c.kind}{c.invert ? ' (inverted)' : ''}</span>
                  <button
                    className="mask-x"
                    aria-label={`Remove ${KIND_LABEL[c.kind] ?? c.kind} from ${l.name}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      dropPart(l, k);
                    }}
                  >
                    <X size={12} />
                  </button>
                </div>
              );
            })}
            </div>
          );
        })}
      </div>
      {selected && (
        <div className="mask-ops" data-testid="mask-ops">
          {OPS.map(({ op, label, icon: OpIcon }) => (
            <DropdownMenu key={op}>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" data-testid={`mask-${op}`}>
                  <OpIcon size={12} /> {label}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="min-w-[180px]">
                <div className="menu-label">{label} {op === 'intersect' ? 'with' : op === 'add' ? 'to' : 'from'} {selected.name}</div>
                {PART_KINDS.map((k) => {
                  const KIcon = kindIcon(shapeOf(k));
                  return (
                    <DropdownMenuItem key={k} onSelect={() => combine(selected, op, k)} data-testid={`mask-${op}-${k}`}>
                      <KIcon size={13} /> {KIND_LABEL[k]}
                    </DropdownMenuItem>
                  );
                })}
              </DropdownMenuContent>
            </DropdownMenu>
          ))}
        </div>
      )}
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
      {selected && (() => {
        const m = maskPart(selected.mask, part);
        if (!m || (m.kind !== 'luminance' && m.kind !== 'color')) return null;
        const label = part === null ? selected.name : KIND_LABEL[m.kind];
        if (m.kind === 'luminance') {
          const set = (next: Partial<typeof m>, what: string, key: string) => setPart(selected, part, { ...m, ...next }, `${label}: ${what}`, `${part}:${key}`);
          return (
            <Section id="mask-range" title="Tones to select">
              <div className="slider-list" data-testid="mask-luminance">
                <Slider label="From" min={0} max={100} def={60} value={Math.round(m.lo * 100)} display={pct(m.lo)} onChange={(v) => set({ lo: Math.min(v / 100, m.hi) }, 'range', 'lo')} />
                <Slider label="To" min={0} max={100} def={100} value={Math.round(m.hi * 100)} display={pct(m.hi)} onChange={(v) => set({ hi: Math.max(v / 100, m.lo) }, 'range', 'hi')} />
                <Slider label="Smoothness" min={0} max={100} def={25} value={Math.round((m.smooth ?? 0.25) * 100)} display={pct(m.smooth ?? 0.25)} onChange={(v) => set({ smooth: v / 100 }, 'smoothness', 'smooth')} />
              </div>
              <div className="lum-band" aria-hidden>
                <span style={{ left: `${m.lo * 100}%`, width: `${(m.hi - m.lo) * 100}%` }} />
              </div>
            </Section>
          );
        }
        const pickMore = () => onPick('Click another colour to add', (c) => setPart(selected, part, { ...m, points: [...m.points, { x: c.x, y: c.y }] }, `${label}: added a colour`));
        return (
          <Section id="mask-range" title="Colours to select">
            <div className="slider-list" data-testid="mask-color">
              <Slider label="Refine" min={0} max={100} def={50} value={Math.round((m.range ?? 0.5) * 100)} display={pct(m.range ?? 0.5)} onChange={(v) => setPart(selected, part, { ...m, range: v / 100 }, `${label}: refine`, `${part}:refine`)} />
            </div>
            <div className="mask-colors">
              <span className="dev-note">{m.points.length} {m.points.length === 1 ? 'colour' : 'colours'} picked</span>
              <span className="grow" />
              {m.points.length > 1 && (
                <Button variant="ghost" size="sm" onClick={() => setPart(selected, part, { ...m, points: m.points.slice(0, -1) }, `${label}: removed a colour`)}>
                  Undo last
                </Button>
              )}
              <Button variant="ghost" size="sm" disabled={m.points.length >= 5} onClick={pickMore} title={m.points.length >= 5 ? 'Up to 5 colours, as Lightroom' : undefined} data-testid="mask-color-add">
                <Pipette size={12} /> Add a colour
              </Button>
            </div>
          </Section>
        );
      })()}
      {selected && (
        <Section id="mask-sliders" title={selected.name}>
          <div className="slider-list">
            <Slider
              label="Amount"
              name="Mask amount"
              min={0}
              max={200}
              def={100}
              value={selected.amount ?? 100}
              display={`${Math.round(selected.amount ?? 100)}`}
              onChange={(v) => upsert({ ...selected, amount: v === 100 ? undefined : v }, `${selected.name}: amount`, 'amount')}
            />
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
          {(() => {
            const m = maskPart(selected.mask, part) ?? selected.mask;
            const whose = part === null ? selected.name : KIND_LABEL[m.kind];
            return (
              <>
                <label className="part" style={{ marginTop: 8 }}>
                  <Checkbox checked={Boolean(m.invert)} onCheckedChange={(v) => setPart(selected, part, { ...m, invert: Boolean(v) || undefined } as MaskRef, `${whose}: ${v ? 'inverted' : 'not inverted'}`)} data-testid="mask-invert" />
                  Invert{part === null ? '' : ` ${whose}`}
                </label>
                {(m.kind === 'linear' || m.kind === 'radial') && <div className="dev-note" style={{ marginTop: 6 }}>Drag the handles on the photo to place it.</div>}
              </>
            );
          })()}
        </Section>
      )}
    </div>
  );
}
