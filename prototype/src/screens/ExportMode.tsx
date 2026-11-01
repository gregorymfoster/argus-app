// Export mode (⌘4, its own mode: Greg 2026-11-01; mocked as MO-Export): the Export dialog's
// presets and settings around every Photo that will go out, so you see what leaves before it
// does. Picks of the open Shoot by default; ⌫ leaves one out. A line flags Picks that aren't
// finished (a Shoot match waiting, or no edit) with a one-click fix.

import { useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { AlertCircle, Download, Flag, Undo2 } from 'lucide-react';
import { SHOOT } from '../data/dummy';
import { useLibrary } from '../store/library';
import { isEdited, picksOf } from '../data/modes';
import { acceptPreEdits, usePreEdits } from '../data/preEdit';
import { go } from '../router';
import { PhotoView } from '../ui/PhotoView';
import { ExportModeBody } from '../ui/Export';
import { Hints, ModeSwitch, QueueMeter } from '../ui/ViewBar';
import { Kbd } from '../ui/kit';
import { EmptyState } from '../ui/Empty';
import { Button } from '@/components/ui/button';

export function ExportScreen() {
  const { state, dispatch } = useLibrary();
  const pre = usePreEdits(state);
  const picks = picksOf(state);
  const [out, setOut] = useState<Set<string>>(new Set());
  const [focus, setFocus] = useState<string | null>(null);
  const photos = picks.filter((p) => !out.has(p.id));
  const waiting = photos.filter((p) => pre.byPhoto[p.id]);
  const bare = photos.filter((p) => !pre.byPhoto[p.id] && !isEdited(state, p));
  const toggleOut = (id: string) => setOut((o) => (o.has(id) ? new Set([...o].filter((x) => x !== id)) : new Set([...o, id])));

  useHotkeys(['backspace', 'delete'], () => focus && toggleOut(focus), [focus]);
  useHotkeys(['escape', 'g'], () => go({ name: 'cull' }));

  const middle = (
    <div className="export-photos" data-testid="export-photos">
      {waiting.length + bare.length > 0 && (
        <div className="export-warn" data-testid="export-warn">
          <AlertCircle size={12} />
          <span className="trunc">
            {[waiting.length && `${waiting.length} with a Shoot match waiting`, bare.length && `${bare.length} not edited`].filter(Boolean).join(', ')}
          </span>
          <span className="grow" />
          {waiting.length > 0 && (
            <Button variant="ghost" size="sm" onClick={() => acceptPreEdits(waiting.map((p) => pre.byPhoto[p.id]), dispatch)}>
              Accept {waiting.length === 1 ? 'the match' : `the ${waiting.length} matches`}
            </Button>
          )}
          <Button variant="ghost" size="sm" onClick={() => go({ name: 'edit', id: (waiting[0] ?? bare[0]).id })}>
            Open in Edit <Kbd quiet>⌘3</Kbd>
          </Button>
        </div>
      )}
      {picks.length ? (
        <div className="export-grid">
          {picks.map((p) => {
            const left = out.has(p.id);
            // "not edited" only stands out once some Picks are: on a fresh Shoot it would be on every tile.
            const note = left ? 'left out' : pre.byPhoto[p.id] ? 'match waiting' : bare.length < photos.length && !isEdited(state, p) ? 'not edited' : null;
            return (
              <figure key={p.id} className={`export-tile ${left ? 'left-out' : ''} ${focus === p.id ? 'focus' : ''}`} onClick={() => setFocus(p.id)} onDoubleClick={() => go({ name: 'edit', id: p.id })} data-testid={`export-${p.id}`}>
                <PhotoView photo={p} size="tile" edits recipe={state.recipes[p.id]} look={state.look} />
                <span className="corner">{left ? <Undo2 size={10} /> : <Flag size={10} strokeWidth={2.2} />}</span>
                {note && <span className={`export-note ${left ? '' : 'warn'}`}>{note}</span>}
                <figcaption className="mono trunc">{p.file}</figcaption>
              </figure>
            );
          })}
        </div>
      ) : (
        <EmptyState art="tray" title="No Picks to export yet" testId="export-empty">
          Keep the best of each moment in Cull first.
          <Button size="sm" onClick={() => go({ name: 'moments' })}>
            Cull <Kbd>⌘2</Kbd>
          </Button>
        </EmptyState>
      )}
    </div>
  );

  return (
    <div className="panel main view" style={{ flexDirection: 'column', borderRadius: 0, border: 'none', background: 'var(--bg)' }} data-testid="export-screen">
      <ExportModeBody
        mode={{
          photos,
          middle,
          head: ({ n, facts, start, running }) => (
            <header className="panel-head view-head">
              <ModeSwitch mode="export" />
              <span className="vsep" />
              <span className="view-file trunc">{n === picks.length ? `${n} Picks` : `${n} of ${picks.length} Picks`}</span>
              <span className="mono faint view-meta">{SHOOT.name}</span>
              <span className="grow" />
              <span className="faint view-meta wide-only">{facts}</span>
              <QueueMeter />
              <Button size="sm" variant="default" onClick={start} disabled={!n || running} data-testid="export-start">
                <Download size={12} /> Export {n} <Kbd>⌘↵</Kbd>
              </Button>
            </header>
          ),
        }}
      />
      <footer className="view-foot">
        <span>{out.size ? `${out.size} left out` : 'Originals are never touched'}</span>
        <span className="grow" />
        <Hints items={[['⌫', 'Leave out'], ['⌘↵', 'Export'], ['⌘3', 'Edit'], ['Esc', 'Library']]} />
      </footer>
    </div>
  );
}
