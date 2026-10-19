// Open design questions the prototype lets Greg flip between. Each Decision
// is one switch; screens read the chosen option with useVariant(id). Once
// Greg picks, the winner moves into DECISIONS.md and the switch is deleted,
// so only genuinely open questions live here. The panel hides when empty.

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';

export type Screen = 'import' | 'cull' | 'loupe' | 'compare' | 'look' | 'edit';

export interface Decision {
  id: string;
  screens: Screen[] | 'all';
  question: string;
  options: { id: string; label: string; note: string }[];
}

// Example shape, kept for the next round:
// { id: 'agent', screens: ['cull'], question: 'Where does the Agent live?',
//   options: [{ id: 'side', label: 'Side panel', note: '…' }, { id: 'dock', label: 'Bottom dock', note: '…' }] }
export const DECISIONS: Decision[] = [];

export const DECISION_BY_ID: Record<string, Decision> = Object.fromEntries(DECISIONS.map((d) => [d.id, d]));

interface VariantState {
  choice: Record<string, string>;
  notes: Record<string, string>;
  set: (id: string, option: string) => void;
  note: (id: string, text: string) => void;
}

const KEY = 'argus-prototype-variants';
const Ctx = createContext<VariantState | null>(null);

function load(): { choice: Record<string, string>; notes: Record<string, string> } {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return JSON.parse(raw);
  } catch {
    /* storage can be unavailable; defaults are fine */
  }
  return { choice: {}, notes: {} };
}

export function VariantsProvider({ children }: { children: ReactNode }) {
  const [saved, setSaved] = useState(load);
  useEffect(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify(saved));
    } catch {
      /* ignore */
    }
  }, [saved]);
  // ?v=agent:dock pins variants for the screenshot script.
  useEffect(() => {
    const q = new URLSearchParams(location.search).get('v');
    if (!q) return;
    const choice: Record<string, string> = {};
    q.split(',').forEach((pair) => {
      const [k, v] = pair.split(':');
      if (k && v) choice[k] = v;
    });
    setSaved((s) => ({ ...s, choice: { ...s.choice, ...choice } }));
  }, []);
  const set = useCallback((id: string, option: string) => setSaved((s) => ({ ...s, choice: { ...s.choice, [id]: option } })), []);
  const note = useCallback((id: string, text: string) => setSaved((s) => ({ ...s, notes: { ...s.notes, [id]: text } })), []);
  const value = useMemo(() => ({ choice: saved.choice, notes: saved.notes, set, note }), [saved, set, note]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useVariants() {
  const v = useContext(Ctx);
  if (!v) throw new Error('useVariants outside VariantsProvider');
  return v;
}

export function useVariant(id: string): string {
  const { choice } = useVariants();
  return choice[id] ?? DECISION_BY_ID[id]?.options[0].id ?? '';
}

export function feedbackMarkdown(choice: Record<string, string>, notes: Record<string, string>): string {
  const lines = ['Argus prototype feedback', ''];
  for (const d of DECISIONS) {
    const pick = d.options.find((o) => o.id === (choice[d.id] ?? d.options[0].id))!;
    const touched = d.id in choice || notes[d.id];
    lines.push(`- ${d.question} **${pick.label}**${touched ? '' : ' (default, not looked at)'}${notes[d.id] ? `. ${notes[d.id]}` : ''}`);
  }
  if (notes.general) lines.push('', notes.general);
  return lines.join('\n');
}

/** A per-viewer setting remembered in localStorage (grid density and the like). */
export function usePref<T extends string>(key: string, fallback: T): [T, (v: T) => void] {
  const [v, setV] = useState<T>(() => {
    try {
      return (localStorage.getItem(`argus-pref-${key}`) as T) ?? fallback;
    } catch {
      return fallback;
    }
  });
  const set = useCallback(
    (next: T) => {
      setV(next);
      try {
        localStorage.setItem(`argus-pref-${key}`, next);
      } catch {
        /* ignore */
      }
      // Other places reading the same pref (Settings and the Import dialog) follow along.
      window.dispatchEvent(new CustomEvent('argus-pref', { detail: { key, value: next } }));
    },
    [key],
  );
  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<{ key: string; value: T }>).detail;
      if (d.key === key) setV(d.value);
    };
    window.addEventListener('argus-pref', on);
    return () => window.removeEventListener('argus-pref', on);
  }, [key]);
  return [v, set];
}
