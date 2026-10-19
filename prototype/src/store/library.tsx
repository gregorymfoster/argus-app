// The prototype's whole Library state in one reducer. It mirrors the real
// tool layer's verbs (set_verdict, apply_look, write_recipe, undo) so screens
// can later swap this for Tauri calls without changing shape.
//
// Live, the Library is the only source of truth for Verdicts, stars and Recipes. A flag,
// star or edit shows at once, is saved as an engine batch (one slider drag is one batch),
// and screens then take what the engine answers ('cull/states', 'recipe/load'). Its Undo
// step holds that batch, so ⌘Z is the engine's `undo_batch` and ⇧⌘Z undoes the Undo.
// Nothing here keeps its own copy to sync back.

import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, useRef, type ReactNode } from 'react';
import { LIVE, aiName, asPerson, providerDown, liveShootId, readCull, saveChats, savedChats, saveRecipe, undoBatch, writeCull, type CullState, type CullWrite, type Written } from '../data/live';
import { PHOTOS, PHOTO_BY_ID } from '../data/dummy';
import { addAdjust, emptyRecipe } from '../domain/render';
import type { Actor, Adjust, Correction, NumKey, LocalAdjust, Look, Photo, Recipe, Verdict, Version } from '../domain/types';

export interface ChatStep {
  label: string;
  ms: number;
  done: boolean;
  /** Ended in an error: shown with a cross, not a tick. */
  failed?: boolean;
}

export interface ChatMessage {
  id: string;
  role: 'you' | 'agent';
  text?: string;
  steps?: ChatStep[];
  /** `batch`: an engine batch to undo (live), instead of the prototype's own undo stack. */
  actions?: { label: string; kind: 'review' | 'undo'; ids?: string[]; batch?: number }[];
}

interface Undoable {
  aiVerdicts: Record<string, Verdict>;
  overrides: Record<string, Verdict>;
  /** Star Ratings: the AI's suggestion, and the person's own (which always wins). */
  aiStars: Record<string, number>;
  starOverrides: Record<string, number>;
  look: Look | null;
  recipes: Record<string, Recipe>;
}

export interface OverrideLog {
  photoId: string;
  what: string;
  from: string;
  to: string;
  at: number;
}

/** One Undo step: a snapshot to put back, or (live) an engine batch to undo. */
interface Step {
  snap: Undoable;
  label: string;
  key?: string;
  at: number;
  /** Live: the engine batch this step made, once saved. Undo and Redo are `undo_batch` on it. */
  batch?: Promise<Written>;
}

export interface State extends Undoable {
  imported: number;
  /** Live: how many Photos the running Import will bring in, and its ETA. */
  importTotal?: number;
  importEta?: number | null;
  importLabel?: string;
  importDone?: number;
  importing: boolean;
  culling: boolean;
  versions: Record<string, Version[]>;
  past: Step[];
  future: Step[];
  chats: Record<string, ChatMessage[]>;
  overrideLog: OverrideLog[];
  /** Shown by Sonner. `undo` adds an Undo button (batches, Sync, Look changes). */
  toast: { text: string; at: number; undo?: boolean; redo?: boolean } | null;
  /** Ids an Agent batch just touched, for "Review changes". */
  highlight: string[];
}

