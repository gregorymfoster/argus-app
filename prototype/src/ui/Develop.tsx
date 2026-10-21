// Lightroom's Edit panel pieces: collapsible sections and the thin tool strip down the
// right edge (Edit, Masking, History). Screens compose these; the sliders themselves are
// kit's Slider.

import { useState, type ReactNode } from 'react';
import { ChevronRight, Eye, EyeOff } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Kbd } from './kit';

const OPEN_KEY = 'argus.develop.closed';

function readClosed(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(OPEN_KEY) ?? '[]') as string[]);
  } catch {
    return new Set();
  }
}

/**
 * A collapsible panel section (Light, Color, ...); which ones are closed is remembered.
 * With `shown`, an eye turns the section's effect off and on in the preview, as Lightroom's.
 */
export function Section({ id, title, aside, shown, onShown, closed, children }: { id: string; title: string; aside?: ReactNode; shown?: boolean; onShown?: (v: boolean) => void; closed?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(() => {
    const c = readClosed();
    return closed ? c.has(`+${id}`) : !c.has(id);
  });
  const toggle = () => {
    setOpen(!open);
    try {
      // A section closed by default remembers being opened as `+id`.
      const c = readClosed();
      const k = closed ? `+${id}` : id;
      if (open === !closed) c.add(k);
      else c.delete(k);
      localStorage.setItem(OPEN_KEY, JSON.stringify([...c]));
    } catch {
      // Only a convenience.
    }
  };
  return (
    <section className={`dev-section ${open ? 'open' : ''}`} data-testid={`section-${id}`}>
      <div className="dev-section-head">
        <button className="dev-section-toggle" onClick={toggle} aria-expanded={open}>
          <ChevronRight size={12} className="chev" />
          {title}
        </button>
        <span className="grow" />
        {aside}
        {onShown && (
          <button className="dev-eye" onClick={() => onShown(!shown)} aria-pressed={!shown} aria-label={shown ? `Hide ${title} in the preview` : `Show ${title} in the preview`} title={shown ? `Preview without ${title}` : `${title} is off in the preview`}>
            {shown ? <Eye size={12} /> : <EyeOff size={12} />}
          </button>
        )}
      </div>
      {open && <div className="dev-section-body">{children}</div>}
    </section>
  );
}

export interface Tool {
  id: string;
  label: string;
  icon: LucideIcon;
  keys?: string;
  badge?: string | number;
}

/** Lightroom's vertical tool strip: one icon per tool, the open one lit. */
export function ToolStrip({ tools, value, onChange }: { tools: (Tool | 'gap')[]; value: string; onChange: (id: string) => void }) {
  return (
    <nav className="tool-strip" role="tablist" aria-orientation="vertical" aria-label="Edit tools">
      {tools.map((t, i) =>
        t === 'gap' ? (
          <span key={`gap${i}`} className="grow" />
        ) : (
          <Tooltip key={t.id}>
            <TooltipTrigger asChild>
              <button role="tab" aria-selected={value === t.id} aria-label={t.label} className={value === t.id ? 'on' : ''} onClick={() => onChange(t.id)}>
                <t.icon size={16} strokeWidth={1.6} />
                {t.badge ? <span className="tool-badge">{t.badge}</span> : null}
              </button>
            </TooltipTrigger>
            <TooltipContent side="left">
              {t.label}
              {t.keys && <Kbd>{t.keys}</Kbd>}
            </TooltipContent>
          </Tooltip>
        ),
      )}
    </nav>
  );
}
