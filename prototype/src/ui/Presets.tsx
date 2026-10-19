// Lightroom's Presets panel: Auto first, then built-in Looks and adaptive presets as small
// thumbnails of the open Photo. Hovering one previews it on the photo; clicking applies it.

import { Wand2 } from 'lucide-react';
import type { Look, Photo, Recipe } from '../domain/types';
import { PRESETS, PRESET_GROUPS, applyPreset, hasPreset, type Preset } from '../domain/presets';
import { PhotoView } from './PhotoView';
import { Section } from './Develop';
import { Kbd } from './kit';

interface Props {
  photo: Photo;
  recipe: Recipe;
  look: Look | null;
  onHover: (r: Recipe | null) => void;
  onApply: (p: Preset) => void;
  onAuto: () => void;
}

export function PresetsPanel({ photo, recipe, look, onHover, onApply, onAuto }: Props) {
  return (
    <div className="dev-panel" onMouseLeave={() => onHover(null)}>
      <div className="dev-title">
        <span>Presets</span>
        <span className="grow" />
        <span className="dev-note">Hover to preview</span>
      </div>
      <button className="preset-auto" onClick={onAuto} onMouseEnter={() => onHover(null)} data-testid="preset-auto">
        <Wand2 size={13} /> Auto
        <Kbd>⇧A</Kbd>
      </button>
      {PRESET_GROUPS.map((g) => (
        <Section key={g} id={`presets-${g}`} title={g}>
          <div className="preset-grid">
            {PRESETS.filter((p) => p.group === g).map((p) => {
              const r = applyPreset(recipe, p);
              return (
                <button key={p.id} className={`preset ${hasPreset(recipe, p) ? 'on' : ''}`} onMouseEnter={() => onHover(r)} onFocus={() => onHover(r)} onClick={() => onApply(p)} data-testid={`preset-${p.id}`} title={p.local ? `Adds a ${p.local.mask.kind} Mask` : undefined}>
                  <PhotoView photo={photo} recipe={r} look={look} />
                  <span>{p.name}</span>
                </button>
              );
            })}
          </div>
        </Section>
      ))}
    </div>
  );
}
