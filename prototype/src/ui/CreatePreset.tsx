// Lightroom's Create Preset: a name and the Settings groups to keep, saved from the open
// Photo into the person's own Presets (group Yours). The same dialog renames one, or updates
// it with the open Photo's settings. ⇧⌘N opens it in Edit, as New Preset does in Lightroom.

import { useEffect, useRef, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { DEFAULT_PRESET_SETTINGS, PRESET_SETTINGS, type Preset } from '../domain/presets';
import { GROUPS } from '../domain/settings';
import type { Recipe } from '../domain/types';
import { renamePreset, savePreset } from '../data/yourPresets';
import { Kbd } from './kit';

type Ask = { mode: 'create' | 'update' | 'rename'; photo: string; recipe: Recipe; preset?: Preset };

/** Open the dialog: a new Preset from `photo`, or `update`/`rename` an existing one. */
export function askPreset(a: Ask) {
  window.dispatchEvent(new CustomEvent('argus:create-preset', { detail: a }));
}

/** ⇧⌘N: Create Preset from the open Photo. */
export function useCreatePresetKey(photo: string, recipe: Recipe) {
  useHotkeys('mod+shift+n', () => askPreset({ mode: 'create', photo, recipe }), { preventDefault: true }, [photo, recipe]);
}

const LABEL: Record<string, string> = Object.fromEntries(GROUPS.map((g) => [g.id, g.label]));
const KEY = 'argus:preset-groups';
function lastGroups(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null') as string[] | null;
    if (Array.isArray(v) && v.length && v.every((g) => (PRESET_SETTINGS as readonly string[]).includes(g))) return v;
  } catch {
    // No storage: the defaults.
  }
  return DEFAULT_PRESET_SETTINGS;
}

/** The Create Preset dialog, mounted once by the app; `onSaved` reports what happened. */
export function CreatePresetDialog({ onSaved }: { onSaved: (text: string) => void }) {
  const [ask, setAsk] = useState<Ask | null>(null);
  const [name, setName] = useState('');
  const [groups, setGroups] = useState<string[]>(DEFAULT_PRESET_SETTINGS);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const on = (e: Event) => {
      const a = (e as CustomEvent<Ask>).detail;
      setAsk(a);
      setName(a.preset?.name ?? '');
      setGroups(a.preset?.settings ?? lastGroups());
      setError(null);
    };
    window.addEventListener('argus:create-preset', on);
    return () => window.removeEventListener('argus:create-preset', on);
  }, []);
  const toggle = (g: string, on: boolean) => setGroups((gs) => (on ? PRESET_SETTINGS.filter((x) => x === g || gs.includes(x)) : gs.filter((x) => x !== g)));
  const rename = ask?.mode === 'rename';
  const ready = !!name.trim() && (rename || groups.length > 0) && !busy;
  const confirm = async () => {
    if (!ask || !ready) return;
    setBusy(true);
    try {
      if (rename) {
        await renamePreset(ask.preset!.id, name);
        onSaved(`Renamed the Preset to ${name.trim()}`);
      } else {
        try {
          localStorage.setItem(KEY, JSON.stringify(groups));
        } catch {
          // Not remembered this time.
        }
        const p = await savePreset(name, ask.photo, ask.recipe, groups, ask.mode === 'update' ? ask.preset?.id : undefined);
        onSaved(ask.mode === 'update' ? `Updated ${p.name} with this Photo’s settings` : `Saved ${p.name} to Yours in Presets`);
      }
      setAsk(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      input.current?.focus();
    } finally {
      setBusy(false);
    }
  };
  const title = !ask ? '' : ask.mode === 'create' ? 'Create Preset' : ask.mode === 'update' ? `Update ${ask.preset?.name}` : `Rename ${ask.preset?.name}`;
  return (
    <Dialog open={!!ask} onOpenChange={(o) => !o && setAsk(null)}>
      <DialogContent
        className="remove-dialog"
        closable={false}
        data-testid="preset-dialog"
        onKeyDown={(e) => {
          if (e.key === 'Enter') (e.preventDefault(), void confirm());
        }}
      >
        <DialogTitle className="remove-title">{title}</DialogTitle>
        {!rename && (
          <DialogDescription className="remove-note">
            {ask?.mode === 'update' ? 'Its ticked groups take this Photo’s settings.' : 'Saves this Photo’s settings in the ticked groups. Applied, they replace the same settings on a Photo; the rest of the Photo stays.'}
          </DialogDescription>
        )}
        <input ref={input} className="preset-name" autoFocus value={name} placeholder="Preset name" aria-label="Preset name" onChange={(e) => (setName(e.target.value), setError(null))} data-testid="preset-name" />
        {error && <div className="preset-error" role="alert">{error}</div>}
        {!rename && (
          <div className="paste-groups">
            {PRESET_SETTINGS.map((g) => (
              <label key={g} className="remove-check paste-group">
                <Checkbox checked={groups.includes(g)} onCheckedChange={(c) => toggle(g, c === true)} data-testid={`preset-group-${g}`} />
                <span>{LABEL[g]}</span>
              </label>
            ))}
          </div>
        )}
        <div className="remove-foot">
          {!rename && (
            <>
              <button className="import-link" onClick={() => setGroups([...PRESET_SETTINGS])}>Check all</button>
              <button className="import-link" onClick={() => setGroups([])}>Check none</button>
            </>
          )}
          <span className="grow" />
          <button className="import-link" onClick={() => setAsk(null)}>
            <Kbd>esc</Kbd> cancel
          </button>
          <Button size="sm" variant="default" onClick={() => void confirm()} disabled={!ready} data-testid="preset-save">
            {rename ? 'Rename' : ask?.mode === 'update' ? 'Update' : 'Create'} <Kbd>↵</Kbd>
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
