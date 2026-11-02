// What the mode switch needs (Library ⌘1, Cull ⌘2, Edit ⌘3, Export ⌘4; mocked as MO-*):
// each mode's progress and where Edit opens. Edit and Export work on the Picks (Greg,
// 2026-11-01: "Picks only").

import { PHOTOS } from './dummy';
import { moments, type Moment } from '../domain/moments';
import { verdictOf, type State } from '../store/library';
import type { Photo } from '../domain/types';

export type Mode = 'library' | 'cull' | 'edit' | 'export';

/** The open Shoot's Picks, in capture order. */
export const picksOf = (state: State): Photo[] => PHOTOS.slice(0, state.imported).filter((p) => verdictOf(state, p.id) === 'pick');

/** Edited at all: the person's or the AI's Recipe (live: the engine's edit stamp). */
export const isEdited = (state: State, p: Photo) => !!state.recipes[p.id] || !!p.edit;

/** The Cull pass last open (1, or a Round), so ⌘2 comes back to it. */
let lastPass = 1;
export const setLastPass = (p: number) => (lastPass = p);
export const currentPass = () => lastPass;

/** Frames flagged in Cull mode this session (the dummy store forgets a Verdict that matches the AI's). */
export const flaggedHere = new Set<string>();

/** Settled: the person Picked or Rejected it themselves. An AI Verdict alone isn't theirs (Greg, 2026-11-01). */
export const isSettled = (state: State, id: string) => {
  const o = state.overrides[id];
  return o === 'pick' || o === 'reject' || (flaggedHere.has(id) && (verdictOf(state, id) ?? 'unflagged') !== 'unflagged');
};

/** Done: every frame of the moment is settled. Cull mode's rail and the mode switch both count this. */
export const isMomentDone = (state: State, m: Moment) => m.photoIds.every((id) => isSettled(state, id));

export function cullProgress(state: State): { done: number; total: number } {
  const list = moments(PHOTOS.slice(0, state.imported));
  return { done: list.filter((m) => isMomentDone(state, m)).length, total: list.length };
}

/** Where ⌘3 opens: the current Photo if it is a Pick, else the first Pick not edited yet, else the first Pick. */
export function editTarget(state: State, current?: string): string | null {
  const picks = picksOf(state);
  if (current && picks.some((p) => p.id === current)) return current;
  return (picks.find((p) => !isEdited(state, p)) ?? picks[0])?.id ?? null;
}