export type Action =
  | { type: 'import/progress'; n: number }
  | { type: 'import/start' }
  | { type: 'cull/start' }
  | { type: 'cull/write'; verdicts: Record<string, Verdict>; stars?: Record<string, number> }
  | { type: 'stars/set'; id: string; stars: number; by: Actor }
  | { type: 'verdict/set'; id: string; verdict: Verdict; by: Actor }
  | { type: 'look/apply'; look: Look; corrections: Record<string, Correction>; ids: string[] }
  | { type: 'look/adjust'; key: NumKey; value: number }
  | { type: 'look/clear' }
  | { type: 'look/sync'; id: string }
  | { type: 'correction/set'; id: string; patch: Partial<Correction>; by: Actor }
  | { type: 'recipe/global'; id: string; key: NumKey; value: number; by: Actor }
  | { type: 'recipe/load'; id: string; recipe: Recipe }
  /** `key` makes steady changes (a slider drag) one Version and one Undo step. */
  | { type: 'recipe/set'; id: string; recipe: Recipe; label: string; by: Actor; key?: string }
  | { type: 'recipe/local-upsert'; id: string; local: LocalAdjust; label: string }
  | { type: 'recipe/local-adjust'; id: string; localId: string; key: NumKey; value: number; by: Actor }
  | { type: 'recipe/local-remove'; id: string; localId: string }
  | { type: 'version/restore'; id: string; n: number }
  | { type: 'batch'; label: string; actions: Action[] }
  | { type: 'undo'; batch?: Promise<Written> }
  | { type: 'redo'; batch?: Promise<Written> }
  | { type: 'engine/saving'; change: Action; batch: Promise<Written> }
  | { type: 'engine/saved'; label: string; batch: Promise<Written> }
  | { type: 'look/load'; looks: Look[] }
  | { type: 'cull/states'; states: CullState[] }
  | { type: 'chat/push'; ctx: string; msg: ChatMessage }
  | { type: 'chat/patch'; ctx: string; msgId: string; patch: Partial<ChatMessage> }
  | { type: 'highlight'; ids: string[] }
  | { type: 'toast'; text: string }
  /** Photos left or came back (Remove from Library, its Undo): the grid shows `imported` of PHOTOS. */
  | { type: 'photos/changed'; imported: number }
  | { type: 'import/live'; n: number; done: number; total: number; eta: number | null; importing: boolean; label?: string }
  | { type: 'reset'; skipImport?: boolean };

function initial(skipImport = false): State {
  return {
    aiVerdicts: skipImport ? Object.fromEntries(PHOTOS.map((p) => [p.id, p.aiVerdict])) : {},
    overrides: skipImport ? Object.fromEntries(PHOTOS.filter((p) => p.personVerdict).map((p) => [p.id, p.personVerdict!])) : {},
    aiStars: skipImport ? Object.fromEntries(PHOTOS.filter((p) => p.aiStars).map((p) => [p.id, p.aiStars!])) : {},
    starOverrides: skipImport ? Object.fromEntries(PHOTOS.filter((p) => p.personStars != null).map((p) => [p.id, p.personStars!])) : {},
    look: null,
    recipes: {},
    imported: skipImport ? PHOTOS.length : 0,
    importing: false,
    culling: false,
    versions: {},
    past: [],
    future: [],
    chats: (LIVE && restoredChats()) || (skipImport ? { cull: [seedCullMessage()] } : {}),
    overrideLog: [],
    toast: null,
    highlight: [],
  };
}

/** Live: the Shoot's earlier conversations. A step still running when the page went away is shown as stopped. */
function restoredChats(): Record<string, ChatMessage[]> | null {
  const chats = savedChats<Record<string, ChatMessage[]>>();
  if (!chats) return null;
  for (const list of Object.values(chats)) for (const m of list) m.steps = m.steps?.map((st) => ({ ...st, done: true }));
  // The intro is always today's copy, not the one saved with an older version.
  if (chats.cull?.[0]?.id === 'seed') chats.cull[0] = seedCullMessage();
  return chats;
}

function seedCullMessage(): ChatMessage {
  const n = (v: Verdict) => PHOTOS.filter((p) => p.aiVerdict === v).length;
  if (LIVE && PHOTOS.length === 0) {
    return { id: 'seed', role: 'agent', text: 'Your Library is empty. Import a folder or memory card and I can cull it for you.' };
  }
  if (LIVE) {
    return {
      id: 'seed',
      role: 'agent',
      // Name the model only when it can run: the panel says what's wrong otherwise.
      text: `Ask me to cull this Shoot and ${providerDown() ? 'I' : aiName()} will flag, star and stack all ${PHOTOS.length} Photos. Open a Photo and ask me to fix the light, or ask here to make the whole Shoot consistent. Everything I do can be undone.`,
    };
  }
  return {
    id: 'seed',
    role: 'agent',
    steps: [
      { label: 'Scored 112 photos ·3.8s', ms: 0, done: true },
      { label: 'Found 9 bursts ·0.4s', ms: 0, done: true },
      { label: 'Wrote Verdicts and stars ·0.1s', ms: 0, done: true },
    ],
    text: `I went through all ${PHOTOS.length}. ${n('pick')} are keepers, ${n('unflagged')} are close calls I left unflagged, and the rest are soft, mid-blink or duplicates. Bursts are collapsed to their best frame.`,
  };
}

