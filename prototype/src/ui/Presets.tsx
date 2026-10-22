// Lightroom's Presets panel: Auto first, the Amount of the Preset that's on, the AI's
// suggested order for this Shoot, then every Preset in Lightroom-style groups, as small
// thumbnails of the open Photo. Hovering one previews it on the photo; clicking applies it,
// clicking it again takes it off. The person's own Presets (Yours) come first; + or ⇧⌘N
// saves the open Photo's settings as one, and right-click renames, updates or deletes it.

import { Pencil, Plus, RefreshCw, Sparkles, Trash2, Wand2 } from 'lucide-react';
import type { Look, Photo, Recipe } from '../domain/types';
import { PRESETS, PRESET_GROUPS, YOURS, appliedIn, applyPreset, hasPreset, presetById, recommended, slot, type Preset } from '../domain/presets';
import { usePresetRanking } from '../data/presetRanking';
import { deletePreset, useYourPresets } from '../data/yourPresets';
import { GROUPS } from '../domain/settings';
import { useLibrary } from '../store/library';
import { askPreset } from './CreatePreset';
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from '@/components/ui/menu';
import { LIVE, aiName } from '../data/live';
import { PhotoView } from './PhotoView';
import { Section } from './Develop';
import { Kbd, Slider, Tip } from './kit';
import { ProgressBar } from './Progress';
import { Button } from '@/components/ui/button';

/** How many Presets the Suggested row shows. */
const SUGGESTED = 6;

interface Props {
  photo: Photo;
  recipe: Recipe;
  look: Look | null;
  onHover: (r: Recipe | null) => void;
  /** A new Recipe from a Preset; `key` merges a drag of the Amount into one History step. */
  onSet: (r: Recipe, label: string, key?: string) => void;
  onAuto: () => void;
}

