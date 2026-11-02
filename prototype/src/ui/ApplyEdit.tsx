// Paste, Sync and Auto on many Photos go through the Apply popover (Greg, 2026-11-02: "this
// should be my average experience when applying edits to everything"), as Apply look does:
// ⇧⌘V, ⌥⌘V and ⇧⌘S, Cull's Apply this edit (⇧⌘A) and its right-click Paste and Sync, and
// Auto (⇧A, the Auto button, ⌘K) on a selection or the whole Shoot, and Cull's A (the AI's
// first take on each selected Photo). Each Photo is drawn as the
// edit leaves it (engine `apply_preview`), Photos edited by hand start left out, Return
// applies the ticked ones as one Undo and Esc changes nothing. One Photo still applies at once.
//
// Mounted once by the app; `openApply` opens it from anywhere.

import { useEffect, useMemo, useState } from 'react';
import { Check, ChevronDown } from 'lucide-react';
import { PHOTOS, PHOTO_BY_ID, SHOOT } from '../data/dummy';
import { useApplyPreview, type Change } from '../data/applyPreview';
import { call } from '../data/backend';
import { demoAutoRecipe } from '../data/auto';
import { picksOf } from '../data/modes';
import { LIVE, readRecipes } from '../data/live';
import { GROUPS, groupsLabel, pasteRecipe, type GroupId } from '../domain/settings';
import { recipeOf, useLibrary, verdictOf } from '../store/library';
import type { Suggestion } from '../domain/types';
import { PhotoView } from './PhotoView';
import { ApplyBody, ApplyPopoverAt, toolbarCorner, type ApplyTile, type Scope } from './ApplyPopover';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';
import { Button } from '@/components/ui/button';

/** What the popover applies, and to which Photos. */
export type ApplyAsk =
  | {
      kind: 'paste';
      /** Paste, Sync or Cull's Apply this edit: only the words differ. */
      verb: 'paste' | 'sync' | 'copy';
      from: string;
      groups: GroupId[];
      ids: string[];
      /** Applies the ticked Photos (ui/PasteSettings.tsx's paste). */
      apply: (ids: string[], groups: GroupId[]) => void;
    }
  | {
      kind: 'auto';
      /** The selection; null for the whole Shoot (Picks, or All but Rejects). */
      ids: string[] | null;
      apply: (ids: string[]) => void;
    }
  | {
      kind: 'look';
      /** Sync to Look: this Photo's own sliders folded into the Look its Shoot shares. */
      from: string;
      /** Applies with the unticked Photos kept as they look now (taken off the Look). */
      apply: (keepOut: string[]) => void;
    }
  | {
      kind: 'agent';
      /** An Agent turn that edited several Photos, saved as `batch`: reviewed after the fact. */
      batch: number;
      ids: string[];
      /** Return: the unticked Photos go back as they were. */
      apply: (putBack: string[]) => void;
      /** Esc or Cancel: the whole turn is undone. */
      cancel: () => void;
      done?: boolean;
    }
  | {
      kind: 'takes';
      /** One waiting AI take per Photo: the first (Cull's A) or a Crop like this offer. */
      takes: Suggestion[];
      /** Words for the header, note and foot, when they aren't Cull's A. */
      words?: { title: string; note: string; foot: string };
      /** The take changes only what the person didn't (a crop): hand-edited Photos start ticked too. */
      onHand?: boolean;
      apply: (ids: string[]) => void;
    };

const EVENT = 'argus:apply';

/** Open the Apply popover, by `at` (the button that started it) or under the toolbar. */
export function openApply(ask: ApplyAsk, at?: Element | null) {
  window.dispatchEvent(new CustomEvent(EVENT, { detail: { ask, at: at?.getBoundingClientRect() ?? toolbarCorner() } }));
}

/**
 * Auto on `ids` (the selection; null for the whole Shoot): through the popover when it reaches
 * more than one Photo, at once on one.
 */
