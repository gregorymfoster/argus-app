// Work the window runs itself, outside the engine's Queue: an Export, adding files to Apple
// Photos, writing out training data. It registers here so it shows in the one place all
// background work shows (Greg, 2026-11-02: "a universal background-work-queue section"):
// the Status bar, the Background work panel and Work mode's Running list. Never a toast of
// its own while it runs; a short toast with the result when it ends is fine.

import { useSyncExternalStore } from 'react';

export interface AppWork {
  id: string;
  /** Picks the icon: 'export', 'photos', 'data'. */
  kind: string;
  label: string;
  /** What it's on now, e.g. the file being exported. */
  detail?: string;
  done: number;
  /** 0 while the size isn't known: the bar then runs without a fill. */
  total: number;
  started: number;
  /** Set when it can be stopped part way. */
  stop?: () => void;
  /** Time the Mac slept while it ran (ms): left out of the ETA, and said on its row. */
  slept?: number;
}

let works: AppWork[] = [];
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());
const subscribe = (l: () => void) => (listeners.add(l), () => void listeners.delete(l));

// A closed lid freezes the window with everything in it; the work carries on where it was on
// wake (Greg, 2026-11-02). A timer that fires late by more than a few ticks means the Mac
// slept: that time comes out of each running piece's pace, so its ETA stays honest, and its
// row says it paused.
const TICK = 5000;
/** Later than this, a tick means the Mac slept rather than a busy moment. */
export const SLEPT_MS = 30_000;
let lastTick = Date.now();
let lastSleep: { ms: number; at: number } | null = null;

/** The window noticed a gap of `gap` ms in its clock (exported for checks). */
export function noteSleep(gap: number, now = Date.now()) {
  lastSleep = { ms: gap, at: now };
  works = works.map((w) => ({ ...w, started: w.started + gap, slept: (w.slept ?? 0) + gap }));
  emit();
}

/** The last sleep the window saw, if any: how long and when it woke. */
export const lastWake = () => lastSleep;

if (typeof window !== 'undefined') {
  setInterval(() => {
    const now = Date.now();
    const gap = now - lastTick - TICK;
    lastTick = now;
    if (gap >= SLEPT_MS) noteSleep(gap, now);
  }, TICK);
  (window as { __argusSleep?: (ms: number) => void }).__argusSleep = (ms) => noteSleep(ms);
}

/** "Paused while asleep 42 min" for a row whose work slept, or null. */
export function sleptNote(ms: number | undefined): string | null {
  if (!ms || ms < SLEPT_MS) return null;
  const min = Math.round(ms / 60_000);
  return `paused while asleep ${min < 1 ? '<1' : min} min`;
}

/** Seconds left at the pace so far, once something is done. */
export function workEta(w: AppWork, now = Date.now()): number | null {
  if (!w.total || w.done <= 0) return null;
  return (((now - w.started) / w.done) * (w.total - w.done)) / 1000;
}

/** Starts showing a piece of work; call `end` when it finishes, stops or fails. */
export function startWork(w: Omit<AppWork, 'started' | 'done'> & { done?: number }) {
  const item: AppWork = { done: 0, ...w, started: Date.now() };
  works = [...works.filter((x) => x.id !== w.id), item];
  emit();
  return {
    update(change: Partial<Pick<AppWork, 'done' | 'total' | 'detail' | 'label'>>) {
      works = works.map((x) => (x.id === w.id ? { ...x, ...change } : x));
      emit();
    },
    end() {
      works = works.filter((x) => x.id !== w.id);
      emit();
    },
  };
}

/** Runs `job` as a piece of work with no known size, for the length of the promise. */
export async function asWork<T>(w: Omit<AppWork, 'started' | 'done' | 'total'>, job: () => Promise<T>): Promise<T> {
  const h = startWork({ ...w, total: 0 });
  try {
    return await job();
  } finally {
    h.end();
  }
}

export const appWorks = () => works;

/** Everything the window is running itself, oldest first. */
export function useAppWork(): AppWork[] {
  return useSyncExternalStore(subscribe, appWorks);
}
