// The Shoot look under the hero in Edit (canvas board SL-Look): the whole Shoot as it will look,
// each frame matched to its own light, redrawn as the person drags. Click a frame to make it
// the hero; Lock look (⇧L) readies a Shoot match on every frame, to accept in the grid.

import { useEffect, useMemo, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Lock, Sparkles } from 'lucide-react';
import { toast } from 'sonner';
import { PHOTOS } from '../data/dummy';
import { LIVE } from '../data/live';
import { lockShootLook, suggestHeroes, useShootLook } from '../data/shootLook';
import type { Photo, Recipe } from '../domain/types';
import { useLibrary, verdictOf } from '../store/library';
import { go } from '../router';
import { PhotoView } from './PhotoView';
import { ProgressBar } from './Progress';
import { Tip } from './kit';
import { Button } from '@/components/ui/button';

/** At most this many frames are drawn, spread evenly over the Shoot (the hero always). */
const SHOWN = 30;

/** Every `step`th frame of `all` in capture order, so the strip spans the whole Shoot. */
function spread(all: string[], keep: string[]): string[] {
  if (all.length <= SHOWN) return all;
  const pick = new Set(keep.filter((id) => all.includes(id)));
  for (let i = 0; pick.size < SHOWN && i < SHOWN; i++) pick.add(all[Math.floor(((i + 0.5) * all.length) / SHOWN)]);
  return all.filter((id) => pick.has(id));
}

export function ShootLookStrip({ hero, recipe }: { hero: Photo; recipe: Recipe }) {
  const { state } = useLibrary();
  const [heroes, setHeroes] = useState<string[]>([]);
  const all = useMemo(
    () =>
      PHOTOS.slice(0, state.imported)
        .filter((p) => verdictOf(state, p.id) !== 'reject')
        .sort((a, b) => (a.meta?.captured ?? '').localeCompare(b.meta?.captured ?? ''))
        .map((p) => p.id),
    // Only Rejects and new Photos change which frames show.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [state.imported, PHOTOS.length, JSON.stringify(state.aiVerdicts), JSON.stringify(state.overrides)],
  );
  const ids = useMemo(() => spread(all, [hero.id, ...heroes]), [all, hero.id, heroes]);
  const look = useShootLook(hero.id, recipe, ids, true);
  const checks = ids.filter((id) => look.frames[id]?.check).length;

  // Which frames to edit next, once every shown frame has been read (it measures the Shoot).
  const read = look.reading == null && Object.keys(look.frames).length > 0;
  useEffect(() => {
    if (!read) return;
    suggestHeroes(3)
      .then(setHeroes)
      .catch(() => {});
  }, [read, look.anchors.join(',')]);
  const next = heroes.filter((id) => !look.anchors.includes(id) && id !== hero.id);

  const lock = () => {
    if (!LIVE) return toast('Look locked (demo)', { id: 'argus' });
    void lockShootLook()
      .then(() => {
        toast(`Look locked for ${all.length} Photos`, { id: 'argus', description: 'A Shoot match is on its way to every frame. Accept all in the grid, as one Undo.' });
        go({ name: 'cull' });
      })
      .catch((e) => toast(String(e).replace(/^Error: /, ''), { id: 'argus' }));
  };
  useHotkeys('shift+l', lock, { preventDefault: true }, [all.length]);

  return (
    <section className="look-strip" aria-label="Whole Shoot, as it will look" data-testid="shoot-look">
      <div className="look-strip-head">
        <span className="look-strip-title">Whole Shoot, as it will look</span>
        <span className="faint">
          updates as you drag · {ids.length < all.length ? `${ids.length} of ${all.length} shown` : `${all.length} Photos`}, Rejects hidden
        </span>
        {checks > 0 && (
          <Tip label="Their light is unlike every frame you edited. Look at them, or edit one by hand.">
            <span className="look-check-count" data-testid="shoot-look-checks">{checks} to check</span>
          </Tip>
        )}
        <span className="grow" />
        {look.reading ? (
          <span className="look-strip-bar">
            <ProgressBar label="Reading each frame's light" done={look.reading.done} total={look.reading.total} eta={look.reading.eta} testId="shoot-look-reading" />
          </span>
        ) : (
          look.ms != null && <span className="faint mono" data-testid="shoot-look-ms">redrawn in {look.ms < 1000 ? `${look.ms} ms` : `${(look.ms / 1000).toFixed(1)} s`}</span>
        )}
        {look.error && <span className="faint" title={look.error}>Couldn't match: {look.error}</span>}
        <Tip label="A Shoot match on every frame, ready to accept in the grid" keys="⇧L">
          <Button variant="default" size="sm" onClick={lock} data-testid="shoot-look-lock">
            <Lock size={11} /> Lock look
          </Button>
        </Tip>
      </div>
      <div className="look-strip-grid">
        {ids.map((id) => {
          const p = PHOTOS.find((x) => x.id === id);
          if (!p) return null;
          const f = look.frames[id];
          const isHero = id === hero.id;
          return (
            <button key={id} className={`look-tile${isHero ? ' hero' : ''}`} onClick={() => !isHero && go({ name: 'edit', id })} aria-label={`${p.file}${isHero ? ', the hero' : ''}`} data-testid="shoot-look-tile" data-drawn={f?.image ? 'yes' : undefined}>
              {f?.image ? <img src={f.image} alt="" draggable={false} /> : <PhotoView photo={p} size="tile" recipe={isHero ? recipe : undefined} edits={!isHero} style={{ width: '100%', height: '100%', borderRadius: 0 }} />}
              {isHero ? (
                <span className="look-badge hero">Hero</span>
              ) : f?.anchor ? (
                <span className="look-badge">edited</span>
              ) : f?.check ? (
                <span className="look-badge check">check</span>
              ) : next.includes(id) ? (
                <span className="look-badge next">
                  <Sparkles size={9} /> edit next
                </span>
              ) : null}
            </button>
          );
        })}
      </div>
    </section>
  );
}
