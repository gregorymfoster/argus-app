// Engine state the UI keeps fresh by asking again every few seconds (queue, takes, eye
// flags, the meter). Polling is the cost of an open window, so it is paid once and only
// when it shows (docs/agents/performance.md):
//
// - one poll per command and arguments, however many components show it, at the fastest
//   interval any of them asks for
// - nothing while the window is hidden (minimised, behind another app); one call on return
// - components re-render only when the answer changed, not on every reply

import { useRef, useSyncExternalStore } from 'react';
import { call } from './backend';

interface Poll {
  command: string;
  args: Record<string, unknown>;
  /** The last answer and its JSON, to tell a change from a repeat. */
  value: unknown;
  text: string | null;
  subs: Set<() => void>;
  /** Each subscriber's interval; the poll runs at the smallest. */
  wants: Map<object, number>;
  timer: ReturnType<typeof setTimeout> | null;
  inFlight: boolean;
}

const polls = new Map<string, Poll>();
const hidden = () => typeof document !== 'undefined' && document.hidden;

function every(p: Poll): number {
  return Math.min(...p.wants.values());
}

function schedule(p: Poll) {
  if (p.timer) clearTimeout(p.timer);
  p.timer = null;
  if (!p.subs.size || hidden()) return;
  p.timer = setTimeout(() => void load(p), every(p));
}

async function load(p: Poll) {
  if (p.inFlight) return;
  p.inFlight = true;
  try {
    const value = await call(p.command, p.args);
    const text = JSON.stringify(value);
    if (text !== p.text) {
      p.text = text;
      p.value = value;
      p.subs.forEach((f) => f());
    }
  } catch {
    // Keep the last answer; the next round tries again.
  } finally {
    p.inFlight = false;
    schedule(p);
  }
}

if (typeof document !== 'undefined')
  document.addEventListener('visibilitychange', () => {
    for (const p of polls.values()) {
      if (hidden()) schedule(p);
      else if (p.subs.size) void load(p);
    }
  });

/** Ask every poll of `command` again now (after the UI changed what it shows). */
export function refreshPolls(command: string) {
  for (const p of polls.values()) if (p.command === command && p.subs.size) void load(p);
}

/**
 * The engine's answer to `command` with `args`, asked again every `ms` while shown.
 * `command` null polls nothing (dummy data, or nothing to show). Undefined until the first
 * answer arrives.
 */
export function usePoll<T>(command: string | null, args: Record<string, unknown>, ms: number): T | undefined {
  const key = command ? `${command} ${JSON.stringify(args)}` : '';
  const me = useRef({});
  const subscribe = (notify: () => void) => {
    if (!command) return () => {};
    let p = polls.get(key);
    if (!p) {
      p = { command, args, value: undefined, text: null, subs: new Set(), wants: new Map(), timer: null, inFlight: false };
      polls.set(key, p);
    }
    const poll = p;
    const first = !poll.subs.size;
    poll.subs.add(notify);
    poll.wants.set(me.current, ms);
    if (first) void load(poll);
    else schedule(poll);
    return () => {
      poll.subs.delete(notify);
      poll.wants.delete(me.current);
      if (!poll.subs.size) {
        if (poll.timer) clearTimeout(poll.timer);
        polls.delete(key);
      } else schedule(poll);
    };
  };
  // Re-subscribe when the key or the interval changes.
  const sub = useMemoOnce(key, ms, subscribe);
  return useSyncExternalStore(sub, () => (command ? (polls.get(key)?.value as T | undefined) : undefined));
}

// A subscribe function that stays the same while key and interval do, so
// useSyncExternalStore doesn't resubscribe on every render.
function useMemoOnce(key: string, ms: number, make: (notify: () => void) => () => void) {
  const ref = useRef<{ key: string; ms: number; fn: (notify: () => void) => () => void } | null>(null);
  if (!ref.current || ref.current.key !== key || ref.current.ms !== ms) ref.current = { key, ms, fn: make };
  return ref.current.fn;
}

/**
 * `setInterval` that rests while the window is hidden and runs `fn` once on return.
 * Returns the stop function. For polls with their own state (`usePoll` covers the rest).
 */
export function whileShown(fn: () => void, ms: number): () => void {
  let t: ReturnType<typeof setInterval> | null = null;
  const start = () => {
    if (!t) t = setInterval(fn, ms);
  };
  const stop = () => {
    if (t) clearInterval(t);
    t = null;
  };
  const onVisibility = () => {
    if (hidden()) return stop();
    if (!t) fn();
    start();
  };
  if (!hidden()) start();
  document.addEventListener('visibilitychange', onVisibility);
  return () => {
    stop();
    document.removeEventListener('visibilitychange', onVisibility);
  };
}
