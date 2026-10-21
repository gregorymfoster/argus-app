// Lightroom's Presets panel: Auto first, the Amount of the Preset that's on, the AI's
// suggested order for this Shoot, then every Preset in Lightroom-style groups, as small
// thumbnails of the open Photo. Hovering one previews it on the photo; clicking applies it,
// clicking it again takes it off.

import { Sparkles, Wand2 } from 'lucide-react';
import type { Look, Photo, Recipe } from '../domain/types';
import { PRESETS, PRESET_GROUPS, appliedIn, applyPreset, hasPreset, presetById, slot, type Preset } from '../domain/presets';
import { usePresetRanking } from '../data/presetRanking';
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
  const on = appliedIn(recipe, 'look');
  const why = new Map((ranking?.ranked ?? []).map((r) => [r.id, r.why]));
  const suggested = (ranking?.ranked ?? [])
    .filter((r) => r.score > 0)
    .map((r) => presetById(r.id))
    .filter((p): p is Preset => !!p)
    .slice(0, SUGGESTED);

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
    const title = [hint, p.why, p.local?.length ? `Adds ${p.local.map((l) => l.mask.kind).join(' and ')} Masks` : null].filter(Boolean).join('\n');
    return (
      <button key={p.id} className={`preset ${active ? 'on' : ''}`} aria-pressed={active} onMouseEnter={() => onHover(preview)} onFocus={() => onHover(preview)} onClick={() => pick(p)} data-testid={`preset-${p.id}`} title={title}>
        <PhotoView photo={photo} recipe={preview} look={look} />
        <span>{p.name}</span>
      </button>
    );
  };

  return (
    <div className="dev-panel" onMouseLeave={() => onHover(null)}>
      <div className="dev-title">
        <span>Presets</span>
        <span className="grow" />
        <span className="dev-note">Hover to preview</span>
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
      </Section>
      {PRESET_GROUPS.map((g) => (
        <Section key={g} id={`presets-${g}`} title={g} closed={g !== 'Essentials'}>
          <div className="preset-grid">{PRESETS.filter((p) => p.group === g).map((p) => tile(p))}</div>
        </Section>
      ))}
    </div>
  );
}