const snap = (s: State): Undoable => ({ aiVerdicts: s.aiVerdicts, overrides: s.overrides, aiStars: s.aiStars, starOverrides: s.starOverrides, look: s.look, recipes: s.recipes });

/** A Photo's stars and whether the person set them (0 = unrated). */
export function starsOf(s: Pick<State, 'aiStars' | 'starOverrides'>, id: string): { stars: number; you: boolean } {
  const you = s.starOverrides[id];
  return you != null ? { stars: you, you: true } : { stars: s.aiStars[id] ?? 0, you: false };
}

export function recipeOf(s: Pick<State, 'recipes' | 'look'>, id: string): Recipe {
  return s.recipes[id] ?? emptyRecipe(s.look?.id ?? null);
}

export function verdictOf(s: Pick<State, 'aiVerdicts' | 'overrides'>, id: string): Verdict | null {
  return s.overrides[id] ?? s.aiVerdicts[id] ?? null;
}

function withVersion(s: State, id: string, recipe: Recipe, label: string, by: Actor, key?: string): State {
  const list = s.versions[id] ?? [];
  const last = list[list.length - 1];
  const coalesce = key && last && last.label === label && Date.now() - last.at < 1500;
  const next = coalesce
    ? [...list.slice(0, -1), { ...last, recipe, at: Date.now() }]
    : [...list, { n: (last?.n ?? 0) + 1, label, by, at: Date.now(), recipe }];
  return { ...s, recipes: { ...s.recipes, [id]: recipe }, versions: { ...s.versions, [id]: next } };
}

