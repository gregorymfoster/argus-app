// Suggestions: whole edits the AI proposes for a Photo (sliders, crop and straighten), worked
// out on the engine's background Queue. Nothing changes until the person accepts one, which
// saves it as a new Version. With dummy data they're made up here so the screen can be
// designed without a Library.

import { useEffect, useMemo, useState } from 'react';
import { call } from './backend';
import { refreshPolls, usePoll, whileShown } from './poll';
import { LIVE, chosenProvider, liveShootId } from './live';
import { PHOTOS, PHOTO_BY_ID } from './dummy';
import type { Recipe, Suggestion } from '../domain/types';
import type { EngineTask } from '../ui/Progress';

interface EngineSuggestion {
  id: number;
  photo_id: number;
  reference_id: number | null;
  rank: number;
  title: string;
  note: string;
  recipe: Recipe;
  status: Suggestion['status'];
}

export interface QueueStatus {
  /** `stage` and `waiting` come from the process running the job: what it does right now,
   *  and what it waits on instead of working ("`argus cull` is using the model"). */
  running: { id: number; kind: string; label: string; done: number; total: number; stage?: string; waiting?: string | null } | null;
  queued: number;
  failed: number;
  done_today: number;
  eta_s: number | null;
  compute: 'gentle' | 'balanced' | 'full';
  paused: boolean;
  /** The Mac is unplugged; `battery_paused`: background work waits for the charger. */
  on_battery?: boolean;
  battery_paused?: boolean;
  /** "Run anyway" is on until the Mac is next plugged in. */
  run_anyway?: boolean;
  /** Model work (Suggestions, culls) waits until the Mac is idle and plugged in, or overnight;
   *  `run_now` overrides that until the queue is empty. */
  idle_only?: boolean;
  waiting_for_idle?: boolean;
  /** Idle, but a cull or a big batch still waits for overnight (or Run now). */
  waiting_for_overnight?: boolean;
  run_now?: boolean;
  /** The Mac is running hot: background work waits for it to cool. */
  thermal_paused?: boolean;
  /** Another Argus process has the model ("`argus cull` is using the model"). */
  model_busy?: string | null;
  /** What this process runs outside the Queue (an Import, an AI cull from the chat, Auto all). */
  tasks?: EngineTask[];
}

