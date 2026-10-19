// One Look for the whole Shoot (Greg: "easy to apply a consistent look across the shoot").
// Lightroom's Before / After of the Pick in focus (side by side, top and bottom, or a split
// you drag), a strip of the Picks, and the Agent beside it with the Look as a card.

import { useEffect, useMemo, useRef, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Columns2, MoreHorizontal, RefreshCw, Rows2, SquareSplitHorizontal, Undo2 } from 'lucide-react';
import { PHOTOS, PHOTO_BY_ID, SHOOT } from '../data/dummy';
import { LIVE, asPerson, listLooks, readRecipes } from '../data/live';
import type { Adjust, NumKey, Photo } from '../domain/types';
import { fmt, recipeOf, useLibrary, verdictOf } from '../store/library';
import { go } from '../router';
import { Agent } from '../agent/AgentPanel';
import { PhotoView } from '../ui/PhotoView';
import { Kbd, Slider, Tip } from '../ui/kit';
import { BackToGrid, Hints, QueueMeter } from '../ui/ViewBar';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';

const LOOK_SLIDERS: { key: NumKey; label: string; min: number; max: number; step?: number }[] = [
  { key: 'exposure', label: 'Exposure', min: -2, max: 2, step: 0.05 },
  { key: 'temp', label: 'Temperature', min: -100, max: 100 },
  { key: 'tint', label: 'Tint', min: -100, max: 100 },
  { key: 'contrast', label: 'Contrast', min: -100, max: 100 },
  { key: 'saturation', label: 'Saturation', min: -100, max: 100 },
  { key: 'shadows', label: 'Shadows', min: -100, max: 100 },
  // The engine doesn't render Fade or Grain yet, so the app leaves them out.
  ...(LIVE ? [] : ([{ key: 'fade', label: 'Fade', min: 0, max: 100 }, { key: 'grain', label: 'Grain', min: 0, max: 100 }] as const)),
];

const SHORT: Partial<Record<keyof Adjust, string>> = { temp: 'Temp', saturation: 'Sat', exposure: 'Exp' };

/** "Temp +6 · Vibrance +12": the Look's non-zero sliders, as the card reads them. */
function summary(a: Adjust): string[] {
  return (Object.entries(a) as [keyof Adjust, number][]).filter(([, v]) => v).map(([k, v]) => `${SHORT[k] ?? k.charAt(0).toUpperCase() + k.slice(1)} ${fmt(k, v)}`);
}

type Layout = 'lr' | 'tb' | 'split';
const LAYOUTS: { id: Layout; label: string; icon: typeof Columns2 }[] = [
  { id: 'lr', label: 'Left / right', icon: Columns2 },
  { id: 'tb', label: 'Top / bottom', icon: Rows2 },
  { id: 'split', label: 'Split', icon: SquareSplitHorizontal },
];

/** Lightroom's Before / After for the Pick in focus. */
function BeforeAfter({ p, layout, solo }: { p: Photo; layout: Layout; solo: boolean }) {
  const { state } = useLibrary();
  const recipe = recipeOf(state, p.id);
  const [at, setAt] = useState(0.5);
  const box = useRef<HTMLDivElement>(null);
  const drag = (e: React.PointerEvent) => {
    const r = box.current?.getBoundingClientRect();
    if (r) setAt(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)));
  };
  const after = (
    <PhotoView photo={p} size="full" recipe={recipe} look={state.look}>
      <span className="look-label">After</span>
    </PhotoView>
  );
  if (solo)
    return (
      <div className="look-pair" data-testid="look-pair">
        <div className="stage-fit look-after">{after}</div>
      </div>
    );
  if (layout === 'split')
    return (
      <div className="look-split" data-testid="look-pair">
        <div
          className="split-inner"
          ref={box}
          onPointerDown={(e) => {
            (e.target as HTMLElement).setPointerCapture(e.pointerId);
            drag(e);
          }}
          onPointerMove={(e) => e.buttons === 1 && drag(e)}
        >
          <PhotoView photo={p} size="full" original>
            <span className="look-label">Before</span>
          </PhotoView>
          <div className="split-after look-after" style={{ clipPath: `inset(0 0 0 ${at * 100}%)` }}>
            <PhotoView photo={p} size="full" recipe={recipe} look={state.look} style={{ width: '100%' }}>
              <span className="look-label right">After</span>
            </PhotoView>
          </div>
          <span className="split-line" style={{ left: `${at * 100}%` }} />
        </div>
      </div>
    );
  return (
    <div className={`look-pair ${layout === 'tb' ? 'top-bottom' : ''}`} data-testid="look-pair">
      <div className="stage-fit">
        <PhotoView photo={p} size="full" original>
          <span className="look-label">Before</span>
        </PhotoView>
      </div>
      <div className="stage-fit look-after">{after}</div>
    </div>
  );
}

