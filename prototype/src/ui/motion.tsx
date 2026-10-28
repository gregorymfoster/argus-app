// Small motion that answers the person (docs/design.md, Feedback and motion): a flag that
// stamps on, a count that ticks, a key hint that lights when its key is pressed, a check that
// draws itself, and the grid photo that grows into the Loupe. Each plays only on a change the
// person just caused, never on mount (a virtual grid remounts tiles on every scroll), and
// `prefers-reduced-motion` turns all of it off (the CSS rule in styles.css, plus `calm()`).

import { useEffect, useRef, useState } from 'react';
import { flushSync } from 'react-dom';

/** Whether the person asked the system for less motion. */
export const calm = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/**
 * 0 until `value` changes after mount, then a number that grows with each change: use it as a
 * `key` to replay a CSS animation, and as a flag that one should play at all.
 */
export function useChanged<T>(value: T): number {
  const prev = useRef(value);
  const [n, setN] = useState(0);
  useEffect(() => {
    if (Object.is(prev.current, value)) return;
    prev.current = value;
    setN((x) => x + 1);
  }, [value]);
  return n;
}

/** A count that rolls up or down a few pixels when it changes (Picks 34 → 35). */
export function Tick({ value, className }: { value: number | string; className?: string }) {
  const n = useChanged(value);
  const prev = useRef(value);
  const [dir, setDir] = useState<'up' | 'down'>('up');
  useEffect(() => {
    if (prev.current !== value) setDir(Number(value) < Number(prev.current) ? 'down' : 'up');
    prev.current = value;
  }, [value]);
  return (
    <span key={n} className={[className, n ? `tick tick-${dir}` : ''].filter(Boolean).join(' ')}>
      {value}
    </span>
  );
}

/** The bars whose quiet key hints light up when their key is pressed. */
const HINT_BARS = '.cull-foot, .loupe-bar, .look-bar, .view-foot, .panel-foot';

function hintMatches(text: string, e: KeyboardEvent) {
  const key = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  const t = text.trim();
  if (t === key) return true;
  const range = /^(\d)-(\d)$/.exec(t);
  if (range && /^\d$/.test(key)) return key >= range[1] && key <= range[2];
  const arrows: Record<string, string> = { ArrowLeft: '←', ArrowRight: '→', ArrowUp: '↑', ArrowDown: '↓', Escape: 'Esc', Enter: '↵', ' ': 'Space', Tab: 'Tab', '\\': '\\' };
  return arrows[e.key] != null && t.includes(arrows[e.key]);
}

/**
 * Key echo: pressing a shortcut lights its hint in the bar for a moment, the way Linear and
 * Raycast confirm a key. It teaches the keys and says the press landed. One listener for the
 * whole app; it only reads the DOM of the visible bars.
 */
export function useKeyEcho() {
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.repeat || calm()) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.closest('input, textarea, [contenteditable="true"], [role="dialog"]') || t.isContentEditable)) return;
      if (['Meta', 'Shift', 'Alt', 'Control'].includes(e.key)) return;
      for (const kbd of document.querySelectorAll<HTMLElement>(`:is(${HINT_BARS}) kbd.quiet`)) {
        if (!hintMatches(kbd.textContent ?? '', e)) continue;
        const item = kbd.parentElement;
        if (!item) continue;
        const mods = item.textContent ?? '';
        // ⌘Z lights only with ⌘ held, and plain Z never lights ⌘Z.
        if (mods.includes('⌘') !== e.metaKey || mods.includes('⇧') !== e.shiftKey) continue;
        item.classList.remove('echo');
        void item.offsetWidth; // restart the animation
        item.classList.add('echo');
      }
    };
    window.addEventListener('keydown', on, true);
    return () => window.removeEventListener('keydown', on, true);
  }, []);
}

type VT = { finished: Promise<void> };
type StartVT = (cb: () => Promise<void> | void) => VT;

/** Screens a photo grows into from the grid, and shrinks back out of. */
const ZOOMS = new Set(['cull>loupe', 'loupe>cull', 'cull>edit', 'edit>cull']);

/**
 * Swap screens with the open photo flying between its grid tile and the Loupe or Edit stage
 * (a same-document View Transition, in Safari 18 and Chromium; elsewhere the plain swap).
 * Only between the grid and one photo: moving from photo to photo never animates, since
 * nothing moves over the photo while the person judges it.
 */
export function routeSwap(from: string, to: string, apply: () => void) {
  const start = (document as Document & { startViewTransition?: StartVT }).startViewTransition?.bind(document);
  if (!start || calm() || !ZOOMS.has(`${from}>${to}`) || document.hidden) return apply();
  document.documentElement.dataset.zoom = to === 'cull' ? 'out' : 'in';
  const vt = start(async () => {
    flushSync(apply);
    // Wait a beat for the photo it lands on to decode, so it doesn't fly into a grey box.
    const img = document.querySelector<HTMLImageElement>(to === 'cull' ? '.grid-v .tile.cur img' : '.stage-fit img');
    if (img && !img.complete) await Promise.race([img.decode().catch(() => undefined), new Promise((r) => setTimeout(r, 120))]);
  });
  // A transition cut short by the next one (two keys in quick succession) is fine: the swap still happened.
  const vtp = vt as VT & { ready?: Promise<void>; updateCallbackDone?: Promise<void> };
  vtp.ready?.catch(() => undefined);
  vtp.updateCallbackDone?.catch(() => undefined);
  void vt.finished.catch(() => undefined).finally(() => delete document.documentElement.dataset.zoom);
}