function apply(s: State, a: Action): State {
  switch (a.type) {
    case 'import/start':
      return { ...s, importing: true, imported: 0 };
    case 'import/progress':
      return { ...s, imported: Math.min(PHOTOS.length, a.n), importing: a.n < PHOTOS.length };
    case 'photos/changed':
      return { ...s, imported: a.imported };
    case 'import/live':
      return { ...s, imported: a.n, importing: a.importing, importTotal: a.total, importEta: a.eta, importLabel: a.label, importDone: a.done };
    case 'cull/start':
      return { ...s, culling: true };
    case 'cull/write':
      return { ...s, culling: false, aiVerdicts: { ...s.aiVerdicts, ...a.verdicts }, aiStars: { ...s.aiStars, ...a.stars } };
    case 'stars/set': {
      if (a.by === 'ai') return { ...s, aiStars: { ...s.aiStars, [a.id]: a.stars } };
      const ai = s.aiStars[a.id];
      const log = ai && ai !== a.stars ? [...s.overrideLog, { photoId: a.id, what: 'Rating', from: `${ai}★`, to: `${a.stars}★`, at: Date.now() }] : s.overrideLog;
      // Pressing the AI's own number confirms it: the stars become the person's.
      return { ...s, starOverrides: { ...s.starOverrides, [a.id]: a.stars }, overrideLog: log };
    }
    case 'verdict/set': {
      const overrides = { ...s.overrides };
      const ai = s.aiVerdicts[a.id];
      const before = verdictOf(s, a.id);
      if (a.by === 'ai') return { ...s, aiVerdicts: { ...s.aiVerdicts, [a.id]: a.verdict } };
      // Live, the Library keeps the person's Verdict even when it matches the AI's, as with stars.
      if (ai === a.verdict && !LIVE) delete overrides[a.id];
      else overrides[a.id] = a.verdict;
      const log = ai && ai !== a.verdict ? [...s.overrideLog, { photoId: a.id, what: 'Verdict', from: ai, to: a.verdict, at: Date.now() }] : s.overrideLog;
      void before;
      return { ...s, overrides, overrideLog: log };
    }
    case 'look/apply': {
      const recipes = { ...s.recipes };
      for (const id of a.ids) {
        const r = recipeOf(s, id);
        recipes[id] = { ...r, look: a.look.id, correction: a.corrections[id] ?? r.correction };
      }
      return { ...s, look: a.look, recipes };
    }
    case 'look/adjust':
      if (!s.look) return s;
      return { ...s, look: { ...s.look, adjust: { ...s.look.adjust, [a.key]: a.value } } };
    case 'look/clear':
      return { ...s, look: null };
    case 'look/sync': {
      // Lightroom's "Sync": fold one Photo's global sliders into the shared Look.
      const r = recipeOf(s, a.id);
      const look: Look = s.look ? { ...s.look, adjust: addAdjust(s.look.adjust, r.global) } : { id: 'shoot', name: 'Shoot Look', adjust: { ...r.global } };
      return withVersion({ ...s, look }, a.id, { ...r, global: {}, look: look.id }, 'Synced to the Look', 'you');
    }
    case 'correction/set': {
      const r = recipeOf(s, a.id);
      return withVersion(s, a.id, { ...r, correction: { ...r.correction, ...a.patch } }, 'Correction', a.by, 'corr');
    }
    case 'recipe/load':
      // A Recipe read from the Library: not a change, so no Version and no Undo step.
      return { ...s, recipes: { ...s.recipes, [a.id]: a.recipe } };
    case 'recipe/set':
      return withVersion(s, a.id, a.recipe, a.label, a.by, a.key);
    case 'recipe/global': {
      const r = recipeOf(s, a.id);
      return withVersion(s, a.id, { ...r, global: { ...r.global, [a.key]: a.value } }, `${cap(a.key)} ${fmt(a.key, a.value)}`, a.by, `g:${a.key}`);
    }
    case 'recipe/local-upsert': {
      const r = recipeOf(s, a.id);
      const exists = r.local.some((l) => l.id === a.local.id);
      const local = exists ? r.local.map((l) => (l.id === a.local.id ? a.local : l)) : [...r.local, a.local];
      return withVersion(s, a.id, { ...r, local }, a.label, a.local.source);
    }
    case 'recipe/local-adjust': {
      const r = recipeOf(s, a.id);
      const local = r.local.map((l) => (l.id === a.localId ? { ...l, adjust: { ...l.adjust, [a.key]: a.value } } : l));
      const name = r.local.find((l) => l.id === a.localId)?.name ?? 'Mask';
      return withVersion(s, a.id, { ...r, local }, `${name}: ${cap(a.key)} ${fmt(a.key, a.value)}`, a.by, `l:${a.localId}:${a.key}`);
    }
    case 'recipe/local-remove': {
      const r = recipeOf(s, a.id);
      const name = r.local.find((l) => l.id === a.localId)?.name ?? 'Mask';
      return withVersion(s, a.id, { ...r, local: r.local.filter((l) => l.id !== a.localId) }, `Removed ${name}`, 'you');
    }
    case 'version/restore': {
      const v = s.versions[a.id]?.find((x) => x.n === a.n);
      if (!v) return s;
      return withVersion(s, a.id, v.recipe, `Restored v${a.n}`, 'you');
    }
    case 'batch':
      return a.actions.reduce(apply, s);
    case 'chat/push':
      return { ...s, chats: { ...s.chats, [a.ctx]: [...(s.chats[a.ctx] ?? []), a.msg] } };
    case 'chat/patch':
      return { ...s, chats: { ...s.chats, [a.ctx]: (s.chats[a.ctx] ?? []).map((m) => (m.id === a.msgId ? { ...m, ...a.patch } : m)) } };
    case 'cull/states': {
      // What the Library has now. The person's own Verdict and stars win, as in the engine.
      const [aiVerdicts, overrides, aiStars, starOverrides] = [{ ...s.aiVerdicts }, { ...s.overrides }, { ...s.aiStars }, { ...s.starOverrides }];
      for (const p of a.states) {
        const id = String(p.id);
        if (p.verdict_by === 'person') overrides[id] = p.verdict;
        else {
          delete overrides[id];
          aiVerdicts[id] = p.verdict;
        }
        if (p.stars_by === 'person') starOverrides[id] = p.stars;
        else {
          delete starOverrides[id];
          if (p.stars) aiStars[id] = p.stars;
          else delete aiStars[id];
        }
      }
      return { ...s, aiVerdicts, overrides, aiStars, starOverrides };
    }
    case 'look/load': {
      // The Library's Looks: the Look screen shows the one most of the loaded Recipes are under.
      const under = new Map<string, number>();
      for (const r of Object.values(s.recipes)) if (r.look) under.set(r.look, (under.get(r.look) ?? 0) + 1);
      const look = a.looks.filter((l) => under.has(l.id)).sort((x, y) => under.get(y.id)! - under.get(x.id)!)[0] ?? null;
      return { ...s, look };
    }
    case 'highlight':
      return { ...s, highlight: a.ids };
    case 'toast':
      return { ...s, toast: { text: a.text, at: Date.now() } };
    default:
      return s;
  }
}

