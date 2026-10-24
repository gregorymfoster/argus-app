// Lightroom's filmstrip: the whole Shoot under the loupe and Edit, so moving on never means
// going back to the grid. Only a window around the open Photo mounts, so 2,000 Photos stay cheap.

import { Fragment, useEffect, useRef } from 'react';
import { Sparkles } from 'lucide-react';
import { PHOTO_BY_ID } from '../data/dummy';
import { useLibrary, verdictOf } from '../store/library';
import { PhotoView } from './PhotoView';

const SPAN = 40;

/** `upNext`: Keepers first is on, so a mark after the open Photo says where the re-ordered
 * frames start; `moved` rings the ones that just moved up. */
export function Filmstrip({ order, pos, stackIds = [], upNext = false, moved, open }: { order: string[]; pos: number; stackIds?: string[]; upNext?: boolean; moved?: Set<string>; open: (id: string) => void }) {
  const { state } = useLibrary();
  const ref = useRef<HTMLDivElement>(null);
  const ids = order.slice(Math.max(0, pos - SPAN), pos + SPAN + 1);
  useEffect(() => {
    ref.current?.querySelector('.on')?.scrollIntoView({ block: 'nearest', inline: 'center' });
  }, [pos]);
  return (
    <div className="filmstrip" ref={ref} aria-label="Filmstrip" data-testid="filmstrip">
      {ids.map((id, i) => {
        const s = PHOTO_BY_ID[id];
        if (!s) return null;
        const v = verdictOf(state, id);
        // Every Stack's frames sit on one band, as in the grid; the open Photo's Stack is underlined.
        const stk = s.stackId ? ['stk', PHOTO_BY_ID[ids[i - 1]]?.stackId !== s.stackId && 'stk-first', PHOTO_BY_ID[ids[i + 1]]?.stackId !== s.stackId && 'stk-last'] : [];
        const cls = [id === order[pos] && 'on', stackIds.includes(id) && 'in-stack', v === 'reject' && 'rejected', moved?.has(id) && 'moved-up', ...stk].filter(Boolean).join(' ');
        const thumb = (
          <button key={id} className={cls} onClick={() => open(id)} aria-label={`${s.file}, ${s.reason}`}>
            <PhotoView photo={s} size="tile" edits style={{ width: '100%', height: '100%', borderRadius: 0 }} />
            {v === 'pick' && <span className="dot pick" />}
          </button>
        );
        if (!upNext || id !== order[pos] || pos === order.length - 1) return thumb;
        return (
          <Fragment key={id}>
            {thumb}
            <span className="up-next" data-testid="up-next">
              <Sparkles size={10} /> Up next
            </span>
          </Fragment>
        );
      })}
    </div>
  );
}
