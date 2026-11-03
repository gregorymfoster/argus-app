// Apply look to Shoot (Greg, 2026-11-01; canvas boards AL-Offer, AL-Popover, AL-Applied): edit one
// Photo, then one popover shows the rest of the Shoot as it will look, each frame matched to its
// own light (engine `shoot_look_preview`, redrawn as the person drags). Untick frames to leave
// them out; Apply writes the match on the rest as one Undo (tool `apply_shoot_look`). Frames
// edited by hand are never touched, and every crop and spot stays.
//
// It replaces Apply to rest, the Shoot look strip (W) and Lock look (⇧L), which were three
// overlapping ways to spread a look, none of which applied anything when pressed.

import { useMemo, useState, type ReactNode } from 'react';
import { Sparkles } from 'lucide-react';
import { PHOTOS, SHOOT } from '../data/dummy';
import { LIVE, aiError } from '../data/live';
import { useShootLook } from '../data/shootLook';
import { picksOf } from '../data/modes';
import type { Photo, Recipe } from '../domain/types';
import { useLibrary, verdictOf } from '../store/library';
import { changePhotos } from '../store/changePhotos';
import { PhotoView } from './PhotoView';
import { Kbd } from './kit';
import { ApplyBody, ApplyPopover, type ApplyTile, type Scope } from './ApplyPopover';
import { Button } from '@/components/ui/button';

export type { Scope };

const handEdited = (versions: { by: string }[] | undefined) => versions?.at(-1)?.by === 'you';