/** An engine message for the screen: no Markdown backticks. */
export const plain = (s: string) => s.replace(/`/g, '');

const fromEngine = (s: EngineSuggestion): Suggestion => ({
  id: String(s.id),
  photoId: String(s.photo_id),
  rank: s.rank,
  title: s.title,
  why: s.note,
  recipe: s.recipe,
  referenceId: s.reference_id == null ? null : String(s.reference_id),
  status: s.status,
});

const empty = (): Recipe => ({ schema: 1, global: {}, look: null, correction: { exposure: 0, wbK: 0, face: 0 }, local: [] });

// Dummy mode: three takes per Photo, plus matched ones once "apply to the rest" ran.
const dummyMatched: Record<string, Suggestion[]> = {};
const dummyGone = new Set<string>();
const dummyAsked = new Set<string>();
function dummyList(photoId: string): Suggestion[] {
  const p = PHOTO_BY_ID[photoId];
  if (!p) return [];
  // As on the real Queue, Picks get takes first; others when asked (R) or matched.
  const fresh = p.aiVerdict === 'pick' || dummyAsked.has(photoId);
  const backlit = p.signals.exposure < -0.3;
  const takes: Omit<Suggestion, 'id' | 'photoId' | 'referenceId' | 'status'>[] = [
    { rank: 1, title: 'Natural', why: backlit ? 'Lifts the shadows on the faces, keeps the sky.' : 'A touch brighter, true colour.', recipe: { ...empty(), global: { exposure: backlit ? 0.45 : 0.15, shadows: 25, highlights: -10 } } },
    { rank: 2, title: 'Punchy', why: 'More contrast and colour, the people lifted, cropped to 4:5.', recipe: { ...empty(), global: { exposure: 0.35, contrast: 22, highlights: -30, shadows: 20, vibrance: 20 }, crop: { angle: -1.2, x: 0.09, y: 0, w: 0.82, h: 1 } } },
    { rank: 3, title: 'Moody', why: 'Darker and graded, cool shadows and a warm sky.', recipe: { ...empty(), global: { exposure: -0.25, contrast: 20, highlights: -25, vibrance: -10, temp: -4 } } },
  ];
  const own = (fresh ? takes : []).map((f, i) => ({ ...f, id: `d${photoId}-${i}`, photoId, referenceId: null, status: 'open' as const }));
  return [...own, ...(dummyMatched[photoId] ?? [])].filter((s) => !dummyGone.has(s.id));
}

const changed = (photoId: string) => window.dispatchEvent(new CustomEvent('argus:suggestions', { detail: photoId }));
// Whoever changed Suggestions says so; the polls ask again at once.
if (LIVE) window.addEventListener('argus:suggestions', () => refreshPolls('list_suggestions'));

/** A Photo's open Suggestions, best first: fresh takes, then ones matched to a Hero Photo. */
export function useSuggestions(photoId: string): { list: Suggestion[]; loading: boolean } {
  // New takes land while the Queue works.
  const rows = usePoll<EngineSuggestion[]>(LIVE ? 'list_suggestions' : null, { photo_id: Number(photoId) }, 5000);
  // Dummy data changes in place; `argus:suggestions` says when.
  const [bump, setBump] = useState(0);
  useEffect(() => {
    if (LIVE) return;
    const onChange = (e: Event) => (e as CustomEvent).detail === photoId && setBump((n) => n + 1);
    window.addEventListener('argus:suggestions', onChange);
    return () => window.removeEventListener('argus:suggestions', onChange);
  }, [photoId]);
  const list = useMemo(() => (LIVE ? (rows ?? []).map(fromEngine).sort(byRank) : dummyList(photoId)), [rows, photoId, bump]);
  return { list, loading: LIVE && rows === undefined };
}

const byRank = (a: Suggestion, b: Suggestion) => Number(a.referenceId != null) - Number(b.referenceId != null) || a.rank - b.rank;

/** The image of a Suggestion applied to its Photo (live: rendered by the engine and saved). */
export function useSuggestionImage(s: Suggestion | null, edge: number, amount = 1): string | null {
  const [url, setUrl] = useState<string | null>(null);
  const id = s?.id ?? null;
  useEffect(() => setUrl(null), [id]);
  useEffect(() => {
    if (!LIVE || !id) return;
    let on = true;
    // Dragging Amount re-renders; wait for the drag to settle and keep the last image meanwhile.
    const t = setTimeout(() => {
      call<{ images: string[] }>('get_suggestion', { suggestion_id: Number(id), long_edge: edge, amount })
        .then((r) => on && setUrl(r.images[0] ?? null))
        .catch((e) => console.error('get_suggestion failed', e));
    }, amount === 1 ? 0 : 120);
    return () => {
      on = false;
      clearTimeout(t);
    };
  }, [id, edge, amount]);
  return url;
}

/** Every Photo's takes in the open Shoot, for the grid's badges and bulk accept. */
export function useShootTakes(): Record<string, Suggestion[]> {
  const shoot = LIVE ? liveShootId() : null;
  const rows = usePoll<EngineSuggestion[]>(LIVE && shoot != null ? 'list_suggestions' : null, { shoot_id: shoot || undefined }, 5000);
  const [bump, setBump] = useState(0);
  useEffect(() => {
    if (LIVE) return;
    const onChange = () => setBump((n) => n + 1);
    window.addEventListener('argus:suggestions', onChange);
    return () => window.removeEventListener('argus:suggestions', onChange);
  }, []);
  return useMemo(() => {
    const list = LIVE ? (rows ?? []).map(fromEngine) : PHOTOS.flatMap((p) => dummyList(p.id));
    const next: Record<string, Suggestion[]> = {};
    for (const s of list) (next[s.photoId] ??= []).push(s);
    for (const k in next) next[k].sort(byRank);
    return next;
  }, [rows, bump]);
}

/** Accept several takes (one per Photo) as one batch. Live returns the batch for Undo. */
export async function acceptMany(list: Suggestion[]): Promise<{ batch: number | null }> {
  if (!LIVE || !list.length) return { batch: null };
  const r = await call<{ batch_id: number }>('accept_suggestion', { suggestion_ids: list.map((s) => Number(s.id)) });
  for (const s of list) changed(s.photoId);
  return { batch: r.batch_id };
}

/** Accept: the Suggestion, at its Amount (1 = as proposed), becomes the Photo's new Version
 * (one undo). Live returns its batch. */
export async function acceptSuggestion(s: Suggestion, amount = 1): Promise<{ batch: number | null }> {
  if (!LIVE) return { batch: null };
  const r = await call<{ batch_id: number }>('accept_suggestion', { suggestion_id: Number(s.id), amount });
  changed(s.photoId);
  return { batch: r.batch_id };
}

export async function dismissSuggestion(s: Suggestion): Promise<void> {
  if (LIVE) await call('dismiss_suggestion', { suggestion_id: Number(s.id) });
  else dummyGone.add(s.id);
  changed(s.photoId);
}

/** Ask for new takes on one Photo. It's the Photo open in Edit, so they go to the front of
 * the queue and run at Full (`focus_photo`), not behind the Shoot's background takes. */
export async function requestSuggestions(photoId: string): Promise<void> {
  if (LIVE) {
    await call('suggest_edits', { photo_ids: [Number(photoId)], provider: chosenProvider() });
    await call('focus_photo', { photo_id: Number(photoId), next_ids: [] }).catch(() => {});
    progressChanged(photoId);
    return;
  }
  dummyJobs[photoId] = Date.now();
  progressChanged(photoId);
}

/** A Photo's takes on their way (engine `suggestions_progress`): the queued or running job,
 * its stage and when the takes should land. */
export interface TakesJob {
  id: number;
  status: 'queued' | 'running';
  /** "Queued", "Looking at the photo", "Model thinking", "Review round 1 of 1", "Saving takes". */
  stage: string;
  takes: number;
  matched: boolean;
  /** At the front of the queue, running at Full. */
  focus: boolean;
  /** Recent seconds per Photo's takes, if any have finished. */
  avg_s: number | null;
  /** Seconds until the takes land, if the queue has timings. */
  eta_s: number | null;
  progress?: number;
  elapsed_s?: number;
  ahead?: number;
  behind?: string | null;
  wait_s?: number | null;
  /** Client clock when this was read, so the bar and ETA move between polls. */
  at: number;
}

const progressChanged = (photoId: string) => window.dispatchEvent(new CustomEvent('argus:takes-progress', { detail: photoId }));

// Dummy mode: R queues behind a preview job for a moment, then walks the stages.
const dummyJobs: Record<string, number> = {};
const DUMMY_STAGES: [number, string][] = [[1.6, 'Queued'], [2.6, 'Looking at the photo'], [9.6, 'Model thinking'], [11, 'Rendering takes'], [12, 'Saving takes']];
function dummyProgress(photoId: string): TakesJob | null {
  const t0 = dummyJobs[photoId];
  if (t0 == null) return null;
  const t = (Date.now() - t0) / 1000;
  const total = DUMMY_STAGES[DUMMY_STAGES.length - 1][0];
  if (t >= total) {
    delete dummyJobs[photoId];
    dummyAsked.add(photoId);
    for (const s of dummyList(photoId)) if (!s.referenceId) dummyGone.delete(s.id);
    changed(photoId);
    return null;
  }
  const stage = DUMMY_STAGES.find(([end]) => t < end)![1];
  const queued = stage === 'Queued';
  const run = DUMMY_STAGES[0][0];
  const base = { id: 1, stage, takes: 3, matched: false, focus: true, avg_s: total - run, at: Date.now() };
  return queued
    ? { ...base, status: 'queued', ahead: 1, behind: 'Previews · Lisbon', wait_s: Math.max(1, run - t), eta_s: total - t }
    : { ...base, status: 'running', progress: (t - run) / (total - run), elapsed_s: t - run, eta_s: total - t };
}

/** The takes on their way for a Photo, read every 1.5 s while there are some. When they
 * land the Photo's Suggestions reload at once. */
export function useTakesProgress(photoId: string): TakesJob | null {
  const [job, setJob] = useState<TakesJob | null>(null);
  useEffect(() => {
    let on = true;
    let had = false;
    setJob(null);
    const load = async () => {
      let next: TakesJob | null;
      if (LIVE) {
        try {
          const r = await call<{ job: Omit<TakesJob, 'at'> | null }>('suggestions_progress', { photo_id: Number(photoId) });
          next = r.job ? { ...r.job, at: Date.now() } : null;
        } catch {
          return;
        }
      } else {
        next = dummyProgress(photoId);
      }
      if (!on) return;
      if (had && !next && LIVE) changed(photoId);
      had = next != null;
      setJob(next);
    };
    void load();
    const stop = whileShown(load, LIVE ? 1500 : 500);
    const onAsk = (e: Event) => (e as CustomEvent).detail === photoId && void load();
    window.addEventListener('argus:takes-progress', onAsk);
    return () => {
      on = false;
      stop();
      window.removeEventListener('argus:takes-progress', onAsk);
    };
  }, [photoId]);
  return job;
}

/** Stop the takes on their way (a running job stops at its next step and saves nothing). */
export async function cancelTakes(photoId: string, job: TakesJob): Promise<void> {
  if (LIVE) await call('cancel_job', { job_id: job.id });
  else delete dummyJobs[photoId];
  progressChanged(photoId);
}

/** "Apply to the rest": the Hero Photo's look becomes a Look, the others get matched takes. */
export async function applyToRest(hero: string, heroRecipe: Recipe): Promise<number> {
  if (LIVE) {
    await call('match_to_photo', { photo_id: Number(hero), provider: chosenProvider() });
    return PHOTOS.length - 1;
  }
  const file = PHOTO_BY_ID[hero]?.file.replace(/\.\w+$/, '') ?? hero;
  let n = 0;
  for (const p of PHOTOS) {
    if (p.id === hero) continue;
    const under = p.signals.exposure < -0.3;
    dummyMatched[p.id] = [
      { id: `m${p.id}-1`, photoId: p.id, rank: 1, title: `Match ${file}`, why: under ? 'Your look, brighter to match this light.' : 'Your look, as it is.', recipe: { ...heroRecipe, correction: { exposure: under ? 0.4 : 0, wbK: 0, face: 0 } }, referenceId: hero, status: 'open' },
      { id: `m${p.id}-2`, photoId: p.id, rank: 2, title: 'Match, tighter', why: 'The same look, cropped in.', recipe: { ...heroRecipe, correction: { exposure: under ? 0.4 : 0, wbK: 0, face: 0 }, crop: { angle: 0, x: 0.08, y: 0.08, w: 0.84, h: 0.84 } }, referenceId: hero, status: 'open' },
    ];
    changed(p.id);
    n++;
  }
  return n;
}

/** The background Queue, for the progress chip (live only; null with dummy data). */
export function useQueue(): QueueStatus | null {
  return usePoll<QueueStatus>(LIVE ? 'progress' : null, {}, 3000) ?? null;
}

/** "~38 min", "~2 h", or null. */
export function eta(s: number | null): string | null {
  if (s == null || !isFinite(s)) return null;
  if (s < 90) return `~${Math.max(1, Math.round(s))} s`;
  if (s < 5400) return `~${Math.round(s / 60)} min`;
  return `~${(s / 3600).toFixed(1)} h`;
}
