// Copy, Paste and Sync settings, as in Lightroom: ⇧⌘C asks which groups to copy, ⇧⌘V
// pastes them on the selection (or the current Photo), ⌥⌘V pastes the previous Photo's
// settings, and ⇧⌘S syncs the current Photo's settings to the rest of the selection.
// Live, every paste is one `paste_settings` batch in the Library, so ⌘Z undoes it.

import { useEffect, useRef, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { PHOTO_BY_ID } from '../data/dummy';
import { LIVE } from '../data/live';
import { DEFAULT_GROUPS, GROUPS, groupsLabel, pasteRecipe, type GroupId } from '../domain/settings';
import { recipeOf, useLibrary, type Action, type State } from '../store/library';
import { changePhotos } from '../store/changePhotos';
import { routeNow } from '../router';
import { Kbd } from './kit';
import { openApply } from './ApplyEdit';

/** What ⇧⌘C copied: the Photo (read when pasting, as the engine does) and its groups. */
let clip: { from: string; groups: GroupId[] } | null = null;
/** The Photo the person had open before this one, for ⌥⌘V. */
let previous: string | null = null;

const KEY = 'argus:paste-groups';
export function lastGroups(): GroupId[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null') as GroupId[] | null;
    if (Array.isArray(v) && v.every((g) => GROUPS.some((x) => x.id === g))) return v;
  } catch {
    // No storage (private window, preview): the defaults.
  }
  return DEFAULT_GROUPS;
}
function saveGroups(g: GroupId[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(g));
  } catch {
    // Not remembered this time.
  }
}

const plural = (n: number) => `${n} Photo${n === 1 ? '' : 's'}`;
const fileOf = (id: string) => PHOTO_BY_ID[id]?.file ?? `Photo ${id}`;

/**
 * Paste `groups` of `from`'s settings onto `to`: on more than one Photo through the Apply
 * popover (each drawn as it will look, untick to leave out), on one at once. `verb` names it.
 */
export function pasteSettings(state: State, dispatch: (a: Action) => void, from: string, to: string[], groups: GroupId[], verb: 'paste' | 'sync' | 'copy' = 'paste', at?: Element | null) {
  const targets = to.filter((id) => id !== from);
  if (!groups.length) return;
  if (!targets.length) return dispatch({ type: 'toast', text: `Nothing to paste on: these are ${fileOf(from)}’s own settings` });
  if (targets.length > 1) return openApply({ kind: 'paste', verb, from, groups, ids: targets, apply: (ids, g) => applyPaste(state, dispatch, from, ids, g) }, at);
  applyPaste(state, dispatch, from, targets, groups);
}

/** Paste now, as one undoable change. */
function applyPaste(state: State, dispatch: (a: Action) => void, from: string, to: string[], groups: GroupId[]) {
  const targets = to.filter((id) => id !== from);
  if (!targets.length || !groups.length) return saveGroups(groups);
  saveGroups(groups);
  const label = `Pasted ${groupsLabel(groups)} from ${fileOf(from)} to ${plural(targets.length)}`;
  const src = recipeOf(state, from);
  void changePhotos(dispatch, {
    label,
    tool: 'paste_settings',
    args: { from: Number(from), photo_ids: targets.map(Number), groups },
    written: () => targets.map(Number),
    local: () => targets.map((id) => ({ id, recipe: pasteRecipe(src, recipeOf(state, id), groups) })),
    by: 'you',
    each: 'Pasted settings',
  }).catch(() => {});
  if (LIVE) dispatch({ type: 'toast', text: label });
}

type Ask = { mode: 'copy' | 'sync'; from: string; targets: string[] };

/** Open the Copy settings or Sync settings dialog. */
export function askSettings(a: Ask) {
  window.dispatchEvent(new CustomEvent('argus:paste-settings', { detail: a }));
}

/**
 * The screen's keys for copying and pasting settings. `current` is the Photo in front of
 * the person, `targets` what a paste lands on (the selection, or just `current`).
 */
