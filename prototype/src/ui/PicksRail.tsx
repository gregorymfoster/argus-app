// Edit mode's left rail (mocked as MO-Edit): the Shoot's Picks in the order to work through
// them: Picks with no edit yet, then the edited ones. Clicking a row opens it; Edit's ⌘← ⌘→
// follow the Picks.

import { Check } from 'lucide-react';
import { go } from '../router';
import { useLibrary } from '../store/library';
import { isEdited, picksOf } from '../data/modes';
import type { Photo } from '../domain/types';
import { PhotoView } from './PhotoView';
import { Button } from '@/components/ui/button';

// Scroll only the rail's own list: scrollIntoView would also scroll the stage's ancestors.
// Cull mode's moment rail uses it too. A stable function, so it runs when the row turns on, not every render.
export const keepInView = (el: HTMLElement | null) => {
  const list = el?.closest('.rail-list') as HTMLElement | null;
  if (!el || !list) return;
  const top = el.offsetTop - list.offsetTop;
  if (top < list.scrollTop) list.scrollTop = top;
  else if (top + el.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = top + el.offsetHeight - list.clientHeight;
};

export function PicksRail({ open }: { open: string }) {
  const { state } = useLibrary();
  const picks = picksOf(state);
  const bare = picks.filter((p) => !isEdited(state, p));
  const done = picks.filter((p) => isEdited(state, p));
  const row = (p: Photo, dot: string, note: string) => (
    <button key={p.id} className={`moment-row pick-row ${p.id === open ? 'on' : ''}`} onClick={() => go({ name: 'edit', id: p.id })} ref={p.id === open ? keepInView : undefined} data-testid={`pick-row-${p.id}`}>
      <PhotoView photo={p} size="tile" edits recipe={state.recipes[p.id]} look={state.look} style={{ width: 34, height: 23, borderRadius: 4, flex: 'none' }} />
      <span className="mono trunc grow">{p.file}</span>
      <span className={`pick-dot ${dot}`} />
      <span className="faint pick-note">{note}</span>
    </button>
  );
  const group = (title: string, list: Photo[], dot: string, note: string, action?: React.ReactNode) =>
    list.length > 0 && (
      <>
        <div className="rail-group">
          <span>
            {title} <span className="mono">{list.length}</span>
          </span>
          {action}
        </div>
        {list.map((p) => row(p, dot, note))}
      </>
    );
  return (
    <nav className="moment-rail picks-rail" aria-label="Picks" data-testid="picks-rail">
      <div className="rail-head">
        <span>{picks.length} Picks</span>
        <span className="mono faint">{done.length} edited</span>
      </div>
      <span className="burst-progress rail-bar">
        <span style={{ width: `${picks.length ? (done.length / picks.length) * 100 : 0}%` }} />
      </span>
      <div className="rail-list">
        {group('Not edited', bare, 'todo', 'to do')}
        {group('Edited', done, 'done', 'done')}
        {!picks.length && <div className="faint rail-empty">No Picks yet. Keep some in Cull (⌘2).</div>}
      </div>
      {done.length === picks.length && picks.length > 0 && (
        <div className="rail-foot">
          <Check size={12} className="text-done" />
          <span className="grow">Every Pick is edited</span>
          <Button size="sm" variant="default" onClick={() => go({ name: 'export' })}>
            Export
          </Button>
        </div>
      )}
    </nav>
  );
}