export function PresetsPanel({ photo, recipe, look, onHover, onSet, onAuto }: Props) {
  const { ranking, job, error, rankNow } = usePresetRanking(photo);
  const yours = useYourPresets();
  const { dispatch } = useLibrary();
  const on = appliedIn(recipe, 'look');
  const why = new Map((ranking?.ranked ?? []).map((r) => [r.id, r.why]));
  const ranked = (ranking?.ranked ?? [])
    .filter((r) => r.score > 0)
    .map((r) => presetById(r.id))
    .filter((p): p is Preset => !!p)
    .slice(0, SUGGESTED);
  // Until the AI has ranked the Shoot, the Looks that suit this photo (what Edit's Recommended row
  // showed before the takes took its place, Greg 2026-10-22).
  const suggested = ranked.length ? ranked : recommended(photo, null);

  const pick = (p: Preset) => {
    onHover(null);
    if (hasPreset(recipe, p)) return onSet(applyPreset(recipe, p, 0), `Removed ${p.name}`);
    // A Look-style Preset keeps the Amount of the one it replaces, as Lightroom's does.
    const amount = slot(p) === 'look' && on ? on.amount : 1;
    onSet(applyPreset(recipe, p, amount), `Preset: ${p.name}`);
  };
  const tile = (p: Preset, hint?: string) => {
    const active = hasPreset(recipe, p);
    const preview = active ? applyPreset(recipe, p, 0) : applyPreset(recipe, p, slot(p) === 'look' && on ? on.amount : 1);
    const title = [hint, p.why, p.settings ? `Your Preset: ${p.settings.map((g) => GROUPS.find((x) => x.id === g)?.label ?? g).join(', ')}` : null, p.local?.length ? `Adds ${p.local.map((l) => l.mask.kind).join(' and ')} Masks` : null].filter(Boolean).join('\n');
    return (
      <button key={p.id} className={`preset ${active ? 'on' : ''}`} aria-pressed={active} onMouseEnter={() => onHover(preview)} onFocus={() => onHover(preview)} onClick={() => pick(p)} data-testid={`preset-${p.id}`} title={title}>
        <PhotoView photo={photo} recipe={preview} look={look} />
        <span>{p.name}</span>
      </button>
    );
  };

  const remove = (p: Preset) =>
    deletePreset(p.id)
      .then(() => dispatch({ type: 'toast', text: `Deleted the Preset ${p.name}. Photos keep its settings.` }))
      .catch((e: Error) => dispatch({ type: 'toast', text: e.message }));
  const mine = (p: Preset) => (
    <ContextMenu key={p.id}>
      <ContextMenuTrigger asChild>{tile(p)}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={() => askPreset({ mode: 'update', photo: photo.id, recipe, preset: p })}>
          <RefreshCw size={14} /> Update with this Photo’s settings
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => askPreset({ mode: 'rename', photo: photo.id, recipe, preset: p })}>
          <Pencil size={14} /> Rename
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => void remove(p)} data-testid={`preset-delete-${p.id}`}>
          <Trash2 size={14} /> Delete
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );

  return (
    <div className="dev-panel" onMouseLeave={() => onHover(null)}>
      <div className="dev-title">
        <span>Presets</span>
        <span className="grow" />
        <span className="dev-note">Hover to preview</span>
        <Tip label="Create Preset from this Photo" keys="⇧⌘N">
          <Button variant="ghost" size="icon" className="size-6" aria-label="Create Preset" onClick={() => askPreset({ mode: 'create', photo: photo.id, recipe })} data-testid="preset-create">
            <Plus size={13} />
          </Button>
        </Tip>
      </div>
      <button className="preset-auto" onClick={onAuto} onMouseEnter={() => onHover(null)} data-testid="preset-auto">
        <Wand2 size={13} /> Auto
        <Kbd quiet>⇧A</Kbd>
      </button>
      <div className={on ? '' : 'dim'} data-testid="preset-amount">
        <Slider
          label={on ? on.preset.name : 'Amount'}
          name="Preset amount"
          min={0}
          max={200}
          def={100}
          value={Math.round((on?.amount ?? 1) * 100)}
          display={`${Math.round((on?.amount ?? 1) * 100)}`}
          onChange={(v) => on && onSet(applyPreset(recipe, on.preset, Math.max(0.01, v / 100)), `${on.preset.name} amount`, 'preset-amount')}
        />
      </div>
      {yours.length > 0 && (
        <Section id="presets-yours" title={YOURS}>
          <div className="preset-grid" data-testid="presets-yours">
            {yours.map(mine)}
          </div>
        </Section>
      )}
      <Section
        id="presets-suggested"
        title="Suggested for this Shoot"
        aside={
          job ? undefined : (
            <span className="flex items-center gap-1">
              {ranking && (
                <Tip label={`${aiName()} ranked these on ${ranking.photos.length || 'a few'} Photos of this Shoot. A suggestion, not a verdict.`}>
                  <Sparkles size={12} className="acc" />
                </Tip>
              )}
              {LIVE && (
                <Button variant="ghost" size="sm" onClick={() => void rankNow()} data-testid="preset-rank-now">
                  {ranking ? 'Rank again' : 'Rank now'}
                </Button>
              )}
            </span>
          )
        }
      >
        {job ? (
          <ProgressBar label={job.status === 'running' ? `${aiName()} is ranking Presets` : 'Ranking waits in the Queue'} done={job.done} total={job.total || 3} testId="preset-rank-progress" />
        ) : null}
        {suggested.length ? (
          <div className="preset-grid" data-testid="presets-suggested">
            {suggested.map((p) => tile(p, why.get(p.id) ? `${aiName()}: ${why.get(p.id)}` : undefined))}
          </div>
        ) : !job ? (
          <div className="dev-note">{error ? `Ranking failed: ${error}` : `${aiName()} hasn't ranked the Presets for this Shoot yet.`}</div>
        ) : null}
        {!ranked.length && !job && <div className="dev-note">Picked for this photo until {aiName()} ranks the Shoot.</div>}
      </Section>
      {PRESET_GROUPS.map((g) => (
        <Section key={g} id={`presets-${g}`} title={g} closed={g !== 'Essentials'}>
          <div className="preset-grid">{PRESETS.filter((p) => p.group === g).map((p) => tile(p))}</div>
        </Section>
      ))}
    </div>
  );
}