export function useSettingsKeys(current: string | null, targets: string[]) {
  const { state, dispatch } = useLibrary();
  const last = useRef(current);
  useEffect(() => {
    if (current && last.current && last.current !== current) previous = last.current;
    last.current = current;
  }, [current]);
  // Read when the key is pressed, not from the render that bound the keys: ⇧⌘V pressed right
  // after ⌘→ otherwise reached the Photo just left and, being the source, pasted nothing.
  const at = useRef({ current, targets, state });
  at.current = { current, targets, state };
  const now = {
    get current() {
      // Edit and the Loupe show the Photo in the URL, which moves before the screen does.
      const r = routeNow();
      const here = (r.name === 'edit' || r.name === 'loupe') && !at.current.targets.length ? r.id : at.current.current;
      return { current: here, to: at.current.targets.length ? at.current.targets : here ? [here] : [], state: at.current.state };
    },
  };
  const opts = { preventDefault: true };
  useHotkeys('mod+shift+c', () => {
    const { current } = now.current;
    if (current) askSettings({ mode: 'copy', from: current, targets: [] });
  }, opts);
  useHotkeys('mod+shift+v', () => pasteClip(now.current.state, dispatch, now.current.to), opts);
  useHotkeys(
    'mod+alt+v',
    () => {
      const { to, state } = now.current;
      if (!previous || !PHOTO_BY_ID[previous]) return dispatch({ type: 'toast', text: 'No previous Photo to paste from' });
      pasteSettings(state, dispatch, previous, to, lastGroups());
    },
    opts,
  );
  useHotkeys(
    'mod+shift+s',
    () => {
      const { current, to, state } = now.current;
      const rest = to.filter((id) => id !== current);
      if (current && rest.length) pasteSettings(state, dispatch, current, rest, lastGroups(), 'sync');
      else dispatch({ type: 'toast', text: 'Select the Photos to sync, then ⇧⌘S' });
    },
    opts,
  );
}

/** ⇧⌘V: what ⇧⌘C copied, onto `to`. */
export function pasteClip(state: State, dispatch: (a: Action) => void, to: string[]) {
  if (!clip) return dispatch({ type: 'toast', text: 'Nothing copied yet: ⇧⌘C copies settings' });
  pasteSettings(state, dispatch, clip.from, to, clip.groups);
}

/** Whether ⇧⌘V has something to paste, for menus. */
export const hasCopiedSettings = () => clip != null;

/** The Copy settings / Sync settings dialog, mounted once by the app. */
export function PasteSettingsDialog() {
  const { state, dispatch } = useLibrary();
  const [ask, setAsk] = useState<Ask | null>(null);
  const [groups, setGroups] = useState<GroupId[]>(DEFAULT_GROUPS);
  useEffect(() => {
    const on = (e: Event) => {
      setAsk((e as CustomEvent<Ask>).detail);
      setGroups(lastGroups());
    };
    window.addEventListener('argus:paste-settings', on);
    return () => window.removeEventListener('argus:paste-settings', on);
  }, []);
  const toggle = (g: GroupId, on: boolean) => setGroups((gs) => (on ? GROUPS.map((x) => x.id).filter((x) => x === g || gs.includes(x)) : gs.filter((x) => x !== g)));
  const confirm = () => {
    if (!ask || !groups.length) return;
    saveGroups(groups);
    if (ask.mode === 'copy') {
      clip = { from: ask.from, groups };
      dispatch({ type: 'toast', text: `Copied ${groupsLabel(groups)} from ${fileOf(ask.from)}: ⇧⌘V pastes` });
    } else pasteSettings(state, dispatch, ask.from, ask.targets, groups, 'sync');
    setAsk(null);
  };
  const title = !ask ? '' : ask.mode === 'copy' ? `Copy settings from ${fileOf(ask.from)}` : `Sync ${fileOf(ask.from)}’s settings to ${plural(ask.targets.length)}`;
  return (
    <Dialog open={!!ask} onOpenChange={(o) => !o && setAsk(null)}>
      <DialogContent className="remove-dialog" closable={false} data-testid="settings-dialog" onKeyDown={(e) => {
          if (e.key === 'Enter') (e.preventDefault(), confirm());
        }}>
        <DialogTitle className="remove-title">{title}</DialogTitle>
        <DialogDescription className="remove-note">Ticked groups replace the same settings on the Photos you paste on. Everything else on them stays.</DialogDescription>
        <div className="paste-groups">
          {GROUPS.map((g) => (
            <label key={g.id} className="remove-check paste-group">
              <Checkbox checked={groups.includes(g.id)} onCheckedChange={(c) => toggle(g.id, c === true)} data-testid={`paste-${g.id}`} />
              <span>{g.label}</span>
            </label>
          ))}
        </div>
        <div className="remove-foot">
          <button className="import-link" onClick={() => setGroups(GROUPS.map((g) => g.id))}>Check all</button>
          <button className="import-link" onClick={() => setGroups([])}>Check none</button>
          <span className="grow" />
          <button className="import-link" onClick={() => setAsk(null)}>
            <Kbd>esc</Kbd> cancel
          </button>
          <Button size="sm" onClick={confirm} disabled={!groups.length} data-testid="settings-confirm">
            {ask?.mode === 'sync' ? 'Sync' : 'Copy'} <Kbd>↵</Kbd>
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
