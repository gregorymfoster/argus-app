// Crop & straighten (Greg, 2026-11-01): the AI proposes a straighten and crop for the Shoot's
// frames, and one popover, laid out as Apply look's (boards AL-Popover, AL-Applied), shows each
// frame as it would be. Untick to leave a frame out; Apply writes the rest as one Undo (tool
// `apply_framing`), changing only the crop. Frames already cropped, by hand or by an earlier
// pass, are never touched. Only frames with something to fix show: a straighten where there is
// a horizon or architecture to level by, a punch-in where the people sit loose.

import { useMemo, useState, type ReactNode } from 'react';
import { PHOTOS, SHOOT } from '../data/dummy';
import { LIVE, aiError } from '../data/live';
import { useFraming } from '../data/framing';
import { picksOf } from '../data/modes';
import type { Photo } from '../domain/types';
import { useLibrary, verdictOf } from '../store/library';
import { changePhotos } from '../store/changePhotos';
import { PhotoView } from './PhotoView';
import { Kbd, Seg } from './kit';
import { ApplyBody, ApplyPopover, type ApplyTile, type Scope } from './ApplyPopover';

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
    <ApplyPopover open={open} onOpenChange={onOpenChange} testId="framing" body={() => <Body before={before} onClose={() => onOpenChange(false)} />}>
      {children}
    </ApplyPopover>
  );
}

function Body({ before, onClose }: { before: boolean; onClose: () => void }) {
  const { state, dispatch } = useLibrary();
  const [scope, setScope] = useState<Scope>('picks');
  const { ids, picks, all } = useFrames(scope);
  // Straighten only by default: Greg rarely crops, and on the bench the AI's crops weren't his.
  const [withCrop, setWithCrop] = useState(false);
  const framing = useFraming(ids, withCrop, true);
  const proposed = ids.filter((id) => framing.frames[id]?.crop);
  const handCropped = ids.filter((id) => framing.frames[id]?.kept === 'already cropped').length;

  const apply = (targets: string[]) => {
    const n = targets.length;
    const label = `${withCrop ? 'Crop & straighten' : 'Straighten'} ${n} Photo${n === 1 ? '' : 's'}`;
    const done = (n: number) => {
      dispatch({ type: 'toast', text: `${withCrop ? 'Cropped and straightened' : 'Straightened'} ${n} Photo${n === 1 ? '' : 's'}. ⌘Z undoes it.` });
      onClose();
    };
    return changePhotos(dispatch, {
      label,
      tool: 'apply_framing',
      args: { photo_ids: targets.map(Number), crop: withCrop },
      written: (r) => (r.applied as number[]) ?? [],
      local: () => targets.map((id) => ({ id, recipe: { ...state.recipes[id], crop: framing.frames[id].crop ?? undefined } })),
      by: 'ai',
    })
      .then(({ changed }) => done(LIVE ? changed || n : n))
      .catch((e) => dispatch({ type: 'toast', text: `Couldn't crop: ${aiError(e)}` }));
  };

  const tiles: ApplyTile[] = proposed.flatMap((id) => {
    const p = PHOTOS.find((x) => x.id === id);
    const f = framing.frames[id];
    if (!p || !f?.crop) return [];
    return [{ id, image: f.image && !before ? f.image : undefined, fallback: <Tile photo={p} crop={before ? undefined : f.crop} />, badge: f.reasons.join(' · '), says: f.reasons.join(', ') }];
  });

  return (
    <ApplyBody
      testId="framing"
      title="Crop & straighten"
      context={SHOOT.name}
      controls={
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
      }
      scope={{ value: scope, onChange: setScope, picks, all }}
      note={<>{withCrop ? 'Frames with a crooked horizon or wall, or people sitting loose.' : 'Frames with a crooked horizon or wall.'} Click a photo to leave it out.</>}
      reading={framing.reading ? { label: 'Looking at each frame', ...framing.reading } : null}
      idle={
        <span className="faint">
          <Kbd quiet>{'\\'}</Kbd> hold for as shot
        </span>
      }
      tiles={tiles}
      empty={<span className="faint" data-testid="framing-none">{withCrop ? 'Nothing to straighten or crop: every frame looks level and well framed.' : 'Nothing to straighten: no crooked horizon or wall found.'}</span>}
      foot={(n) => (
        <>
          {n} photo{n === 1 ? '' : 's'} change, only their crop. {ids.length - proposed.length} left as shot{handCropped ? `, ${handCropped} already cropped` : ''}. One Undo.
        </>
      )}
      footNote={framing.error ? <span className="faint trunc" title={framing.error}>Couldn't look: {framing.error}</span> : undefined}
      wait={!!framing.reading}
      onApply={apply}
      onClose={onClose}
    />
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
