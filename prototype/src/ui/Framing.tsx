// Crop & straighten (Greg, 2026-11-01): the AI proposes a straighten and crop for the Shoot's
// frames, and one popover, laid out as Apply look's (boards AL-Popover, AL-Applied), shows each
// frame as it would be. Untick to leave a frame out; Apply writes the rest as one Undo (tool
// `apply_framing`), changing only the crop. Frames already cropped, by hand or by an earlier
// pass, are never touched. Only frames with something to fix show: a straighten where there is
// a horizon or architecture to level by, a punch-in where the people sit loose.

import { useMemo, useState, type ReactNode } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Check } from 'lucide-react';
import { PHOTOS, SHOOT } from '../data/dummy';
import { LIVE, asPerson } from '../data/live';
import { useFraming } from '../data/framing';
import { picksOf } from '../data/modes';
import type { Photo } from '../domain/types';
import { useLibrary, verdictOf } from '../store/library';
import { PhotoView } from './PhotoView';
import { ProgressBar } from './Progress';
import { Kbd, Seg } from './kit';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Button } from '@/components/ui/button';

type Scope = 'picks' | 'all';

/** The Shoot's frames for `scope`, in capture order. */
function useFrames(scope: Scope): { ids: string[]; picks: number; all: number } {
  const { state } = useLibrary();
  return useMemo(() => {
    const live = PHOTOS.slice(0, state.imported).filter((p) => verdictOf(state, p.id) !== 'reject');
    const picks = picksOf(state);
    const list = (scope === 'picks' && picks.length ? picks : live).slice().sort((a, b) => (a.meta?.captured ?? '').localeCompare(b.meta?.captured ?? ''));
    return { ids: list.map((p) => p.id), picks: picks.length, all: live.length };
    // Only flags and new Photos change which frames show.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope, state.imported, PHOTOS.length, JSON.stringify(state.aiVerdicts), JSON.stringify(state.overrides)]);
}

/** The header button and its popover; `before` is Edit's held \ (every tile shows as shot). */
export function Framing({ open, onOpenChange, before, children }: { open: boolean; onOpenChange: (on: boolean) => void; before: boolean; children: ReactNode }) {
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="end" className="apply-look w-[min(880px,calc(100vw-48px))] p-0" onOpenAutoFocus={(e) => e.preventDefault()} data-testid="framing">
        {open && <Body before={before} onClose={() => onOpenChange(false)} />}
      </PopoverContent>
    </Popover>
  );
}

function Body({ before, onClose }: { before: boolean; onClose: () => void }) {
  const { state, dispatch } = useLibrary();
  const [scope, setScope] = useState<Scope>('picks');
  const { ids, picks, all } = useFrames(scope);
  // Straighten only by default: Greg rarely crops, and on the bench the AI's crops weren't his.
  const [withCrop, setWithCrop] = useState(false);
  const framing = useFraming(ids, withCrop, true);
  const [off, setOff] = useState<Set<string>>(new Set());
  const proposed = ids.filter((id) => framing.frames[id]?.crop);
  const handCropped = ids.filter((id) => framing.frames[id]?.kept === 'already cropped').length;
  const targets = proposed.filter((id) => !off.has(id));
  const [busy, setBusy] = useState(false);

  const apply = () => {
    if (!targets.length || busy || framing.reading) return;
    const n = targets.length;
    const label = `${withCrop ? 'Crop & straighten' : 'Straighten'} ${n} Photo${n === 1 ? '' : 's'}`;
    if (!LIVE) {
      dispatch({ type: 'batch', label, actions: targets.map((id) => ({ type: 'recipe/set' as const, id, recipe: { ...state.recipes[id], crop: framing.frames[id].crop ?? undefined }, label, by: 'ai' as const })) });
      done(n);
      return;
    }
    setBusy(true);
    const w = asPerson('apply_framing', { photo_ids: targets.map(Number), crop: withCrop }, label, (r) => (r.applied as number[]) ?? []);
    dispatch({ type: 'engine/saved', label, batch: w });
    w.then((r) => done(Object.keys(r.recipes ?? {}).length || n))
      .catch((e) => dispatch({ type: 'toast', text: `Couldn't crop: ${String(e).replace(/^Error: /, '')}` }))
      .finally(() => setBusy(false));
  };
  const done = (n: number) => {
    dispatch({ type: 'toast', text: `${withCrop ? 'Cropped and straightened' : 'Straightened'} ${n} Photo${n === 1 ? '' : 's'}. ⌘Z undoes it.` });
    onClose();
  };
  useHotkeys('enter', (e) => (e.preventDefault(), apply()), { enableOnFormTags: false }, [targets.join(','), busy, !!framing.reading]);

  const toggle = (id: string) =>
    setOff((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  return (
    <div className="apply-look-body">
      <div className="apply-look-head">
        <span className="apply-look-title">Crop &amp; straighten</span>
        <span className="faint mono trunc">{SHOOT.name}</span>
        <span className="grow" />
        <Seg
          quiet
          label="What to fix"
          value={withCrop ? 'crop' : 'level'}
          onChange={(v) => setWithCrop(v === 'crop')}
          items={[
            { value: 'level', label: 'Straighten' },
            { value: 'crop', label: 'Straighten + crop' },
          ]}
        />
        {picks > 0 && (
          <Seg
            quiet
            label="Which Photos"
            value={scope}
            onChange={setScope}
            items={[
              { value: 'picks', label: <>Picks <span className="mono faint">{picks}</span></> },
              { value: 'all', label: <>All but Rejects <span className="mono faint">{all}</span></> },
            ]}
          />
        )}
      </div>
      <div className="apply-look-note">
        <span>{withCrop ? 'Frames with a crooked horizon or wall, or people sitting loose.' : 'Frames with a crooked horizon or wall.'} Click a photo to leave it out.</span>
        <span className="grow" />
        {framing.reading ? (
          <span className="apply-look-bar">
            <ProgressBar label="Looking at each frame" done={framing.reading.done} total={framing.reading.total} eta={framing.reading.eta} testId="framing-reading" />
          </span>
        ) : (
          <span className="faint">
            <Kbd quiet>{'\\'}</Kbd> hold for as shot
          </span>
        )}
      </div>
      <div className="apply-look-grid">
        {proposed.map((id) => {
          const p = PHOTOS.find((x) => x.id === id);
          const f = framing.frames[id];
          if (!p || !f?.crop) return null;
          const out = off.has(id);
          return (
            <button key={id} className={`apply-tile${out ? ' off' : ''}`} onClick={() => toggle(id)} aria-pressed={!out} aria-label={`${p.file}, ${f.reasons.join(', ')}${out ? ', left out' : ''}`} data-testid="framing-tile" data-drawn={f.image ? 'yes' : undefined}>
              <span className="apply-well">
                {f.image && !before ? <img src={f.image} alt="" draggable={false} /> : <Tile photo={p} crop={before ? undefined : f.crop} />}
                <span className={`apply-tick${out ? '' : ' on'}`}>{!out && <Check size={9} strokeWidth={3} />}</span>
                <span className="look-badge">{f.reasons.join(' · ')}</span>
              </span>
              <span className="apply-cap mono">{p.file}</span>
            </button>
          );
        })}
        {!framing.reading && !proposed.length && <span className="faint" data-testid="framing-none">{withCrop ? 'Nothing to straighten or crop: every frame looks level and well framed.' : 'Nothing to straighten: no crooked horizon or wall found.'}</span>}
      </div>
      <div className="apply-look-foot">
        <span className="trunc">
          {targets.length} photo{targets.length === 1 ? '' : 's'} change, only their crop. {ids.length - proposed.length} left as shot{handCropped ? `, ${handCropped} already cropped` : ''}. One Undo.
        </span>
        <span className="grow" />
        {framing.error && <span className="faint trunc" title={framing.error}>Couldn't look: {framing.error}</span>}
        <Button variant="ghost" size="sm" onClick={onClose}>
          Cancel <Kbd quiet>esc</Kbd>
        </Button>
        <Button size="sm" onClick={apply} disabled={!targets.length || busy || !!framing.reading} data-testid="framing-go">
          Apply to {targets.length} <Kbd>↵</Kbd>
        </Button>
      </div>
    </div>
  );
}

/** The prototype's stand-in for an engine render: the Photo with the crop drawn on it. */
function Tile({ photo, crop }: { photo: Photo; crop?: { angle: number; x: number; y: number; w: number; h: number } }) {
  return (
    <>
      <PhotoView photo={photo} size="tile" style={{ width: '100%', height: '100%', borderRadius: 0, transform: crop?.angle ? `rotate(${crop.angle}deg) scale(1.06)` : undefined }} />
      {crop && (crop.w < 1 || crop.h < 1) && <span className="crop-box" style={{ left: `${crop.x * 100}%`, top: `${crop.y * 100}%`, width: `${crop.w * 100}%`, height: `${crop.h * 100}%` }} />}
    </>
  );
}
