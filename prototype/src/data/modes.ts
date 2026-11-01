// What the mode switch needs (Library ⌘1, Cull ⌘2, Edit ⌘3, Export ⌘4; mocked as MO-*):
// each mode's progress and where Edit opens. Edit and Export work on the Picks (Greg,
// 2026-11-01: "Picks only").

import { PHOTOS } from './dummy';
import { moments } from '../domain/moments';
import { verdictOf, type State } from '../store/library';
import type { Photo } from '../domain/types';

export type Mode = 'library' | 'cull' | 'edit' | 'export';

/** The open Shoot's Picks, in capture order. */
export const picksOf = (state: State): Photo[] => PHOTOS.slice(0, state.imported).filter((p) => verdictOf(state, p.id) === 'pick');

/** Edited at all: the person's or the AI's Recipe (live: the engine's edit stamp). */
export const isEdited = (state: State, p: Photo) => !!state.recipes[p.id] || !!p.edit;

/** Moments decided: one of their frames has the person's own Verdict. */
export function cullProgress(state: State): { done: number; total: number } {
  const list = moments(PHOTOS.slice(0, state.imported));
  return { done: list.filter((m) => m.photoIds.some((id) => id in state.overrides)).length, total: list.length };
}

/** Where ⌘3 opens: the current Photo if it is a Pick, else the first Pick not edited yet, else the first Pick. */
export function editTarget(state: State, current?: string): string | null {
  const picks = picksOf(state);
  if (current && picks.some((p) => p.id === current)) return current;
  return (picks.find((p) => !isEdited(state, p)) ?? picks[0])?.id ?? null;
}
