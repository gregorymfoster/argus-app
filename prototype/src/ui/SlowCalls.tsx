// Shows an engine call that is taking long, so a hang is never silent (Greg, 2026-11-02).
// The budgets live in data/backend.ts; this only says so, quietly, in one toast per call:
// "Still working: Apply look preview · 0:42", turning to a plain warning once it passes the
// stall budget. It goes away the moment the call answers.

import { useEffect } from 'react';
import { toast } from 'sonner';
import type { SlowCall } from '../data/backend';

const NAMES: Record<string, string> = {
  agent: 'the Agent',
  cull_shoot: 'the AI cull',
  export_photos: 'Export',
  shoot_look_preview: 'Apply look preview',
  framing_preview: 'Crop & straighten preview',
  render_photo: 'the render',
  suggest_edits: 'Suggestions',
  scan_folder: 'reading the folder',
};

export function clock(ms: number): string {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

export function SlowCalls() {
  useEffect(() => {
    const on = (e: Event) => {
      const c = (e as CustomEvent<SlowCall>).detail;
      const id = `slow-call-${c.id}`;
      if (c.done) return void toast.dismiss(id);
      const what = NAMES[c.command] ?? c.command.replace(/_/g, ' ');
      if (c.stalled) toast.warning(`No answer from ${what} for ${clock(c.ms)}`, { id, duration: Infinity, description: 'It may be stuck. The Debug log has the details.' });
      else toast(`Still working: ${what} · ${clock(c.ms)}`, { id, duration: Infinity });
    };
    window.addEventListener('argus:slow-call', on);
    return () => window.removeEventListener('argus:slow-call', on);
  }, []);
  return null;
}