const UNDOABLE = new Set<Action['type']>(['verdict/set', 'stars/set', 'look/apply', 'look/adjust', 'look/clear', 'look/sync', 'correction/set', 'recipe/global', 'recipe/set', 'recipe/local-upsert', 'recipe/local-adjust', 'recipe/local-remove', 'version/restore', 'batch']);

function undoLabel(a: Action): { label: string; key?: string } {
  switch (a.type) {
    case 'batch':
      return { label: a.label };
    case 'recipe/set':
      return { label: a.label, key: a.key && `s:${a.id}:${a.key}` };
    case 'verdict/set':
      return { label: `${cap(a.verdict)} on ${PHOTO_BY_ID[a.id]?.file ?? a.id}` };
    case 'stars/set':
      return { label: `${a.stars ? `${a.stars} stars` : 'No stars'} on ${PHOTO_BY_ID[a.id]?.file ?? a.id}` };
    case 'look/adjust':
      return { label: `Look ${a.key}`, key: `look:${a.key}` };
    case 'look/sync':
      return { label: 'Sync to the Look' };
    case 'look/clear':
      return { label: 'Remove Look' };
    case 'recipe/global':
      return { label: `${cap(a.key)}`, key: `g:${a.id}:${a.key}` };
    case 'recipe/local-adjust':
      return { label: `Mask ${a.key}`, key: `l:${a.id}:${a.localId}:${a.key}` };
    case 'correction/set':
      return { label: 'Correction', key: `c:${a.id}` };
    default:
      return { label: a.type };
  }
}

/** Live, a snapshot never puts back Verdicts, stars or Recipes: those come from the Library. */
const restore = (u: Undoable): Partial<Undoable> => {
  if (!LIVE) return u;
  const { aiVerdicts: _v, overrides: _o, aiStars: _s, starOverrides: _so, recipes: _r, ...rest } = u;
  return rest;
};

function reducer(s: State, a: Action): State {
  if (a.type === 'reset') return initial(a.skipImport);
  if (a.type === 'undo') {
    const last = s.past[s.past.length - 1];
    if (!last) return { ...s, toast: { text: 'Nothing to undo', at: Date.now() } };
    const back = last.batch ? {} : restore(last.snap);
    return { ...s, ...back, past: s.past.slice(0, -1), future: [...s.future, { snap: snap(s), label: last.label, at: Date.now(), batch: a.batch }], toast: { text: `Undid ${last.label}`, at: Date.now(), redo: true } };
  }
  if (a.type === 'redo') {
    const next = s.future[s.future.length - 1];
    if (!next) return s;
    const back = next.batch ? {} : restore(next.snap);
    return { ...s, ...back, future: s.future.slice(0, -1), past: [...s.past, { snap: snap(s), label: next.label, at: Date.now(), batch: a.batch }], toast: { text: `Redid ${next.label}`, at: Date.now() } };
  }
  // Live: shown at once; the Library's answer follows as 'cull/states' or 'recipe/load'.
  if (a.type === 'engine/saving') return record(s, a.change, a.batch);
  // Live: a change the engine already made (Sync to Look, a Suggestion), as an Undo step.
  if (a.type === 'engine/saved') return { ...s, past: [...s.past, { snap: snap(s), label: a.label, at: Date.now(), batch: a.batch }], future: [] };
  return record(s, a);
}

/** Apply a change and add its Undo step (one per slider drag: steady changes to one key coalesce). */
function record(s: State, a: Action, batch?: Promise<Written>): State {
  const out = apply(s, a);
  if (!UNDOABLE.has(a.type) || out === s) return out;
  const { label, key } = undoLabel(a);
  const prev = s.past[s.past.length - 1];
  if (key && prev?.key === key && prev.batch === batch && Date.now() - prev.at < 1500) {
    return { ...out, past: [...s.past.slice(0, -1), { ...prev, at: Date.now() }], future: [] };
  }
  const big = a.type === 'batch' || (a.type === 'recipe/set' && !a.key) || a.type === 'look/sync' || a.type === 'look/clear' || a.type === 'version/restore';
  return { ...out, past: [...s.past, { snap: snap(s), label, key, at: Date.now(), batch }], future: [], toast: big ? { text: label, at: Date.now(), undo: true } : out.toast };
}