/** The Shoot's frames for `scope`, in capture order, the hero first. */
function useFrames(hero: Photo, scope: Scope): { ids: string[]; picks: number; all: number } {
  const { state } = useLibrary();
  return useMemo(() => {
    const live = PHOTOS.slice(0, state.imported).filter((p) => verdictOf(state, p.id) !== 'reject');
    const picks = picksOf(state);
    const list = (scope === 'picks' && picks.length ? picks : live).slice().sort((a, b) => (a.meta?.captured ?? '').localeCompare(b.meta?.captured ?? ''));
    const ids = [hero.id, ...list.map((p) => p.id).filter((id) => id !== hero.id)];
    return { ids, picks: picks.filter((p) => p.id !== hero.id).length, all: live.filter((p) => p.id !== hero.id).length };
    // Only flags and new Photos change which frames show.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hero.id, scope, state.imported, PHOTOS.length, JSON.stringify(state.aiVerdicts), JSON.stringify(state.overrides)]);
}

/**
 * The header button and its popover. `open` is controlled so the Edit panel's offer can open it
 * too; `before` is Edit's held \ (every tile shows its before).
 */
export function ApplyLook({ hero, recipe, open, onOpenChange, before, onApplied, children }: { hero: Photo; recipe: Recipe; open: boolean; onOpenChange: (on: boolean) => void; before: boolean; onApplied: () => void; children: ReactNode }) {
  return (
    <ApplyPopover open={open} onOpenChange={onOpenChange} testId="apply-look" body={() => <Body hero={hero} recipe={recipe} before={before} onClose={() => onOpenChange(false)} onApplied={onApplied} />}>
      {children}
    </ApplyPopover>
  );
}

function Body({ hero, recipe, before, onClose, onApplied }: { hero: Photo; recipe: Recipe; before: boolean; onClose: () => void; onApplied: () => void }) {
  const { state, dispatch } = useLibrary();
  const [scope, setScope] = useState<Scope>('picks');
  const { ids, picks, all } = useFrames(hero, scope);
  const look = useShootLook(hero.id, recipe, ids, true);
  // Frames edited by hand are always kept as they are.
  const yours = (id: string) => id !== hero.id && (look.frames[id]?.anchor || handEdited(state.versions[id]));
  const fileOf = (id: string) => PHOTOS.find((p) => p.id === id)?.file ?? id;
  const undrawn = ids.filter((id) => look.gaveUp.includes(id) || look.frames[id]?.error).length;

  const apply = (targets: string[]) => {
    const n = targets.length;
    const checks = targets.filter((id) => look.frames[id]?.check).length;
    const label = `Apply look to ${n} Photo${n === 1 ? '' : 's'}`;
    const done = (n: number, skipped: { photo_id: number; why: string }[] = []) => {
      const left = skipped.filter((s) => !/edited by hand/.test(s.why));
      const leftNote = left.length ? `. ${left.length} left as they were (${fileOf(String(left[0].photo_id))}: ${left[0].why})` : '';
      dispatch({ type: 'toast', text: `Look applied to ${n} Photo${n === 1 ? '' : 's'}${leftNote}${checks ? `. ${checks} have light unlike ${hero.file}: worth a look` : ''}. ⌘Z undoes it.` });
      onApplied();
      onClose();
    };
    return changePhotos(dispatch, {
      label,
      tool: 'apply_shoot_look',
      args: { photo_id: Number(hero.id), photo_ids: targets.map(Number) },
      written: (r) => (r.applied as number[]) ?? [],
      local: () => targets.map((id) => ({ id, recipe: { ...recipe, crop: state.recipes[id]?.crop ?? recipe.crop } })),
      by: 'ai',
    })
      .then(({ changed, reply }) => done(LIVE ? changed : n, (reply.skipped as { photo_id: number; why: string }[]) ?? []))
      .catch((e) => dispatch({ type: 'toast', text: `Couldn't apply the look: ${aiError(e)}` }));
  };

  const tiles: ApplyTile[] = ids.flatMap((id) => {
    const p = PHOTOS.find((x) => x.id === id);
    if (!p) return [];
    const f = look.frames[id];
    const isHero = id === hero.id;
    const mine = yours(id);
    const notDrawn = look.gaveUp.includes(id) || !!f?.error;
    return [
      {
        id,
        image: f?.image && !before && !mine ? f.image : undefined,
        fallback: <PhotoView photo={p} size="tile" recipe={isHero ? recipe : undefined} edits={!before} original={before} style={{ width: '100%', height: '100%', borderRadius: 0 }} />,
        lock: isHero ? ('source' as const) : mine ? ('kept' as const) : undefined,
        badge: isHero ? 'Your edit' : mine ? 'edited by you, kept' : notDrawn ? <span title={f?.error ?? 'Took too long to draw'}>not drawn</span> : f?.check ? 'check light' : undefined,
        warn: !isHero && !mine && (notDrawn || !!f?.check),
        says: isHero ? 'your edit' : mine ? 'edited by you, kept' : undefined,
      },
    ];
  });
  const checks = tiles.filter((t) => !t.lock && look.frames[t.id]?.check).length;

  return (
    <ApplyBody
      testId="apply-look"
      title="Apply this look to the Shoot"
      context={SHOOT.name}
      scope={{ value: scope, onChange: setScope, picks, all }}
      note={
        <>
          Matched to each photo's own light, from <span className="mono">{hero.file}</span>. Quick fixes find faces and sky again in each. Click a photo to leave it out.
        </>
      }
      reading={look.reading ? { label: look.slow ? `Still on ${fileOf(look.slow.ids[0])} · ${look.slow.seconds} s` : 'Reading light', done: look.reading.done, total: look.reading.total, eta: look.slow ? null : look.reading.eta } : null}
      idle={
        <span className="faint">
          <Kbd quiet>{'\\'}</Kbd> hold for before
        </span>
      }
      tiles={tiles}
      foot={(n) => (
        <>
          {n} photo{n === 1 ? '' : 's'} change. Photos you edited, crops and brushes stay as they are. One Undo.
          {checks > 0 && (
            <span className="look-check-count" style={{ marginLeft: 8 }}>
              <Sparkles size={9} /> {checks} to check
            </span>
          )}
        </>
      )}
      footNote={
        look.error ? (
          <span className="faint trunc" title={look.error}>Couldn't match: {look.error}</span>
        ) : undrawn > 0 ? (
          <span className="faint trunc">{undrawn} couldn't be drawn; Apply still tries them</span>
        ) : undefined
      }
      onApply={apply}
      onClose={onClose}
    />
  );
}

/**
 * The quiet offer in the Edit panel after a hand edit (board AL-Offer): never pops over the
 * photo. Not now hides it until the next edit of this Photo.
 */
export function ApplyLookOffer({ count, onOpen, onDismiss }: { count: number; onOpen: () => void; onDismiss: () => void }) {
  return (
    <section className="apply-offer" data-testid="apply-look-offer">
      <div className="apply-offer-title">
        <Sparkles size={12} className="acc" /> Use this look on the rest of the Shoot?
      </div>
      <div className="faint">
        {count} more Photo{count === 1 ? '' : 's'}. Each keeps its own light; your style carries over.
      </div>
      <div className="row" style={{ gap: 6 }}>
        <Button size="sm" onClick={onOpen} data-testid="apply-look-offer-open">
          Preview <Kbd>⇧⌘A</Kbd>
        </Button>
        <Button size="sm" variant="ghost" onClick={onDismiss}>
          Not now
        </Button>
      </div>
    </section>
  );
}

/** How many other frames the offer would reach: the Picks if any, else everything but Rejects. */
export function useOfferCount(hero: Photo): number {
  const { ids } = useFrames(hero, 'picks');
  return ids.length - 1;
}
