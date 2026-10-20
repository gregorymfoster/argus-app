// Lightroom's filmstrip: the whole Shoot under the loupe and Edit, so moving on never means
// going back to the grid. Only a window around the open Photo mounts, so 2,000 Photos stay cheap.

import { useEffect, useRef } from 'react';
import { PHOTO_BY_ID } from '../data/dummy';
import { useLibrary, verdictOf } from '../store/library';
import { PhotoView } from './PhotoView';

const SPAN = 40;

export function Filmstrip({ order, pos, stackIds = [], open }: { order: string[]; pos: number; stackIds?: string[]; open: (id: string) => void }) {
  const { state } = useLibrary();
  const ref = useRef<HTMLDivElement>(null);
  const ids = order.slice(Math.max(0, pos - SPAN), pos + SPAN + 1);
  useEffect(() => {
    ref.current?.querySelector('.on')?.scrollIntoView({ block: 'nearest', inline: 'center' });
  }, [pos]);
  return (
    <div className="filmstrip" ref={ref} aria-label="Filmstrip" data-testid="filmstrip">
      {ids.map((id) => {
        const s = PHOTO_BY_ID[id];
        if (!s) return null;
        const v = verdictOf(state, id);
        const cls = [id === order[pos] ? 'on' : '', stackIds.includes(id) ? 'in-stack' : '', v === 'reject' ? 'rejected' : ''].join(' ');
        return (
          <button key={id} className={cls} onClick={() => open(id)} aria-label={`${s.file}, ${s.reason}`}>
            <PhotoView photo={s} size="tile" edits style={{ width: '100%', height: '100%', borderRadius: 0 }} />
            {v === 'pick' && <span className="dot pick" />}
          </button>
        );
      })}
    </div>
  );
}
