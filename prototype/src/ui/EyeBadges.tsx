// Closed eyes and soft focus from the eyes model (src/data/facts.ts), as small marks in
// the same style as the AI status mark. Informational: they never flag a Photo.

import { EyeClosed, Focus } from 'lucide-react';
import type { EyeFlags } from '../data/facts';
import { Tip } from './kit';

export function EyeBadges({ flags, inline = false }: { flags: EyeFlags | null; inline?: boolean }) {
  if (!flags) return null;
  return (
    <span className={`eye-badges ${inline ? 'inline' : ''}`} data-testid="eye-badges">
      {flags.closed && (
        <Tip label="Eyes closed on a face (from the eyes model; nothing is flagged)">
          <span className="eye-badge" data-testid="eyes-closed" aria-label="Eyes closed">
            <EyeClosed size={10} strokeWidth={2.2} />
            {inline && 'Eyes closed'}
          </span>
        </Tip>
      )}
      {flags.soft && (
        <Tip label="Soft focus on every face the eyes model could read">
          <span className="eye-badge" data-testid="soft-focus" aria-label="Soft focus">
            <Focus size={10} strokeWidth={2.2} />
            {inline && 'Soft focus'}
          </span>
        </Tip>
      )}
    </span>
  );
}