const RECIPE_CHANGES = new Set<Action['type']>(['recipe/global', 'recipe/local-adjust', 'correction/set', 'recipe/local-upsert', 'recipe/local-remove', 'recipe/set']);

/** Live: the Photo whose Recipe this action changes, or null. */
function recipeChange(a: Action): string | null {
  return RECIPE_CHANGES.has(a.type) && 'id' in a ? a.id : null;
}

export function cap(k: string) {
  const words: Record<string, string> = { temp: 'Temperature', wbK: 'White balance', skin: 'Skin even' };
  return words[k] ?? k.charAt(0).toUpperCase() + k.slice(1);
}

/** Sliders that only go up from 0: shown without a sign. */
const UNSIGNED = new Set(['fade', 'grain', 'skin', 'eyes', 'teeth', 'vignetteMidpoint', 'vignetteFeather', 'vignetteHighlights', 'grainSize', 'grainRoughness', 'sharpen', 'sharpenDetail', 'sharpenMasking', 'noise', 'noiseDetail', 'noiseContrast', 'colorNoise', 'colorNoiseDetail', 'colorNoiseSmoothness', 'defringePurple', 'defringeGreen', 'lensVignetteMidpoint', 'sat', 'hue°', 'blending', 'refineSat', 'split']);

export function fmt(key: keyof Adjust | string, v: number) {
  const sign = v > 0 ? '+' : v < 0 ? '−' : '';
  if (key === 'exposure' || key === 'face') return `${sign}${Math.abs(v).toFixed(2)}`;
  if (key === 'sharpenRadius') return v.toFixed(1);
  if (key === 'removeCA') return v ? 'on' : 'off';
  if (UNSIGNED.has(key)) return `${Math.round(v)}`;
  return `${sign}${Math.abs(Math.round(v))}`;
}

const Ctx = createContext<{ state: State; dispatch: React.Dispatch<Action> } | null>(null);

/** Live: the flags and stars the person set in this action, as one Library write (null if none). */
function cullWrite(a: Action): CullWrite | null {
  const sets = a.type === 'batch' ? a.actions : [a];
  if (!sets.every((x) => (x.type === 'verdict/set' || x.type === 'stars/set') && x.by === 'you')) return null;
  const w: CullWrite = { label: a.type === 'batch' ? a.label : undoLabel(a).label, flags: [], stars: [] };
  for (const x of sets) {
    if (x.type === 'verdict/set') {
      const g = w.flags.find((f) => f.verdict === x.verdict);
      if (g) g.ids.push(x.id);
      else w.flags.push({ ids: [x.id], verdict: x.verdict });
    } else if (x.type === 'stars/set') {
      const g = w.stars.find((f) => f.stars === x.stars);
      if (g) g.ids.push(x.id);
      else w.stars.push({ ids: [x.id], stars: x.stars });
    }
  }
  return w.flags.length || w.stars.length ? w : null;
}

/** Live: an edit being made (a slider drag), saved as one batch once it settles. */
interface Pending {
  key: string;
  /** Saves the newest value of the drag. */
  write: () => Promise<Written>;
  timer: ReturnType<typeof setTimeout>;
  batch: Promise<Written>;
  save: () => void;
}

/** How long a slider has to rest before its drag is saved as one batch. */
const SETTLE_MS = 600;