export function askAuto(ids: string[] | null, run: (ids: string[]) => void, at?: Element | null) {
  if (ids && ids.length <= 1) return ids.length && run(ids);
  openApply({ kind: 'auto', ids, apply: run }, at);
}

const plural = (n: number) => `${n} Photo${n === 1 ? '' : 's'}`;
const fileOf = (id: string) => PHOTO_BY_ID[id]?.file ?? `Photo ${id}`;
const handEdited = (versions: { by: string }[] | undefined) => versions?.at(-1)?.by === 'you';

export function ApplyEditHost() {
  const [open, setOpen] = useState<{ ask: ApplyAsk; at: DOMRect } | null>(null);
  useEffect(() => {
    const on = (e: Event) => setOpen((e as CustomEvent<{ ask: ApplyAsk; at: DOMRect }>).detail);
    window.addEventListener(EVENT, on);
    return () => window.removeEventListener(EVENT, on);
  }, []);
  const close = () => {
    // Closing an Agent turn's review without Apply undoes the turn, so Esc leaves the
    // Photos as they were before the request.
    if (open?.ask.kind === 'agent' && !open.ask.done) open.ask.cancel();
    setOpen(null);
  };
  return (
    <ApplyPopoverAt at={open?.at ?? null} onClose={close} testId="apply-edit">
      {open && (open.ask.kind === 'paste' ? <PasteBody key={JSON.stringify(open.at)} ask={open.ask} onClose={close} /> : open.ask.kind === 'auto' ? <AutoBody key={JSON.stringify(open.at)} ask={open.ask} onClose={close} /> : open.ask.kind === 'look' ? <LookBody key={JSON.stringify(open.at)} ask={open.ask} onClose={close} /> : open.ask.kind === 'agent' ? <AgentBody key={JSON.stringify(open.at)} ask={open.ask} onClose={close} /> : <TakesBody key={JSON.stringify(open.at)} ask={open.ask} onClose={close} />)}
    </ApplyPopoverAt>
  );
}

function PasteBody({ ask, onClose }: { ask: Extract<ApplyAsk, { kind: 'paste' }>; onClose: () => void }) {
  const { state } = useLibrary();
  const [groups, setGroups] = useState<GroupId[]>(ask.groups);
  const ids = useMemo(() => ask.ids.filter((id) => id !== ask.from), [ask]);
  const change = useMemo<Change>(() => ({ kind: 'paste', from: ask.from, groups }), [ask.from, groups]);
  const pv = useApplyPreview(change, ids, groups.length > 0);
  const src = PHOTO_BY_ID[ask.from];
  const tiles: ApplyTile[] = [
    ...(src ? [{ id: ask.from, fallback: <PhotoView photo={src} size="tile" edits style={{ width: '100%', height: '100%', borderRadius: 0 }} />, lock: 'source' as const, badge: 'From', says: 'the settings come from here' }] : []),
    ...tilesFor(ids, pv, (id) => pasteRecipe(recipeOf(state, ask.from), recipeOf(state, id), groups), state.versions),
  ];
  const title = ask.verb === 'sync' ? `Sync ${fileOf(ask.from)}'s settings` : ask.verb === 'copy' ? 'Apply this edit' : `Paste ${fileOf(ask.from)}'s settings`;
  return (
    <ApplyBody
      testId="apply-edit"
      title={title}
      context={plural(ids.length)}
      controls={<GroupPicker groups={groups} onChange={setGroups} />}
      note={<>{groupsLabel(groups).replace(/^./, (c) => c.toUpperCase())} replace the same settings on each; everything else stays. Photos you edited by hand start left out. Click a photo to leave it out or put it in.</>}
      reading={readingOf(pv, 'Drawing each photo')}
      tiles={tiles}
      foot={(n) => <>{n} photo{n === 1 ? '' : 's'} change. One Undo.</>}
      footNote={footNoteOf(pv)}
      wait={!groups.length}
      onApply={(targets) => {
        ask.apply(targets, groups);
        onClose();
      }}
      onClose={onClose}
    />
  );
}

