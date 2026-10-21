import { useEffect, useRef, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { ArrowLeft, Bandage, Copy, Crop as CropIcon, History, Layers, SlidersHorizontal, SquareDashedMousePointer } from 'lucide-react';
import { PHOTOS, PHOTO_BY_ID } from '../data/dummy';
import type { Crop, LocalAdjust, NumKey, Recipe, Spot, Version } from '../domain/types';
import { fmt, recipeOf, useLibrary } from '../store/library';
import { go } from '../router';
import { Agent } from '../agent/AgentPanel';
import { LIVE, asPerson, focusPhoto, wrote } from '../data/live';
import { loadRecipe, pickSpotSource, useDetailRender, useEngineMasks, useLiveRender } from '../data/liveEdit';
import { listVersions } from '../data/liveAgent';
import { PhotoView } from '../ui/PhotoView';
import { SuggestionStrip } from '../ui/Suggestions';
import { Filmstrip } from '../ui/Filmstrip';
import { acceptSuggestion, applyToRest, cancelTakes, dismissSuggestion, requestSuggestions, useQueue, useSuggestionImage, useSuggestions, useTakesProgress } from '../data/suggestions';
import { Kbd, Tip, ESC_OUTSIDE_DIALOGS } from '../ui/kit';
import { QueueChip } from '../ui/Queue';
import { ToolStrip } from '../ui/Develop';
import { autoAction } from '../data/auto';
import { PresetsPanel } from '../ui/Presets';
import { EditPanel, GLOBAL, LIGHT, PRESENCE, WB } from '../ui/EditPanel';
import { MaskingPanel } from '../ui/MaskingPanel';
import { MaskHandles } from '../ui/MaskHandles';
import { PickOverlay, type Picked } from '../ui/EditSections';
import { CropOverlay, CropPanel, FULL } from '../ui/CropPanel';
import { HEAL_BRUSH, HealOverlay, HealPanel, SPOT_R, newSpotId, spotsOf, type HealBrush } from '../ui/HealPanel';
import { Button } from '@/components/ui/button';
import { longEdge, useNeed, useZoomPan } from '../ui/zoomPan';
import { Navigator, ZoomReadout } from '../ui/Navigator';
import { useSettingsKeys } from '../ui/PasteSettings';

/** What a take changes against the current Recipe, for the panel: [label, was, now]. */
function takeDiff(cur: Recipe, take: Recipe): [string, string, string][] {
  const rows: [string, string, string][] = [];
  const keys = new Set([...Object.keys(cur.global), ...Object.keys(take.global)].filter((k) => k !== 'hsl') as NumKey[]);
  for (const k of keys) {
    const a = cur.global[k] as unknown;
    const b = take.global[k] as unknown;
    if (typeof (b ?? a) === 'object') {
      // Curve and grading: say they changed rather than list every point.
      if (JSON.stringify(a ?? {}) !== JSON.stringify(b ?? {})) rows.push([(k as string) === 'grading' ? 'Color grading' : (k as string) === 'curve' ? 'Tone curve' : String(k), a ? 'set' : 'none', b ? 'changed' : 'none']);
      continue;
    }
    const [x, y] = [(a as number | undefined) ?? 0, (b as number | undefined) ?? 0];
    if (Math.abs(x - y) > 1e-6) rows.push([GLOBAL.find((d) => d.key === k)?.label ?? String(k), fmt(k, x), fmt(k, y)]);
  }
  // The take's own Masks (sky held, subject lifted): one row each, with what they do.
  for (const l of take.local) {
    if (cur.local.some((c) => c.id === l.id && JSON.stringify(c.adjust) === JSON.stringify(l.adjust))) continue;
    const what = Object.entries(l.adjust)
      .filter(([, v]) => typeof v === 'number' && v !== 0)
      .map(([k, v]) => `${GLOBAL.find((d) => d.key === k)?.label ?? k} ${fmt(k as NumKey, v as number)}`)
      .join(', ');
    rows.push([l.name || `${l.mask.kind} Mask`, cur.local.some((c) => c.id === l.id) ? 'changed' : 'none', what || 'set']);
  }
  const ca = cur.correction?.exposure ?? 0;
  const cb = take.correction?.exposure ?? 0;
  if (Math.abs(ca - cb) > 1e-6) rows.push(['Correction', fmt('exposure', ca), fmt('exposure', cb)]);
  if (take.look && take.look !== cur.look) rows.push(['Look', cur.look ?? 'none', take.look]);
  const crop = (r: Recipe) => (r.crop && (r.crop.w < 1 || r.crop.h < 1) ? `${Math.round(r.crop.w * 100)}×${Math.round(r.crop.h * 100)}%` : 'none');
  const angle = (r: Recipe) => `${(r.crop?.angle ?? 0).toFixed(1)}°`;
  if (crop(cur) !== crop(take)) rows.push(['Crop', crop(cur), crop(take)]);
  if (angle(cur) !== angle(take)) rows.push(['Straighten', angle(cur), angle(take)]);
  return rows;
}

let seq = 0;

/**
 * The History list. Live: the Library's Versions (the Agent's included), reloaded when the
 * tab opens and whenever the Agent or an undo changes this Photo.
 */
function useVersions(photoId: string, tab: string, local: Version[]): Version[] {
  const [live, setLive] = useState<Version[]>([]);
  useEffect(() => {
    if (!LIVE) return;
    let on = true;
    const load = () =>
      listVersions(photoId)
        .then((vs) => on && setLive(vs.map((v) => ({ n: v.version, label: v.label ?? (v.author === 'ai' ? 'AI edit' : 'Your edit'), by: v.author === 'ai' ? 'ai' : 'you', at: Date.parse(v.at), recipe: v.recipe }))))
        .catch((e) => console.error('list_versions failed', e));
    void load();
    const onChange = (e: Event) => (e as CustomEvent).detail === photoId && void load();
    window.addEventListener('argus:versions', onChange);
    return () => {
      on = false;
      window.removeEventListener('argus:versions', onChange);
    };
  }, [photoId, tab]);
  return LIVE ? live : local;
}

export function EditScreen({ id }: { id: string }) {
  const { state, dispatch } = useLibrary();
  const p = PHOTO_BY_ID[id] ?? PHOTOS[0];
  const order = PHOTOS.slice(0, state.imported).map((x) => x.id);
  const pos = order.indexOf(p.id);
  const nextKey = order.slice(pos + 1, pos + 4).join(',');
  useEffect(() => focusPhoto(p.id, nextKey ? nextKey.split(',') : []), [p.id, nextKey]);
  // Lightroom's Develop moves on with ⌘← ⌘→; plain arrows belong to the sliders.
  useHotkeys('mod+right', () => order[pos + 1] && go({ name: 'edit', id: order[pos + 1] }), { preventDefault: true }, [pos]);
  useHotkeys('mod+left', () => pos > 0 && go({ name: 'edit', id: order[pos - 1] }), { preventDefault: true }, [pos]);
  const recipe = recipeOf(state, p.id);
  // Which tool the strip has open: Edit, Crop, Masking, Presets, History (design pass v2).
  const [tab, setTab] = useState('edit');
  const versions = useVersions(p.id, tab, state.versions[p.id] ?? []);
  const [masks, setMasks] = useState(false);
  const [before, setBefore] = useState(false);
  /** A click on the photo the Edit panel asked for (eyedropper, Point Color). */
  const [picking, setPicking] = useState<{ label: string; then: (p: Picked) => void } | null>(null);
  const [brush, setBrush] = useState(false);
  const [stroke, setStroke] = useState<{ x: number; y: number; r: number }[] | null>(null);
  const [selMask, setSelMask] = useState<string | null>(null);
  // Live: start from the Library's saved Recipe, then let the engine render every change.
  const [loaded, setLoaded] = useState(!LIVE);
  useEffect(() => {
    if (!LIVE) return;
    let on = true;
    loadRecipe(p.id)
      .then((r) => on && r && dispatch({ type: 'recipe/load', id: p.id, recipe: r }))
      .catch((e) => console.error('read_recipe failed', e))
      .finally(() => on && setLoaded(true));
    return () => {
      on = false;
    };
  }, [p.id, dispatch]);
  // Hovering a preset previews it on the photo, through the engine when live.
  const [hover, setHover] = useState<Recipe | null>(null);
  useEffect(() => setHover(null), [p.id, tab]);

  // Suggestions: previewing one shows it on the photo; accepting saves it as a Version.
  const takes = useSuggestions(p.id);
  const queue = useQueue();
  const coming = useTakesProgress(p.id);
  const [pick, setPick] = useState(-1);
  useEffect(() => setPick(-1), [p.id]);
  useEffect(() => {
    if (pick >= takes.list.length) setPick(takes.list.length - 1);
  }, [pick, takes.list.length]);
  const take = pick >= 0 ? takes.list[pick] ?? null : null;
  // The take's Amount, like a Lightroom preset's: 0 = your edit, 100 = the take, up to 150.
  const [amount, setAmount] = useState(100);
  useEffect(() => setAmount(100), [p.id, take?.id]);
  const takeUrl = useSuggestionImage(take, 1600, amount / 100);
  const cycle = (d: number) => {
    const n = takes.list.length + 1;
    if (n > 1) setPick((i) => ((((i + 1 + d) % n) + n) % n) - 1);
  };

  const back = () => go({ name: state.look ? 'look' : 'cull' });
  useHotkeys('backslash', (e) => setBefore(e.type === 'keydown'), { keydown: true, keyup: true });
  useHotkeys('o', () => setMasks((m) => !m));
  // Lightroom desktop's keys: E the Edit panel, M Masking, ⇧A Auto, V black and white.
  useHotkeys('e', () => setTab('edit'));
  useHotkeys('m', () => setTab((t) => (t === 'masking' ? 'edit' : 'masking')));
  useHotkeys('c', () => setTab((t) => (t === 'crop' ? 'edit' : 'crop')));
  useHotkeys('h', () => setTab((t) => (t === 'heal' ? 'edit' : 'heal')));
  useHotkeys('shift+p', () => setTab((t) => (t === 'presets' ? 'edit' : 'presets')));
  useHotkeys('b', () => setBrush((b) => !b));
  // Heal (H): the next Spot's settings, the selected Spot, and the newest Recipe for picks
  // that finish after it changed.
  const [healBrush, setHealBrush] = useState<HealBrush>(HEAL_BRUSH);
  const [selSpot, setSelSpot] = useState<string | null>(null);
  useEffect(() => setSelSpot(null), [p.id]);
  const latest = useRef(recipe);
  latest.current = recipe;
  const saveSpots = (spots: Spot[], label: string, key?: string) => dispatch({ type: 'recipe/set', id: p.id, recipe: { ...latest.current, spots }, label, by: 'you', key });
  /** Where a Spot should copy from: the engine's pick when live, else beside it. */
  // Sources each Spot has had, so / keeps finding new ones as Lightroom's does.
  const tried = useRef(new Map<string, { x: number; y: number }[]>());
  const pickFrom = async (x: number, y: number, r: number, without?: Spot) => {
    const base = { ...latest.current, spots: spotsOf(latest.current).filter((s) => s.id !== without?.id) };
    const avoid = without ? [...(tried.current.get(without.id) ?? []), without.from] : [];
    if (without) tried.current.set(without.id, avoid);
    if (LIVE) return pickSpotSource(p.id, base, x, y, r, avoid);
    const side = without ? -1 : 1;
    return { x: Math.min(1, Math.max(0, x + side * 3 * r)), y };
  };
  const addSpot = async (x: number, y: number) => {
    const { mode, r, feather, opacity } = healBrush;
    try {
      const from = await pickFrom(x, y, r);
      const spot: Spot = { id: newSpotId(latest.current), mode, x, y, r, from, feather, opacity, source: 'you' };
      saveSpots([...spotsOf(latest.current), spot], mode === 'heal' ? 'Healed a spot' : 'Cloned a spot');
      setSelSpot(spot.id);
    } catch (e) {
      dispatch({ type: 'toast', text: `Couldn't heal there: ${String(e).replace(/^Error: /, '')}` });
    }
  };
  const repick = async (id: string) => {
    const s = spotsOf(latest.current).find((x) => x.id === id);
    if (!s) return;
    const from = await pickFrom(s.x, s.y, s.r, s);
    saveSpots(spotsOf(latest.current).map((x) => (x.id === id ? { ...x, from } : x)), 'New source');
  };
  const sizeBy = (k: number) => {
    const r = Math.min(SPOT_R.max, Math.max(SPOT_R.min, Math.round(healBrush.r * k * 1e4) / 1e4));
    setHealBrush({ ...healBrush, r });
    if (selSpot) saveSpots(spotsOf(recipe).map((x) => (x.id === selSpot ? { ...x, r } : x)), 'Spot size', `spot:${selSpot}:r`);
  };
  // Lightroom's keys: [ and ] size the brush, / picks a new source, Delete removes the Spot.
  useHotkeys('bracketleft', () => tab === 'heal' && sizeBy(1 / 1.15), [tab, healBrush, selSpot, recipe]);
  useHotkeys('bracketright', () => tab === 'heal' && sizeBy(1.15), [tab, healBrush, selSpot, recipe]);
  useHotkeys('slash', () => tab === 'heal' && selSpot && void repick(selSpot), [tab, selSpot]);
  useHotkeys('delete', () => {
    if (tab !== 'heal' || !selSpot) return;
    saveSpots(spotsOf(recipe).filter((x) => x.id !== selSpot), 'Removed a Spot');
    setSelSpot(null);
  }, [tab, selSpot, recipe]);
  useHotkeys('escape', () => (brush ? setBrush(false) : tab === 'heal' && selSpot ? setSelSpot(null) : back()), ESC_OUTSIDE_DIALOGS, [brush, state.look, tab, selSpot]);
  // Lightroom's Sync: fold this photo's sliders into the Shoot's Look.
  const hasGlobal = Object.values(recipe.global).some((v) => v);
  const reload = async () => {
    const r = await loadRecipe(p.id);
    if (r) dispatch({ type: 'recipe/load', id: p.id, recipe: r });
    window.dispatchEvent(new CustomEvent('argus:versions', { detail: p.id }));
  };
  const sync = () => {
    if (!LIVE) return dispatch({ type: 'look/sync', id: p.id });
    const done = asPerson('sync_to_look', { photo_id: Number(p.id) }, 'Sync to the Look', (r) => [Number(p.id), ...((r.photos_under_look as number[]) ?? [])]);
    dispatch({ type: 'engine/saved', label: 'Sync to the Look', batch: done });
    done
      .then(() => dispatch({ type: 'toast', text: 'Synced to the Look' }))
      .then(reload)
      .catch((e) => dispatch({ type: 'toast', text: `Sync failed: ${String(e).replace(/^Error: /, '')}` }));
  };
  const restore = (v: Version) => {
    if (!LIVE) return dispatch({ type: 'version/restore', id: p.id, n: v.n });
    // Saved by the store as one engine batch, so ⌘Z undoes it.
    dispatch({ type: 'recipe/set', id: p.id, recipe: v.recipe, label: `Restored v${v.n}`, by: 'you' });
  };
  useHotkeys('shift+s', () => hasGlobal && sync(), [hasGlobal, p.id]);
  useSettingsKeys(p.id, []);
  const auto = () => {
    dispatch(autoAction([p.id], (id) => recipeOf(state, id)));
    dispatch({ type: 'toast', text: `Auto on ${p.file}` });
  };
  const bw = ((recipe.global.saturation as number | undefined) ?? 0) <= -100;
  const toggleBw = () => dispatch({ type: 'recipe/global', id: p.id, key: 'saturation', value: bw ? 0 : -100, by: 'you' });
  useHotkeys('shift+a', auto, [p.id, state]);
  useHotkeys('v', toggleBw, [p.id, bw]);

  const accept = () => {
    if (!take) return;
    const label = `Suggestion: ${take.title}`;
    if (!LIVE) {
      dispatch({ type: 'recipe/set', id: p.id, recipe: take.recipe, label, by: 'ai' });
      setPick(-1);
      return;
    }
    const done = acceptSuggestion(take, amount / 100);
    dispatch({ type: 'engine/saved', label, batch: wrote(done.then((r) => r.batch)) });
    done
      .then(() => dispatch({ type: 'toast', text: `Accepted ${take.title}` }))
      .then(reload)
      .then(() => setPick(-1))
      .catch((e) => dispatch({ type: 'toast', text: `Accept failed: ${String(e).replace(/^Error: /, '')}` }));
  };
  const dismiss = () => {
    if (!take) return;
    dismissSuggestion(take).catch((e) => dispatch({ type: 'toast', text: `Dismiss failed: ${String(e).replace(/^Error: /, '')}` }));
  };
  // The strip shows the takes coming, so no toast.
  const more = () =>
    requestSuggestions(p.id).catch((e) => dispatch({ type: 'toast', text: `Couldn't ask: ${String(e).replace(/^Error: /, '')}` }));
  const toRest = () =>
    applyToRest(p.id, recipe)
      .then((n) => dispatch({ type: 'toast', text: `Matching ${n} Photos to ${p.file} in the background. Nothing changes until you accept.` }))
      .catch((e) => dispatch({ type: 'toast', text: `Couldn't start: ${String(e).replace(/^Error: /, '')}` }));
  useHotkeys('tab', (e) => (e.preventDefault(), cycle(1)), [takes.list.length]);
  useHotkeys('shift+tab', (e) => (e.preventDefault(), cycle(-1)), [takes.list.length]);
  useHotkeys('enter', accept, [take, p.id, amount]);
  useHotkeys('backspace', dismiss, [take]);
  useHotkeys('r', () => void more(), [p.id]);
  useHotkeys('mod+shift+a', (e) => (e.preventDefault(), void toRest()), [p.id, recipe]);

  const setFromPreset = (r: Recipe, label: string, key?: string) => dispatch({ type: 'recipe/set', id: p.id, recipe: r, label, by: 'you', key });
  // While a handle is dragged (a Linear or Radial Mask, the crop box) the preview follows it;
  // the Recipe changes once, on release.
  const [placing, setPlacing] = useState<{ local?: LocalAdjust; crop?: Crop; spot?: Spot } | null>(null);
  const [shown, setShown] = useState({ light: true, color: true });
  let drawn: Recipe = stroke
    ? { ...recipe, local: [...recipe.local.filter((l) => l.id !== 'live-brush'), { id: 'live-brush', name: 'Brush', mask: { kind: 'brush', dots: stroke }, adjust: { exposure: 0.5 }, source: 'you' }] }
    : recipe;
  if (placing?.local) drawn = { ...drawn, local: drawn.local.map((l) => (l.id === placing.local!.id ? placing.local! : l)) };
  if (placing?.spot) drawn = { ...drawn, spots: spotsOf(drawn).map((s) => (s.id === placing.spot!.id ? placing.spot! : s)) };
  let live = hover ?? drawn;
  // The section eyes show the photo without Light or Color; Crop shows the whole frame.
  const off = [...(shown.light ? [] : LIGHT), ...(shown.color ? [] : [...WB, ...PRESENCE])];
  if (off.length) {
    const global = { ...live.global };
    for (const d of off) delete global[d.key];
    live = { ...live, global };
  }
  const crop = placing?.crop ?? recipe.crop ?? FULL;
  if (tab === 'crop') live = { ...live, crop: { ...FULL, angle: crop.angle } };
  // Spots sit on the whole upright frame, so Heal shows it uncropped.
  if (tab === 'heal') live = { ...live, crop: undefined };
  const render = useLiveRender(p.id, loaded ? live : null, LIVE);
  const engineMasks = useEngineMasks(p.id, recipe.local, LIVE && (masks || brush || tab === 'masking'));
  const selLocal = tab === 'masking' ? recipe.local.find((l) => l.id === selMask) : undefined;
  const placed = selLocal && placing?.local?.id === selLocal.id ? placing.local : selLocal;
  const [frame, setFrame] = useState(1.5);
  useEffect(() => {
    if (!p.preview) return;
    const img = new Image();
    img.onload = () => img.naturalHeight && setFrame(img.naturalWidth / img.naturalHeight);
    img.src = p.preview;
  }, [p.preview]);

  // Lightroom's zoom and pan; a click zooms unless it paints a Mask or moves the crop.
  const zp = useZoomPan({ photoKey: p.id, natural: longEdge(p), click: !brush && tab !== 'crop' && tab !== 'heal' });
  const need = useNeed(zp.z);
  const detail = useDetailRender(p.id, live, need, 1536, longEdge(p), LIVE && loaded && !take && !before);

  const at = (e: React.PointerEvent) => {
    const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: (e.clientX - b.left) / b.width, y: (e.clientY - b.top) / b.height, r: 0.06 };
  };
  const brushHandlers = brush
    ? {
        onPointerDown: (e: React.PointerEvent<HTMLDivElement>) => {
          if (zp.z.hand || e.button !== 0) return;
          (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
          setStroke([at(e)]);
        },
        onPointerMove: (e: React.PointerEvent<HTMLDivElement>) => {
          if (!stroke) return;
          const d = at(e);
          const last = stroke[stroke.length - 1];
          if (Math.hypot(d.x - last.x, d.y - last.y) > 0.02 && stroke.length < 80) setStroke([...stroke, d]);
        },
        onPointerUp: () => {
          if (!stroke) return;
          const local: LocalAdjust = { id: `brush${Date.now()}${seq++}`, name: `Brush ${recipe.local.filter((l) => l.mask.kind === 'brush').length + 1}`, mask: { kind: 'brush', dots: stroke }, adjust: { exposure: 0.5 }, source: 'you' };
          dispatch({ type: 'recipe/local-upsert', id: p.id, local, label: `Drew ${local.name}` });
          setStroke(null);
          setBrush(false);
          setSelMask(local.id);
          setMasks(true);
        },
      }
    : {};

  const diff = take && (
    <div className="take-diff" data-testid="take-diff">
      <div className="row">
        <div style={{ fontWeight: 500 }}>{take.title}</div>
        <span className="grow" />
        <span className="faint" style={{ fontSize: 12 }}>vs your edit</span>
      </div>
      {takeDiff(recipe, take.recipe).map(([k, a, b]) => (
        <div key={k} className="diff-row">
          <span className="faint">{k}</span>
          <span className="mono faint">{a}</span>
          <span className="mono">{b}</span>
        </div>
      ))}
    </div>
  );

  const historyPanel = (
    <div className="dev-panel" style={{ paddingLeft: 0, paddingRight: 0 }}>
      <div className="dev-title" style={{ padding: '0 12px' }}>
        <span>History</span>
        <span className="grow" />
        {versions.length > 0 && <span className="faint dev-note">v{versions.length}</span>}
      </div>
      {!versions.length && <div className="faint" style={{ padding: '0 12px', fontSize: 12 }}>No changes yet. Every edit, yours or the Agent's, becomes a Version here.</div>}
      {[...versions].reverse().map((v, i) => (
        <button key={v.n} className={`version ${i === 0 ? 'cur' : ''}`} onClick={() => restore(v)} title="Restore this Version">
          <span className="mono faint" style={{ width: 28 }}>v{v.n}</span>
          <span className="grow" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{v.label}</span>
          <span className={`who ${v.by}`}>{v.by === 'ai' ? 'AI' : 'You'}</span>
        </button>
      ))}
    </div>
  );

  const controls = (
    <>
    <Navigator z={zp.z}>
      <PhotoView photo={p} recipe={live} look={state.look} rendered={render.url} style={{ width: '100%', borderRadius: 0 }} />
    </Navigator>
    <div className="dev-scroll" role="tabpanel" aria-label={tab}>
      {tab === 'edit' && <EditPanel key={p.id} photo={p} recipe={recipe} look={state.look} versions={versions.length} shown={shown} onShown={setShown} onHover={setHover} onAuto={auto} onSync={sync} onPick={(label, then) => setPicking({ label, then })} diff={diff} />}
      {tab === 'crop' && <CropPanel photoId={p.id} recipe={recipe} frame={frame} />}
      {tab === 'heal' && <HealPanel photoId={p.id} recipe={recipe} sel={selSpot} onSel={setSelSpot} brush={healBrush} onBrush={setHealBrush} onRepick={(id) => void repick(id)} />}
      {tab === 'masking' && <MaskingPanel photo={p} recipe={recipe} overlay={masks} onOverlay={setMasks} sel={selMask} onSel={setSelMask} brush={brush} onBrush={setBrush} />}
      {tab === 'presets' && <PresetsPanel photo={p} recipe={recipe} look={state.look} onHover={setHover} onSet={setFromPreset} onAuto={auto} />}
      {tab === 'history' && historyPanel}
    </div>
    </>
  );
  const strip = (
    <ToolStrip
      value={tab}
      onChange={setTab}
      tools={[
        { id: 'edit', label: 'Edit', icon: SlidersHorizontal, keys: 'E' },
        { id: 'crop', label: 'Crop', icon: CropIcon, keys: 'C' },
        { id: 'heal', label: 'Heal', icon: Bandage, keys: 'H', badge: spotsOf(recipe).length || undefined },
        { id: 'masking', label: 'Masking', icon: SquareDashedMousePointer, keys: 'M', badge: recipe.local.length || undefined },
        { id: 'presets', label: 'Presets', icon: Layers, keys: '⇧P' },
        { id: 'history', label: 'History', icon: History, badge: versions.length || undefined },
      ]}
    />
  );

  return (
    <>
      <main className="panel main" style={{ flexDirection: 'column' }}>
        <header className="panel-head toolbar" data-testid="edit-head">
          <Button variant="ghost" size="sm" onClick={back}>
            <ArrowLeft size={13} /> {state.look ? 'Look' : 'Grid'} <Kbd>Esc</Kbd>
          </Button>
          <span className="mono" style={{ fontSize: 12 }}>{p.file}</span>
          <span className="faint mono head-meta" title={p.exif}>{p.exif}</span>
          <span className="grow" />
          <QueueChip />
          <Tip label={`Hold \\ for ${take ? 'your edit' : 'before'}`} keys={'\\'}>
            <span className="faint head-hint" style={{ fontSize: 11 }}>
              <Kbd>{'\\'}</Kbd> {take ? 'your edit' : 'before'}
            </span>
          </Tip>
          <Tip label="Match the rest of the Shoot to this Photo, as Suggestions" keys="⌘⇧A">
            <Button variant="ghost" size="sm" onClick={() => void toRest()} data-testid="apply-rest">
              <Copy size={12} /> Apply to rest
            </Button>
          </Tip>
          <Button variant={masks ? 'default' : 'ghost'} size="sm" onClick={() => setMasks(!masks)} aria-pressed={masks}>
            <Layers size={12} /> Masks
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setTab('history')}>
            <History size={12} /> {versions.length ? `v${versions.length}` : 'History'}
          </Button>
        </header>
        <div className="stage" data-testid="edit-stage" {...zp.stageProps}>
          <div className="stage-fit"><PhotoView photo={p} size="full" recipe={take && !before ? take.recipe : live} look={state.look} original={before && !take} rendered={take ? (before ? render.url : takeUrl) : (detail.url ?? render.url)} engineMasks={take ? undefined : engineMasks} showMasks={(masks || brush || tab === 'masking') && !before && !take} maskFocus={tab === 'masking' && selMask && !masks ? selMask : null} className={brush ? 'brushing' : ''} {...brushHandlers}>
            {before && !take && <span className="compare-flag">Before</span>}
            {take && <span className="compare-flag" data-testid="previewing">{before ? 'Your edit' : `Previewing · ${take.title}`}</span>}
            {!LIVE && take?.recipe.crop && !before && (
              <span className="crop-box" style={{ left: `${take.recipe.crop.x * 100}%`, top: `${take.recipe.crop.y * 100}%`, width: `${take.recipe.crop.w * 100}%`, height: `${take.recipe.crop.h * 100}%`, transform: `rotate(${take.recipe.crop.angle}deg)` }} />
            )}
            {LIVE && render.busy && !before && !take && <span className="compare-flag" data-testid="rendering">Rendering…</span>}
            {brush && !stroke && <span className="compare-flag">Drag to paint a Mask · Esc to stop</span>}
            {placed && (placed.mask.kind === 'linear' || placed.mask.kind === 'radial') && !before && !take && (
              <MaskHandles
                mask={placed.mask}
                onChange={(m, done) => {
                  const local = { ...placed, mask: m };
                  if (!done) return setPlacing({ local });
                  dispatch({ type: 'recipe/local-upsert', id: p.id, local, label: `Moved ${placed.name}` });
                  setPlacing(null);
                }}
              />
            )}
            {picking && !before && !take && (
              <PickOverlay
                label={picking.label}
                onCancel={() => setPicking(null)}
                onPick={(at) => {
                  setPicking(null);
                  picking.then(at);
                }}
              />
            )}
            {tab === 'heal' && !before && !take && (
              <HealOverlay
                spots={spotsOf(drawn)}
                sel={selSpot}
                frame={frame}
                r={healBrush.r}
                onAdd={(x, y) => void addSpot(x, y)}
                onSel={setSelSpot}
                onChange={(spot, done) => {
                  if (!done) return setPlacing({ spot });
                  saveSpots(spotsOf(recipe).map((s) => (s.id === spot.id ? spot : s)), 'Moved a Spot');
                  setPlacing(null);
                }}
              />
            )}
            {tab === 'crop' && !before && !take && (
              <CropOverlay
                crop={crop}
                onChange={(c, done) => {
                  if (!done) return setPlacing({ crop: c });
                  dispatch({ type: 'recipe/set', id: p.id, recipe: { ...recipe, crop: c }, label: 'Moved crop', by: 'you' });
                  setPlacing(null);
                }}
              />
            )}
          </PhotoView></div>
          <ZoomReadout z={zp.z} loading={detail.loading} />
        </div>
        <SuggestionStrip
          photo={p}
          recipe={live}
          look={state.look}
          list={takes.list}
          pick={pick}
          onPick={setPick}
          onAccept={accept}
          amount={amount}
          onAmount={setAmount}
          onDismiss={dismiss}
          onMore={() => void more()}
          onCancel={() => coming && void cancelTakes(p.id, coming).then(() => dispatch({ type: 'toast', text: 'Cancelled the new takes' }))}
          job={coming}
          held={!coming || coming.focus && !queue?.paused ? null : queue?.paused ? 'Paused' : queue?.battery_paused ? 'Waiting for the charger' : null}
          waiting={takes.loading ? 'Loading…' : 'None for this photo yet. Press R to ask for some'}
        />
        <Filmstrip order={order} pos={pos} open={(n) => go({ name: 'edit', id: n })} />
      </main>
      <Agent ctx={`edit:${p.id}`} chips={[p.file, ...(recipe.local.length ? [`${recipe.local.length} Masks`] : [])]} placeholder="A bit more on the face…" header={controls} strip={strip} />
    </>
  );
}
