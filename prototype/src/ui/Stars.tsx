// Lightroom star Ratings. The AI's stars are a quiet suggestion (dim, with a sparkle);
// the person's are solid. Clicking a star sets it; clicking the current one clears it.

import { useEffect, useRef, useState } from 'react';
import { Sparkle, Star } from 'lucide-react';

export function Stars({ n, you, onSet, size = 10 }: { n: number; you: boolean; onSet?: (n: number) => void; size?: number }) {
  const suggested = !you && n > 0;
  // The AI re-rated it as the person culls (learn as you cull): a brief glow says so.
  const was = useRef(n);
  const [glow, setGlow] = useState(false);
  useEffect(() => {
    const changed = suggested && was.current > 0 && was.current !== n;
    was.current = n;
    if (!changed) return;
    setGlow(true);
    const t = setTimeout(() => setGlow(false), 900);
    return () => clearTimeout(t);
  }, [n, suggested]);
  return (
    <span
      className={`stars ${suggested ? 'ai' : ''} ${onSet ? 'live' : ''} ${glow ? 'rerated' : ''}`}
      role={onSet ? 'group' : 'img'}
      aria-label={n ? `${n} star${n > 1 ? 's' : ''}${suggested ? ', suggested' : ''}` : 'No stars'}
      data-stars={n}
      data-you={you || undefined}
      onClick={(e) => onSet && e.stopPropagation()}
    >
      {suggested && <Sparkle className="spark" size={size - 2} strokeWidth={2} />}
      {[1, 2, 3, 4, 5].map((i) =>
        onSet ? (
          <button key={i} className={i <= n ? 'on' : ''} aria-label={`${i} star${i > 1 ? 's' : ''}`} onClick={() => onSet(i === n && you ? 0 : i)}>
            <Star size={size} strokeWidth={2} fill={i <= n ? 'currentColor' : 'none'} />
          </button>
        ) : (
          <Star key={i} className={i <= n ? 'on' : ''} size={size} strokeWidth={2} fill={i <= n ? 'currentColor' : 'none'} />
        ),
      )}
    </span>
  );
}

/** The 0-5 keys, as in Lightroom: 0 clears. */
export const STAR_KEYS = ['0', '1', '2', '3', '4', '5'];
