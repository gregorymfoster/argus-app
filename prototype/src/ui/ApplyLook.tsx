// Apply look to Shoot (Greg, 2026-11-01; canvas boards AL-Offer, AL-Popover, AL-Applied): edit one
// Photo, then one popover shows the rest of the Shoot as it will look, each frame matched to its
// own light (engine `shoot_look_preview`, redrawn as the person drags). Untick frames to leave
// them out; Apply writes the match on the rest as one Undo (tool `apply_shoot_look`). Frames
// edited by hand are never touched, and every crop and spot stays.
//
// It replaces Apply to rest, the Shoot look strip (W) and Lock look (⇧L), which were three
// overlapping ways to spread a look, none of which applied anything when pressed.

import { useMemo, useState, type ReactNode } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Check, Sparkles } from 'lucide-react';
import { PHOTOS, SHOOT } from '../data/dummy';
import { LIVE, asPerson } from '../data/live';
import { useShootLook } from '../data/shootLook';
import { picksOf } from '../data/modes';
import type { Photo, Recipe } from '../domain/types';
import { useLibrary, verdictOf } from '../store/library';
import { PhotoView } from './PhotoView';
import { ProgressBar } from './Progress';
import { Kbd, Seg } from './kit';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Button } from '@/components/ui/button';

export type Scope = 'picks' | 'all';

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
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="end" className="apply-look w-[min(880px,calc(100vw-48px))] p-0" onOpenAutoFocus={(e) => e.preventDefault()} data-testid="apply-look">
        {open && <Body hero={hero} recipe={recipe} before={before} onClose={() => onOpenChange(false)} onApplied={onApplied} />}
      </PopoverContent>
    </Popover>
  );
}

function Body({ hero, recipe, before, onClose, onApplied }: { hero: Photo; recipe: Recipe; before: boolean; onClose: () => void; onApplied: () => void }) {
  const { state, dispatch } = useLibrary();
  const [scope, setScope] = useState<Scope>('picks');
  const { ids, picks, all } = useFrames(hero, scope);
  const look = useShootLook(hero.id, recipe, ids, true);
  // Left out by the person; frames edited by hand are always out.
  const [off, setOff] = useState<Set<string>>(new Set());
  const yours = (id: string) => id !== hero.id && (look.frames[id]?.anchor || handEdited(state.versions[id]));
  const targets = ids.filter((id) => id !== hero.id && !yours(id) && !off.has(id));
  const checks = targets.filter((id) => look.frames[id]?.check).length;
  const [busy, setBusy] = useState(false);

  const apply = () => {
    if (!targets.length || busy) return;
    const n = targets.length;
    const label = `Apply look to ${n} Photo${n === 1 ? '' : 's'}`;
    if (!LIVE) {
      dispatch({ type: 'batch', label, actions: targets.map((id) => ({ type: 'recipe/set' as const, id, recipe: { ...recipe, crop: state.recipes[id]?.crop ?? recipe.crop }, label, by: 'ai' as const })) });
      done(n);
      return;
    }
    setBusy(true);
    const w = asPerson('apply_shoot_look', { photo_id: Number(hero.id), photo_ids: targets.map(Number) }, label, (r) => (r.applied as number[]) ?? []);
    dispatch({ type: 'engine/saved', label, batch: w });
    w.then((r) => done(Object.keys(r.recipes ?? {}).length || n))
      .catch((e) => dispatch({ type: 'toast', text: `Couldn't apply: ${String(e).replace(/^Error: /, '')}` }))
      .finally(() => setBusy(false));
  };
  const done = (n: number) => {
    dispatch({ type: 'toast', text: `Look applied to ${n} Photo${n === 1 ? '' : 's'}${checks ? `. ${checks} have light unlike ${hero.file}: worth a look` : ''}. ⌘Z undoes it.` });
    onApplied();
    onClose();
  };
  useHotkeys('enter', (e) => (e.preventDefault(), apply()), { enableOnFormTags: false }, [targets.join(','), busy]);

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
        <span className="apply-look-title">Apply this look to the Shoot</span>
        <span className="faint mono trunc">{SHOOT.name}</span>
        <span className="grow" />
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
        <span>
          Matched to each photo's own light, from <span className="mono">{hero.file}</span>. Quick fixes find faces and sky again in each. Click a photo to leave it out.
        </span>
        <span className="grow" />
        {look.reading ? (
          <span className="apply-look-bar">
            <ProgressBar label="Reading each frame's light" done={look.reading.done} total={look.reading.total} eta={look.reading.eta} testId="apply-look-reading" />
          </span>
        ) : (
          <span className="faint">
            <Kbd quiet>{'\\'}</Kbd> hold for before
          </span>
        )}
      </div>
      <div className="apply-look-grid">
        {ids.map((id) => {
          const p = PHOTOS.find((x) => x.id === id);
          if (!p) return null;
          const f = look.frames[id];
          const isHero = id === hero.id;
          const mine = yours(id);
          const out = !isHero && (mine || off.has(id));
          return (
            <button key={id} className={`apply-tile${isHero ? ' hero' : ''}${out ? ' off' : ''}`} onClick={() => !isHero && !mine && toggle(id)} disabled={isHero || mine} aria-pressed={!isHero && !mine ? !out : undefined} aria-label={`${p.file}${isHero ? ', your edit' : mine ? ', edited by you, kept' : out ? ', left out' : ''}`} data-testid="apply-look-tile" data-drawn={f?.image ? 'yes' : undefined}>
              <span className="apply-well">
                {f?.image && !before && !mine ? <img src={f.image} alt="" draggable={false} /> : <PhotoView photo={p} size="tile" recipe={isHero ? recipe : undefined} edits={!before} original={before} style={{ width: '100%', height: '100%', borderRadius: 0 }} />}
                {!isHero && !mine && <span className={`apply-tick${out ? '' : ' on'}`}>{!out && <Check size={9} strokeWidth={3} />}</span>}
                {isHero ? <span className="look-badge hero">Your edit</span> : mine ? <span className="look-badge">edited by you, kept</span> : f?.check ? <span className="look-badge check">check light</span> : null}
              </span>
              <span className="apply-cap mono">{p.file}</span>
            </button>
          );
        })}
      </div>
      <div className="apply-look-foot">
        <span className="trunc">
          {targets.length} photo{targets.length === 1 ? '' : 's'} change. Photos you edited, crops and brushes stay as they are. One Undo.
          {checks > 0 && <span className="look-check-count" style={{ marginLeft: 8 }}><Sparkles size={9} /> {checks} to check</span>}
        </span>
        <span className="grow" />
        {look.error && <span className="faint trunc" title={look.error}>Couldn't match: {look.error}</span>}
        <Button variant="ghost" size="sm" onClick={onClose}>
          Cancel <Kbd quiet>esc</Kbd>
        </Button>
        <Button size="sm" onClick={apply} disabled={!targets.length || busy} data-testid="apply-look-go">
          Apply to {targets.length} <Kbd>↵</Kbd>
        </Button>
      </div>
    </div>
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
