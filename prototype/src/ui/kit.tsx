// Argus-specific compositions of the shadcn/ui components in components/ui.
// Screens import from here so a control can be restyled or swapped in one place.

import type { ReactNode } from 'react';
import type { Verdict } from '../domain/types';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { Slider as UiSlider } from '@/components/ui/slider';

export const VERDICT_LABEL: Record<Verdict, string> = { pick: 'Pick', reject: 'Reject', unflagged: 'Unflagged' };
/** Lightroom's flag keys: P pick, X reject, U unflag. */
export const VERDICT_KEYS: Record<string, Verdict> = { p: 'pick', x: 'reject', u: 'unflagged' };

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd>{children}</kbd>;
}

/** A menu item's shortcut, one key cap per key, pushed to the right. */
export function MenuKeys({ keys }: { keys: string[] }) {
  return (
    <span className="menu-keys">
      {keys.map((k) => (
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

export function Seg<T extends string>({ value, onChange, items, label, className }: { value: T; onChange: (v: T) => void; items: { value: T; label: ReactNode; title?: string }[]; label: string; className?: string }) {
  return (
    <ToggleGroup type="single" value={value} onValueChange={(v) => v && onChange(v as T)} aria-label={label} className={className}>
      {items.map((i) => (
        <ToggleGroupItem key={i.value} value={i.value} aria-label={i.title} title={i.title}>
          {i.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