function AutoBody({ ask, onClose }: { ask: Extract<ApplyAsk, { kind: 'auto' }>; onClose: () => void }) {
  const { state } = useLibrary();
  const [scope, setScope] = useState<Scope>('all');
  const shoot = useMemo(() => {
    const live = PHOTOS.slice(0, state.imported).filter((p) => verdictOf(state, p.id) !== 'reject');
    const picks = picksOf(state);
    return { picks: picks.map((p) => p.id), all: live.map((p) => p.id) };
    // Only flags and new Photos change which Photos show.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.imported, PHOTOS.length, JSON.stringify(state.aiVerdicts), JSON.stringify(state.overrides)]);
  const ids = useMemo(() => ask.ids ?? (scope === 'picks' && shoot.picks.length ? shoot.picks : shoot.all), [ask.ids, scope, shoot]);
  const change = useMemo<Change>(() => ({ kind: 'auto' }), []);
  const pv = useApplyPreview(change, ids, true);
  const tiles = tilesFor(ids, pv, (id) => demoAutoRecipe(id, recipeOf(state, id)), state.versions);
  // Auto Denoise rides along with Auto: say how many it reaches, since no tile can show it.
  const denoised = ids.filter((id) => pv.frames[id]?.denoise).length;
  return (
    <ApplyBody
      testId="apply-edit"
      title="Auto"
      context={ask.ids ? plural(ids.length) : SHOOT.name}
      scope={ask.ids ? undefined : { value: scope, onChange: setScope, picks: shoot.picks.length, all: shoot.all.length }}
      note={<>Exposure, tone and white balance from each photo's own light. Photos you edited by hand start left out. Click a photo to leave it out or put it in.</>}
      reading={readingOf(pv, 'Measuring each photo')}
      tiles={tiles}
      foot={(n) => (
        <>
          {n} photo{n === 1 ? '' : 's'} change. Look, color, Masks and crops stay. One Undo.
          {denoised > 0 && ` ${denoised} noisy RAW${denoised === 1 ? ' gets' : 's get'} AI Denoise.`}
        </>
      )}
      footNote={footNoteOf(pv)}
      onApply={(targets) => {
        ask.apply(targets);
        onClose();
      }}
      onClose={onClose}
    />
  );
}

function LookBody({ ask, onClose }: { ask: Extract<ApplyAsk, { kind: 'look' }>; onClose: () => void }) {
  const { state } = useLibrary();
  // Which Photos the Look reaches once synced, from the engine (the Picks, if it makes the Look).
  const [under, setUnder] = useState<{ ids: string[]; name: string } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    call<{ photos_under_look: number[]; look: { name: string } }>('sync_to_look', { photo_id: Number(ask.from), dry_run: true }).then(
      (r) => setUnder({ ids: r.photos_under_look.map(String).filter((id) => id !== ask.from), name: r.look.name }),
      (e) => setErr(String(e).replace(/^Error: /, '')),
    );
  }, [ask.from]);
  const ids = under?.ids ?? [];
  const change = useMemo<Change>(() => ({ kind: 'sync_look', from: ask.from }), [ask.from]);
  const pv = useApplyPreview(change, ids, !!under);
  const src = PHOTO_BY_ID[ask.from];
  // Everyone under the Look changes with it, hand edits too (their own sliders stay on top).
  const tiles: ApplyTile[] = [
    ...(src ? [{ id: ask.from, fallback: <PhotoView photo={src} size="tile" edits style={{ width: '100%', height: '100%', borderRadius: 0 }} />, lock: 'source' as const, badge: 'From', says: 'its sliders go into the Look' }] : []),
    ...tilesFor(ids, pv, (id) => recipeOf(state, id), state.versions).map((t) => ({ ...t, startOff: t.startOff === undefined ? undefined : false, badge: t.warn ? t.badge : undefined })),
  ];
  return (
    <ApplyBody
      testId="apply-edit"
      title={`Sync ${fileOf(ask.from)} to the Look`}
      context={under?.name ?? SHOOT.name}
      note={<>Its own sliders go into the Look, so every photo under it changes with it; each keeps its own sliders on top. A photo you leave out keeps how it looks now and comes off the Look.</>}
      reading={under ? readingOf(pv, 'Drawing each photo') : { label: 'Finding the photos under the Look', done: 0, total: 1, eta: null }}
      tiles={tiles}
      foot={(n) => <>{n} photo{n === 1 ? '' : 's'} follow the Look. One Undo.</>}
      footNote={err ? <span className="faint trunc" title={err}>Couldn't sync: {err}</span> : footNoteOf(pv)}
      wait={!under}
      onApply={(targets) => {
        ask.apply(ids.filter((id) => !targets.includes(id)));
        onClose();
      }}
      onClose={onClose}
    />
  );
}

function AgentBody({ ask, onClose }: { ask: Extract<ApplyAsk, { kind: 'agent' }>; onClose: () => void }) {
  const { state } = useLibrary();
  // The Photos as the Agent left them: their saved Recipes, drawn by the engine.
  const [recipes, setRecipes] = useState<Record<string, unknown> | null>(null);
  useEffect(() => {
    void readRecipes(ask.ids).then(setRecipes, () => setRecipes({}));
  }, [ask.ids]);
  const change = useMemo<Change>(() => ({ kind: 'recipes', recipes: recipes ?? {} }), [recipes]);
  const ids = useMemo(() => ask.ids.filter((id) => recipes && id in recipes), [ask.ids, recipes]);
  const pv = useApplyPreview(change, ids, !!recipes);
  const tiles = tilesFor(ids, pv, (id) => recipeOf(state, id), state.versions).map((t) => ({ ...t, startOff: t.startOff === undefined ? undefined : false, badge: t.warn ? t.badge : undefined }));
  return (
    <ApplyBody
      testId="apply-edit"
      title="Keep the Agent's edits"
      context={plural(ask.ids.length)}
      note={<>Each photo as the Agent left it. Untick one to put it back as it was; Esc puts them all back.</>}
      reading={recipes ? readingOf(pv, 'Drawing each photo') : { label: 'Reading the edits', done: 0, total: 1, eta: null }}
      tiles={tiles}
      foot={(n) => <>{n} photo{n === 1 ? '' : 's'} keep the edit; the rest go back. One Undo.</>}
      footNote={footNoteOf(pv)}
      wait={!recipes}
      onApply={(targets) => {
        ask.done = true;
        ask.apply(ask.ids.filter((id) => !targets.includes(id)));
        onClose();
      }}
      onClose={onClose}
    />
  );
}

function TakesBody({ ask, onClose }: { ask: Extract<ApplyAsk, { kind: 'takes' }>; onClose: () => void }) {
  const { state } = useLibrary();
  const ids = useMemo(() => ask.takes.map((t) => t.photoId), [ask]);
  const byId = useMemo(() => Object.fromEntries(ask.takes.map((t) => [t.photoId, t])), [ask]);
  const change = useMemo<Change>(() => ({ kind: 'recipes', recipes: Object.fromEntries(ask.takes.map((t) => [t.photoId, t.recipe])) }), [ask]);
  const pv = useApplyPreview(change, ids, true);
  const tiles = tilesFor(ids, pv, (id) => byId[id].recipe, state.versions).map((t) => ({ ...t, startOff: ask.onHand && t.startOff !== undefined ? false : t.startOff, badge: t.badge ?? byId[t.id].title, says: t.says ?? byId[t.id].title }));
  return (
    <ApplyBody
      testId="apply-edit"
      title={ask.words?.title ?? "Accept the AI's first take"}
      context={plural(ids.length)}
      note={<>{ask.words?.note ?? "Each photo with the AI's first suggestion for it."}{ask.onHand ? '' : ' Photos you edited by hand start left out.'} Click a photo to leave it out or put it in.</>}
      reading={readingOf(pv, 'Drawing each take')}
      tiles={tiles}
      foot={(n) => <>{n} photo{n === 1 ? '' : 's'} {ask.words?.foot ?? 'take their first suggestion; the others stay waiting'}. One Undo.</>}
      footNote={footNoteOf(pv)}
      onApply={(targets) => {
        ask.apply(targets);
        onClose();
      }}
      onClose={onClose}
    />
  );
}

/** Tiles for `ids`: the engine's render when live, the demo's own render of `demo(id)` otherwise. */
function tilesFor(ids: string[], pv: ReturnType<typeof useApplyPreview>, demo: (id: string) => ReturnType<typeof recipeOf>, versions: Record<string, { by: string }[]>): ApplyTile[] {
  return ids.flatMap((id) => {
    const p = PHOTO_BY_ID[id];
    if (!p) return [];
    const f = pv.frames[id];
    // Live, the engine says which are the person's own; until it has, it's not known yet.
    const hand = f ? f.hand : LIVE && !pv.error && !pv.gaveUp.includes(id) ? undefined : handEdited(versions[id]);
    const notDrawn = pv.gaveUp.includes(id) || !!f?.error;
    return [
      {
        id,
        image: f?.image,
        fallback: <PhotoView photo={p} size="tile" recipe={demo(id)} edits style={{ width: '100%', height: '100%', borderRadius: 0 }} />,
        startOff: hand,
        badge: notDrawn ? <span title={f?.error ?? 'Took too long to draw'}>not drawn</span> : hand ? 'edited by you' : f?.denoise ? <span title="AI Denoise, set from this RAW's own noise; too fine to see in a tile">Denoise {Math.round(f.denoise)}</span> : undefined,
        warn: notDrawn,
        says: hand ? 'edited by you' : undefined,
      },
    ];
  });
}

function readingOf(pv: ReturnType<typeof useApplyPreview>, label: string) {
  if (!pv.reading) return null;
  return { label: pv.slow ? `Still on ${fileOf(pv.slow.id)} · ${pv.slow.seconds} s` : label, done: pv.reading.done, total: pv.reading.total, eta: pv.slow ? null : pv.reading.eta };
}

function footNoteOf(pv: ReturnType<typeof useApplyPreview>) {
  if (pv.error) return <span className="faint trunc" title={pv.error}>Couldn't draw: {pv.error}</span>;
  const undrawn = pv.gaveUp.length + Object.values(pv.frames).filter((f) => f.error).length;
  return undrawn ? <span className="faint trunc">{undrawn} couldn't be drawn; Apply still tries them</span> : undefined;
}

/** Which settings groups a paste carries, as Lightroom's Copy settings asks. */
function GroupPicker({ groups, onChange }: { groups: GroupId[]; onChange: (g: GroupId[]) => void }) {
  const toggle = (g: GroupId) => onChange(groups.includes(g) ? groups.filter((x) => x !== g) : GROUPS.map((x) => x.id).filter((x) => x === g || groups.includes(x)));
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" data-testid="apply-edit-groups">
          {groups.length === GROUPS.length ? 'All settings' : groups.length ? groupsLabel(groups) : 'No settings'} <ChevronDown size={12} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {GROUPS.map((g) => (
          <DropdownMenuItem key={g.id} onSelect={(e) => (e.preventDefault(), toggle(g.id))} data-testid={`apply-edit-group-${g.id}`}>
            <Check size={13} style={{ opacity: groups.includes(g.id) ? 1 : 0 }} />
            {g.label}
          </DropdownMenuItem>
        ))}
        <DropdownMenuItem onSelect={(e) => (e.preventDefault(), onChange(GROUPS.map((g) => g.id)))}>
          <Check size={13} style={{ opacity: 0 }} />
          Check all
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
