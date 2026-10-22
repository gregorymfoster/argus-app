// Argus-specific compositions of the shadcn/ui components in components/ui.
// Screens import from here so a control can be restyled or swapped in one place.

import type { ReactNode } from 'react';
import type { Verdict } from '../domain/types';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { Slider as UiSlider } from '@/components/ui/slider';
import { escClosedDialog } from '@/components/ui/dialog';

/** Options for a screen's Esc: a dialog's Esc closes the dialog and must not also leave the screen. */
export const ESC_OUTSIDE_DIALOGS = { ignoreEventWhen: escClosedDialog };

export const VERDICT_LABEL: Record<Verdict, string> = { pick: 'Pick', reject: 'Reject', unflagged: 'Unflagged' };
/** Lightroom's flag keys: P pick, X reject, U unflag. */
export const VERDICT_KEYS: Record<string, Verdict> = { p: 'pick', x: 'reject', u: 'unflagged' };

/** A key hint. Keycaps belong in menus, the ⌘K palette and tooltips; in bars, headers and hint lines use
 * `quiet`, a plain letter with no cap (docs/design.md, Controls). */
export function Kbd({ children, quiet }: { children: ReactNode; quiet?: boolean }) {
  return <kbd className={quiet ? 'quiet' : undefined}>{children}</kbd>;
}

/** macOS modifier order (Apple HIG): Control, Option, Shift, Command, so ⇧⌘C, never Command first. */
const MOD_ORDER = ['⌃', '⌥', '⇧', '⌘'];
function macKeys(keys: string[]) {
  const rank = (k: string) => (MOD_ORDER.includes(k) ? MOD_ORDER.indexOf(k) : MOD_ORDER.length);
  return [...keys].sort((a, b) => rank(a) - rank(b));
}

/** A menu item's shortcut, one key cap per key in macOS modifier order, pushed to the right. */
export function MenuKeys({ keys }: { keys: string[] }) {
  return (
    <span className="menu-keys">
      {macKeys(keys).map((k) => (
        <kbd key={k}>{k}</kbd>
      ))}
    </span>
  );
}

export function Tip({ label, keys, children }: { label: string; keys?: string; children: ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent>
        {label}
        {keys && <kbd>{keys}</kbd>}
      </TooltipContent>
    </Tooltip>
  );
}

export function TipProvider({ children }: { children: ReactNode }) {
  return <TooltipProvider delayDuration={400} skipDelayDuration={200}>{children}</TooltipProvider>;
}

interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  display: string;
  ai?: boolean;
  /** CSS background for the track, e.g. Temp's blue-to-yellow gradient. */
  track?: string;
  /** The neutral value, where double-click returns it (0 unless given, e.g. a midpoint's 50). */
  def?: number;
  /** The accessible name when the visible label repeats in a panel ("Detail" under Sharpening and Noise). */
  name?: string;
  onChange: (v: number) => void;
}

/** A labelled slider row; double-click resets to zero, as in Lightroom. */
export function Slider({ label, value, min, max, step = 1, display, ai, track, def = 0, name, onChange }: SliderProps) {
  return (
    <div className={`slider ${value !== def ? 'set' : ''}`} onDoubleClick={() => onChange(Math.max(min, Math.min(max, def)))}>
      <span className="slider-label" title={label}>{label}</span>
      <UiSlider min={min} max={max} step={step} value={[value]} onValueChange={([v]) => onChange(v)} aria-label={name ?? label} track={track} bipolar={min < 0} />
      <span className={`val ${ai ? 'ai' : ''}`} title={ai ? 'Set by the Agent' : undefined}>
        {display}
      </span>
    </div>
  );
}

/** A segmented choice. `quiet` for headers, tab rows and bars (no tray, one chip on the active item); the
 * default tray for form choices in panels and Settings. */
export function Seg<T extends string>({ value, onChange, items, label, className, quiet }: { value: T; onChange: (v: T) => void; items: { value: T; label: ReactNode; title?: string }[]; label: string; className?: string; quiet?: boolean }) {
  return (
    <ToggleGroup type="single" look={quiet ? 'quiet' : 'tray'} value={value} onValueChange={(v) => v && onChange(v as T)} aria-label={label} className={className}>
      {items.map((i) => (
        <ToggleGroupItem key={i.value} value={i.value} aria-label={i.title} title={i.title}>
          {i.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