export function LibraryProvider({ children, skipImport }: { children: ReactNode; skipImport?: boolean }) {
  const [state, raw] = useReducer(reducer, skipImport, initial);
  // The state as of the last dispatch, ahead of React's render, so quick changes build on each other.
  const now = useRef(state);
  now.current = state;
  const pending = useRef<Pending | null>(null);
  // Live, flags, stars, edits, Undo and Redo go to the Library; screens then show its answer.
  const dispatch = useCallback((a: Action) => {
    if (!LIVE) return raw(a);
    const send = (x: Action) => {
      now.current = reducer(now.current, x);
      raw(x);
    };
    const settle = (batch: Promise<Written>, ids: string[]) =>
      batch.then(
        (w) => {
          if (w.photos.length) send({ type: 'cull/states', states: w.photos });
          for (const [id, recipe] of Object.entries(w.recipes ?? {})) {
            send({ type: 'recipe/load', id, recipe });
            window.dispatchEvent(new CustomEvent('argus:versions', { detail: id }));
          }
          if (w.looks) send({ type: 'look/load', looks: w.looks });
        },
        (e) => {
          send({ type: 'toast', text: `Not saved: ${String(e).replace(/^Error: /, '')}` });
          if (ids.length) void readCull(ids).then((states) => send({ type: 'cull/states', states }), () => {});
        },
      );
    // Save the edit in progress now (before an Undo, or when another slider moves).
    const flush = () => {
      const p = pending.current;
      if (!p) return;
      pending.current = null;
      clearTimeout(p.timer);
      p.save();
    };
    const afterSave = (id: string) => (w: Written) => {
      window.dispatchEvent(new CustomEvent('argus:versions', { detail: id }));
      return w;
    };

    const w = cullWrite(a);
    if (w) {
      const batch = writeCull(w);
      void settle(batch, [...w.flags, ...w.stars].flatMap((g) => g.ids));
      return send({ type: 'engine/saving', change: a, batch });
    }
    // A drag (steady changes to one key) is saved once it rests, as one batch; anything
    // else is saved at once.
    const save = (change: Action, key: string | undefined, write: () => Promise<Written>) => {
      const p = pending.current;
      if (key && p && p.key === key) {
        p.write = write;
        clearTimeout(p.timer);
        p.timer = setTimeout(flush, SETTLE_MS);
        return send({ type: 'engine/saving', change, batch: p.batch });
      }
      flush();
      if (!key) {
        const batch = write();
        void settle(batch, []);
        return send({ type: 'engine/saving', change, batch });
      }
      let start = () => {};
      const batch = new Promise<Written>((resolve, reject) => {
        start = () => void next.write().then(resolve, reject);
      });
      const next: Pending = { key, write, timer: setTimeout(flush, SETTLE_MS), batch, save: () => start() };
      pending.current = next;
      void settle(batch, []);
      return send({ type: 'engine/saving', change, batch });
    };
    const id = recipeChange(a);
    if (id) {
      const after = apply(now.current, a);
      const recipe = recipeOf(after, id);
      const versions = after.versions[id];
      const label = versions?.[versions.length - 1]?.label ?? undoLabel(a).label;
      return save(a, undoLabel(a).key, () => saveRecipe(id, recipe, label).then(afterSave(id)));
    }
    const look = now.current.look;
    if (a.type === 'look/adjust' && look) {
      const args = { shoot_id: liveShootId(), look_id: look.id, set: { [a.key]: a.value } };
      return save(a, undoLabel(a).key, () => asPerson('set_look', args, `Look: ${cap(a.key)} ${fmt(a.key, a.value)}`));
    }
    if (a.type === 'look/clear' && look) {
      const args = { shoot_id: liveShootId(), look_id: look.id };
      return save(a, undefined, () => asPerson('remove_look', args, `Removed ${look.name}`, (r) => r.removed_from as number[]));
    }
    if (a.type === 'engine/saved') {
      void settle(a.batch, []);
      return send(a);
    }
    if (a.type === 'undo' || a.type === 'redo') {
      flush();
      const s = now.current;
      const step = a.type === 'undo' ? s.past[s.past.length - 1] : s.future[s.future.length - 1];
      if (step?.batch) {
        const batch = step.batch.then((done) => {
          if (done.batch == null) throw new Error('that change was never saved');
          return undoBatch(done.batch);
        });
        void settle(batch, []);
        return send({ ...a, batch });
      }
    }
    send(a);
  }, []);
  useEffect(() => {
    if (LIVE) saveChats(state.chats);
  }, [state.chats]);
  const value = useMemo(() => ({ state, dispatch }), [state, dispatch]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useLibrary() {
  const v = useContext(Ctx);
  if (!v) throw new Error('useLibrary outside LibraryProvider');
  return v;
}

export function visiblePhotos(s: State): Photo[] {
  return PHOTOS.slice(0, s.imported);
}