export function LookScreen({ onExport }: { onExport: () => void }) {
  const { state, dispatch } = useLibrary();
  const [selId, setSelId] = useState<string | null>(null);
  const [layout, setLayout] = useState<Layout>('lr');
  const [solo, setSolo] = useState(false);
  const [adjusting, setAdjusting] = useState(false);
  const [loaded, setLoaded] = useState(0);
  const picks = useMemo(() => PHOTOS.slice(0, state.imported).filter((p) => verdictOf(state, p.id) === 'pick'), [state]);
  const pickIds = picks.map((p) => p.id);
  const sel = (selId && PHOTO_BY_ID[selId]) || picks[0];
  const at = sel ? pickIds.indexOf(sel.id) : -1;
  // Live: the Picks' Recipes and the Shoot's Look come from the Library.
  const pickKey = pickIds.join(',');
  useEffect(() => {
    if (!LIVE) return;
    let on = true;
    Promise.all([readRecipes(pickKey ? pickKey.split(',') : []), listLooks()])
      .then(([recipes, looks]) => {
        if (!on) return;
        for (const [id, recipe] of Object.entries(recipes)) dispatch({ type: 'recipe/load', id, recipe });
        dispatch({ type: 'look/load', looks });
      })
      .catch((e) => console.error('loading the Look failed', e));
    return () => {
      on = false;
    };
  }, [pickKey, dispatch, loaded]);

  const move = (d: number) => {
    const n = pickIds[at + d];
    if (n) setSelId(n);
  };
  const selRecipe = sel ? recipeOf(state, sel.id) : null;
  const canSync = !!selRecipe && Object.values(selRecipe.global).some((v) => v);
  // Lightroom's Sync: fold the Pick in focus's own sliders into the Look every Pick shares.
  const sync = () => {
    if (!sel || !canSync) return;
    if (!LIVE) return dispatch({ type: 'look/sync', id: sel.id });
    const done = asPerson('sync_to_look', { photo_id: Number(sel.id) }, 'Sync to the Look', (r) => [Number(sel.id), ...((r.photos_under_look as number[]) ?? [])]);
    dispatch({ type: 'engine/saved', label: 'Sync to the Look', batch: done });
    done
      .then(() => setLoaded((n) => n + 1))
      .catch((e) => dispatch({ type: 'toast', text: `Sync failed: ${String(e).replace(/^Error: /, '')}` }));
  };
  const cycle = () => setLayout((l) => LAYOUTS[(LAYOUTS.findIndex((x) => x.id === l) + 1) % LAYOUTS.length].id);

  useHotkeys('right', () => move(1), [at, pickKey]);
  useHotkeys('left', () => move(-1), [at, pickKey]);
  useHotkeys('backslash', () => setSolo((s) => !s));
  useHotkeys('y', cycle);
  useHotkeys('shift+s', sync, [sel, canSync]);
  useHotkeys('e', () => sel && go({ name: 'edit', id: sel.id }), [sel]);
  useHotkeys(['escape', 'g'], () => go({ name: 'cull' }));

  const look = state.look;
  const card = (
    <div className="look-card" data-testid="look-card">
      <div className="row" style={{ gap: 8 }}>
        <h3 className="name" style={{ margin: 0 }} aria-label={look ? `Look: ${look.name}` : 'No Look yet'}>
          {look ? look.name : 'No Look yet'}
        </h3>
        <span className="grow" />
        {look && (
          <Button variant="ghost" size="sm" onClick={() => setAdjusting((a) => !a)} aria-expanded={adjusting}>
            {adjusting ? 'Done' : 'Adjust'}
          </Button>
        )}
      </div>
      {look ? (
        summary(look.adjust).length ? (
          <div className="sum">{summary(look.adjust).join(' · ')}</div>
        ) : (
          <div className="sum empty">Every slider at zero.</div>
        )
      ) : (
        <div className="sum empty">Describe a Look below and the Agent applies it to every Pick, with Corrections per photo so the set matches. Or tune one Pick in Edit and press Sync to Look.</div>
      )}
      {look && adjusting && (
        <div className="slider-list" style={{ marginTop: 4 }}>
          {LOOK_SLIDERS.map((s) => (
            <Slider key={s.key} label={s.label} min={s.min} max={s.max} step={s.step} value={look.adjust[s.key] ?? 0} display={fmt(s.key, look.adjust[s.key] ?? 0)} onChange={(v) => dispatch({ type: 'look/adjust', key: s.key, value: v })} />
          ))}
        </div>
      )}
      {look && adjusting && sel && selRecipe && (
        <>
          <div className="row" style={{ marginTop: 6 }}>
            <span style={{ fontWeight: 500, color: 'var(--text)' }}>Corrections</span>
            <span className="grow" />
            <span className="faint mono" style={{ fontSize: 11 }}>{sel.file}</span>
          </div>
          <div className="slider-list">
            <Slider label="Exposure" min={-2} max={2} step={0.05} value={selRecipe.correction.exposure} display={fmt('exposure', selRecipe.correction.exposure)} ai onChange={(v) => dispatch({ type: 'correction/set', id: sel.id, patch: { exposure: v }, by: 'you' })} />
            <Slider label="White bal." min={-1000} max={1000} step={50} value={selRecipe.correction.wbK} display={`${selRecipe.correction.wbK}K`} ai onChange={(v) => dispatch({ type: 'correction/set', id: sel.id, patch: { wbK: v }, by: 'you' })} />
            <Slider label="Face light" min={0} max={1.5} step={0.05} value={selRecipe.correction.face} display={fmt('face', selRecipe.correction.face)} ai onChange={(v) => dispatch({ type: 'correction/set', id: sel.id, patch: { face: v }, by: 'you' })} />
          </div>
        </>
      )}
    </div>
  );
  const after = (
    <div className="look-sync">
      {card}
      <div className="look-actions">
        <Tip label={canSync ? `Fold ${sel?.file}'s own sliders into the Look, for every Pick` : 'Edit a Pick first, then sync its sliders into the Look'} keys="⇧S">
          <Button size="sm" onClick={sync} disabled={!canSync}>
            <RefreshCw size={12} /> Sync to Look
          </Button>
        </Tip>
        <Button size="sm" variant="ghost" onClick={() => dispatch({ type: 'undo' })} disabled={!state.past?.length}>
          <Undo2 size={12} /> Undo
        </Button>
        <span className="grow" />
        {look && (
          <Button size="sm" variant="ghost" onClick={() => dispatch({ type: 'look/clear' })}>
            Remove Look
          </Button>
        )}
      </div>
      {look && <span>Each Photo keeps its own exposure and white balance fixes underneath the Look.</span>}
    </div>
  );
  const title = (
    <>
      <span style={{ fontWeight: 500 }}>Look</span>
      <span className="faint mono" style={{ fontSize: 11 }}>
        {picks.length} Picks
      </span>
      <span className="grow" />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="sm" aria-label="More">
            <MoreHorizontal size={13} />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem disabled={!picks.length} onSelect={onExport}>
            Export {picks.length} Picks… <Kbd>⌘E</Kbd>
          </DropdownMenuItem>
          {sel && <DropdownMenuItem onSelect={() => go({ name: 'edit', id: sel.id })}>Edit {sel.file}</DropdownMenuItem>}
          {look && <DropdownMenuItem onSelect={() => dispatch({ type: 'look/clear' })}>Remove Look</DropdownMenuItem>}
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  );

  return (
    <>
      <main className="panel main view look-v2">
        <header className="panel-head view-head">
          <BackToGrid />
          <h1 className="view-title" style={{ margin: 0 }}>
            Look
          </h1>
          <span className="mono faint view-meta trunc">{[look?.name, SHOOT.name].filter(Boolean).join(' · ')}</span>
          <span className="grow" />
          <span className="layout-seg" role="group" aria-label="Before and after layout">
            {LAYOUTS.map((l) => (
              <button key={l.id} className={l.id === layout && !solo ? 'on' : ''} aria-pressed={l.id === layout && !solo} onClick={() => (setLayout(l.id), setSolo(false))} title={l.label}>
                <l.icon size={12} />
                <span className="wide">{l.label}</span>
              </button>
            ))}
          </span>
          <span className="hints">
            <Kbd>Y</Kbd> cycle
          </span>
          <QueueMeter />
        </header>
        {!picks.length || !sel ? (
          <div className="faint" style={{ padding: 24 }}>No Picks yet. Cull first, then come back to set a Look.</div>
        ) : (
          <>
            <div className="stage look-stage">
              <BeforeAfter p={sel} layout={layout} solo={solo} />
            </div>
            <div className="look-bar">
              <span className="mono" style={{ color: 'var(--text-2)' }}>{sel.file}</span>
              <span>
                Pick {at + 1} of {picks.length}
              </span>
              <span className="grow" />
              <Hints items={[['← →', 'Picks'], ['\\', solo ? 'Before / after' : 'After only'], ['Y', 'Layout'], ['⇧ S', 'Sync'], ['E', 'Edit']]} />
            </div>
            <div className="filmstrip" aria-label="Picks" data-testid="look-strip">
              {picks.map((p) => (
                <button key={p.id} className={p.id === sel.id ? 'on' : ''} onClick={() => setSelId(p.id)} onDoubleClick={() => go({ name: 'edit', id: p.id })} title={p.file} data-testid={`batch-${p.id}`}>
                  <PhotoView photo={p} recipe={recipeOf(state, p.id)} look={look} style={{ width: '100%', height: '100%', borderRadius: 0 }} />
                </button>
              ))}
            </div>
          </>
        )}
      </main>
      <Agent ctx="look" chips={[`${picks.length} Picks`, ...(look ? [look.name] : [])]} placeholder="Describe a change to the Look…" picks={pickIds} title={title} after={after} />
    </>
  );
}
