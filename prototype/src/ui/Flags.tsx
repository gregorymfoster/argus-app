// Lightroom-style flags: a Pick flag and a Reject cross. Flags stick: Pick on a Picked Photo keeps
// it Picked (Greg, 2026-11-01); U or Unflagged clears.

import { Flag, X } from 'lucide-react';
import type { Verdict } from '../domain/types';
import { Tip, VERDICT_LABEL } from './kit';
import { useChanged } from './motion';

export function FlagButtons({ v, onSet, size = 'sm' }: { v: Verdict | null; onSet: (v: Verdict) => void; size?: 'sm' | 'lg' }) {
  const icon = size === 'lg' ? 16 : 13;
  return (
    <div className={`flags ${size}`} role="group" aria-label="Flag" onClick={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
      <Tip label="Pick" keys="Z">
        <button className={`flag pick ${v === 'pick' ? 'on' : ''}`} aria-pressed={v === 'pick'} aria-label="Pick" onClick={() => onSet('pick')}>
          <Flag size={icon} strokeWidth={2} fill={v === 'pick' ? 'currentColor' : 'none'} />
          {size === 'lg' && <span>Pick</span>}
        </button>
      </Tip>
      {size === 'lg' && (
        <Tip label="Unflag" keys="U">
          <button className={`flag unflagged ${v === 'unflagged' || !v ? 'on' : ''}`} aria-pressed={v === 'unflagged' || !v} aria-label="Unflagged" onClick={() => onSet('unflagged')}>
            <span>Unflagged</span>
          </button>
        </Tip>
      )}
      <Tip label="Reject" keys="X">
        <button className={`flag reject ${v === 'reject' ? 'on' : ''}`} aria-pressed={v === 'reject'} aria-label="Reject" onClick={() => onSet('reject')}>
          <X size={icon} strokeWidth={2.4} />
          {size === 'lg' && <span>Reject</span>}
        </button>
      </Tip>
    </div>
  );
}

/** The corner chip: blue Pick, red Reject, nothing when Unflagged. It stamps on when the flag
 * changes while it's on screen (not when a scrolled-in tile mounts). */
export function VerdictChip({ v, you }: { v: Verdict | null; you?: boolean }) {
  const stamp = useChanged(v);
  if (v !== 'pick' && v !== 'reject') return null;
  return (
    <span key={stamp} className={`chip ${v}${stamp ? ' stamp' : ''}`}>
      {v === 'pick' ? <Flag size={10} strokeWidth={2.2} fill="currentColor" /> : <X size={10} strokeWidth={2.6} />}
      <span className="label">{VERDICT_LABEL[v]}</span>
      {you && <span className="you">· you</span>}
    </span>
  );
}
