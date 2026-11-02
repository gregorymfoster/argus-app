// Cull mode (⌘2, mocked as MO-Cull and MO-Cull-Together, Greg 2026-11-01): the Shoot one
// moment at a time, every frame of it side by side (Lightroom's Survey). A moment is a Stack,
// best frame first, or a short run of single frames, so one pass decides every Photo. Z keeps,
// X rejects, ↵ keeps and moves on. ⇧↓ pulls the next moment in as another row, to keep the best
// across a couple of Stacks or merge Stacks the AI split (⇧M). Every ↵ is logged as bench data.
//
// Rounds (GLOSSARY: Round; Greg, 2026-11-01: "I like recursive culling passes"): once every Photo
// is flagged, Round 2 restacks the Picks into moments and goes through them again, Z keeping and
// X cutting; Round 3 runs over Round 2's keepers, and so on. A cut never takes the Pick away.

import { useEffect, useMemo, useRef, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Flag, Layers, Merge, Repeat, Sparkles, X } from 'lucide-react';
import { PHOTOS, PHOTO_BY_ID } from '../data/dummy';
import type { Photo, Verdict } from '../domain/types';
import { fitGrid, keptOnEnter, moments, pageSize, pages, type Moment } from '../domain/moments';
import { logMoment, mergeStacks } from '../data/moments';
import { roundIn, starsOf, useLibrary, verdictOf, type Action } from '../store/library';
import { inPass } from '../domain/rounds';
import { go, openFrom } from '../router';
import { PhotoView, photoRatio } from '../ui/PhotoView';
import { Stars } from '../ui/Stars';
import { EmptyState } from '../ui/Empty';
import { Button } from '@/components/ui/button';
import { Kbd, Tip, VERDICT_LABEL, ESC_OUTSIDE_DIALOGS } from '../ui/kit';
import { Hints, ModeSwitch } from '../ui/ViewBar';
import { aiName } from '../data/live';
import { flaggedHere, isSettled, setLastPass } from '../data/modes';
import { keepInView } from '../ui/PicksRail';
import { StatusFoot } from '../ui/StatusBar';

/** The Survey's size, for sizing and paging its frames. */
function useBox() {
  const [box, setBox] = useState({ w: 0, h: 0 });
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setBox({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(el);
    return () => ro.disconnect();
  }, [el]);
  return { ref: setEl, ...box };
}

/** Frames you picked here (same reason). */
const pickedHere = new Set<string>();

/** What the bench gets for the moment in hand: when it opened, what had focus, what was zoomed.
 *  Module-level, so a trip to the Loupe (Space) and back keeps it. */
let track: { key: string; opened: number; looked: string[]; zoomed: Set<string>; mergedFrom?: string[][] } | null = null;

const label = (m: Moment, n: number) => (m.kind === 'stack' ? `Stack ${n}` : m.photoIds.length === 1 ? `Single ${n}` : `Singles ${n}`);

/** What a Round calls your two marks. */
const ROUND_LABEL = { pick: 'Kept', reject: 'Cut' } as const;

export function CullModeScreen({ start, pass = 1 }: { start?: string; pass?: number }) {
  const { state, dispatch } = useLibrary();
  const [rev, setRev] = useState(0); // bumps after a merge re-stacks the Shoot
  useEffect(() => void setLastPass(pass), [pass]);
  // Pass 1 is the whole Shoot; Round k the Picks that reached Round k-1, restacked: a Stack keeps
  // the frames still in the running together, and a lone survivor joins the singles around it.
  const imported = PHOTOS.slice(0, state.imported);
  const poolKey = pass === 1 ? '' : imported.filter((p) => inPass(roundIn(state, p.id), pass)).map((p) => p.id).join();
  const list = useMemo(
    () => moments(pass === 1 ? imported : imported.filter((p) => inPass(roundIn(state, p.id), pass))),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [state.imported, rev, poolKey],
  );
  // Your mark on a frame in this pass: pass 1 is your flag (an AI Verdict alone isn't yours),
  // a Round its keep or cut.
  const markOf = (id: string): 'pick' | 'reject' | null => {
    if (pass > 1) {
      const m = state.passMarks[id]?.[pass];
      return m === true ? 'pick' : m === false ? 'reject' : null;
    }
    const v = verdictOf(state, id);
    return isSettled(state, id) && (v === 'pick' || v === 'reject') ? v : null;
  };
  // Cull makes you flag every Photo (Greg, 2026-11-01): one is settled once you marked it yourself,
  // and a moment is done only when all its frames are.
  const settled = (id: string) => (pass > 1 ? markOf(id) != null : isSettled(state, id));
  const isDone = (m: Moment) => m.photoIds.every(settled);
  const markWord = pass > 1 ? ROUND_LABEL : VERDICT_LABEL;
  const cutWord = pass > 1 ? 'cut' : 'rejected';
  const doneCount = list.filter(isDone).length;
  const unflagged = list.reduce((n, m) => n + m.photoIds.filter((id) => !settled(id)).length, 0);
  const [at, setAt] = useState(() => {
    const i = list.findIndex((m) => m.id === start || m.photoIds.includes(start ?? ''));
    if (i >= 0) return i;
    return Math.max(0, list.findIndex((m) => !isDone(m)));
  });
  // How many moments show together, from `at`: 1 is the Survey, more are rows.
  const [span, setSpan] = useState(1);
  const shown = list.slice(at, at + span);
  const frames = shown.flatMap((m) => m.photoIds.map((id) => PHOTO_BY_ID[id])).filter(Boolean) as Photo[];
  // Frames share one box, shaped for the widest Photo; a portrait among landscapes letterboxes.
  const aspect = Math.max(0.5, ...frames.map(photoRatio));
  const box = useBox();
  // A big Stack comes in pages of what fits at a size worth comparing; after the last page, a
  // final page of the Picks it collected (a small tournament: Greg, 2026-11-01).
  const [page, setPage] = useState(0);
  const allIds = frames.map((p) => p.id);
  const book = span === 1 ? pages(allIds, pageSize(allIds.length, box.w, box.h, aspect)) : [allIds];
  const finals = book.length > 1 && page >= book.length;
  // The final page holds the Picks as they were on arriving, so an X there doesn't hide the frame.
  const [finalIds, setFinalIds] = useState<string[]>([]);
  const onScreenIds = finals ? finalIds : (book[Math.min(page, book.length - 1)] ?? []);
  const onScreen = onScreenIds.map((id) => PHOTO_BY_ID[id]).filter(Boolean) as Photo[];
  const g = fitGrid(onScreen.length, box.w, box.h, 8, 25, aspect);
  const fitStyle = { '--cols': g.cols, '--rows': g.rows, ...(g.width ? { '--cell-w': `${g.width}px`, '--cell-h': `${Math.floor(g.width / aspect)}px` } : {}) } as React.CSSProperties;
  const [focusId, setFocusId] = useState<string | undefined>();
  const canNextRef = useRef(false);
  const nextRef = useRef(() => {});
  const view = span === 1 ? onScreen : frames;
  const cur = view.find((p) => p.id === focusId) ?? view[0];
  const rowOf = (id: string) => shown.findIndex((m) => m.photoIds.includes(id));

  const key = shown.map((m) => m.id).join('|');
  useEffect(() => setPage(0), [key]);
  // A new moment starts a new record; adding rows keeps the clock and what you looked at.
  if (shown[0] && track?.key !== shown[0].id) track = { key: shown[0].id, opened: performance.now(), looked: [], zoomed: new Set() };
  useEffect(() => {
    // Each moment opens on the frame you kept, else the AI's best.
    const kept = view.find((p) => yourPick(p.id));
    setFocusId((kept ?? view[0])?.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, page, finals]);
  useEffect(() => {
    if (cur && track && track.looked.at(-1) !== cur.id) track.looked.push(cur.id);
  }, [cur?.id]);

  // The change that marks a frame: a flag in pass 1, a keep or cut in a Round.
  const markAction = (id: string, v: Verdict): Action =>
    pass > 1 ? { type: 'pass/set', id, pass, kept: v === 'unflagged' ? null : v === 'pick' } : { type: 'verdict/set', id, verdict: v, by: 'you' };
  const set = (id: string, v: Verdict) => {
    if (pass > 1) return dispatch(markAction(id, v));
    if (v === 'pick') pickedHere.add(id);
    else pickedHere.delete(id);
    if (v === 'unflagged') flaggedHere.delete(id);
    else flaggedHere.add(id);
    dispatch({ type: 'verdict/set', id, verdict: v, by: 'you' });
  };
  const yourPick = (id: string) => (pass > 1 ? markOf(id) === 'pick' : verdictOf(state, id) === 'pick' && (state.overrides[id] === 'pick' || pickedHere.has(id)));
  const stars = (id: string, n: number) => dispatch({ type: 'stars/set', id, stars: n, by: 'you' });

  // The next moment with an unflagged frame, wrapping round to any you skipped; past the end
  // (the done screen) only once every Photo is flagged.
  const next = (from: number, skip = new Set<string>()) => {
    const open = (m: Moment, j: number) => !skip.has(m.id) && !isDone(m) && j >= 0;
    const i = list.findIndex((m, j) => j >= from && open(m, j));
    if (i >= 0) return i;
    const w = list.findIndex(open);
    return w >= 0 ? w : list.length;
  };

  // ↵: keep the frames you picked here (none yet, in a Stack: the focused one) and reject the
  // rest, so no frame is left Unflagged. Then the next moment that still has one.
  const keep = () => {
    if (!shown.length) return;
    // In the Survey, ↵ settles the page on screen; more pages (or the final look) come next.
    const screen: Moment[] = span === 1 ? [{ ...shown[0], photoIds: onScreenIds }] : shown;
    const picked = new Set(view.filter((p) => yourPick(p.id)).map((p) => p.id));
    const elsewhere = allIds.filter((id) => !onScreenIds.includes(id) && markOf(id) === 'pick');
    // The focused frame stands in for "keep this one" only while nothing else in the moment is kept.
    const stand = !finals && !elsewhere.length && cur && (pass > 1 ? markOf(cur.id) : state.overrides[cur.id]) !== 'reject' ? cur.id : undefined;
    const keptHere = keptOnEnter(screen, picked, stand);
    const actions: Action[] = [];
    const rejected: string[] = [];
    for (const m of screen)
      for (const id of m.photoIds) {
        const v = verdictOf(state, id) ?? 'unflagged';
        const target: Verdict = keptHere.has(id) ? 'pick' : 'reject';
        if (target === 'reject') rejected.push(id);
        const same = pass > 1 ? markOf(id) === target : state.overrides[id] === target && v === target;
        if (pass === 1) {
          flaggedHere.add(id);
          if (target === 'pick') pickedHere.add(id);
        }
        if (!same) actions.push(markAction(id, target));
      }
    const n = screen.reduce((a, m) => a + m.photoIds.length, 0);
    dispatch({ type: 'batch', label: `${pass > 1 ? `Round ${pass}: kept` : 'Kept'} ${keptHere.size}, ${cutWord} ${n - keptHere.size}`, actions });
    const pickedSoFar = new Set([...elsewhere, ...keptHere]);
    if (span === 1 && book.length > 1 && !finals) {
      if (page < book.length - 1) return setPage(page + 1);
      if (pickedSoFar.size > 1) return (setFinalIds(allIds.filter((id) => pickedSoFar.has(id))), setPage(book.length));
    }
    const kept = pickedSoFar;
    for (const id of allIds) if (!kept.has(id) && !rejected.includes(id)) rejected.push(id);
    const stacks = shown.filter((m) => m.kind === 'stack');
    logMoment({
      moment_id: `${shown[0].id}@${Date.now().toString(36)}`,
      kind: stacks.length > 1 ? 'stacks' : shown[0].kind === 'stack' ? 'stack' : 'singles',
      shown: frames.map((p) => p.id),
      stacks: stacks.map((m) => m.photoIds),
      kept: [...kept],
      rejected,
      ai_best: stacks.map((m) => m.photoIds[0]),
      zoomed: [...(track?.zoomed ?? [])],
      looked: track?.looked.slice() ?? [],
      seconds: Math.round((performance.now() - (track?.opened ?? performance.now())) / 100) / 10,
      rest: 'reject',
      ai_best_shown: stacks.length > 0,
      outcome: kept.size === 0 ? 'none' : kept.size === frames.length ? 'all' : 'some',
      merged_from: track?.mergedFrom,
      pass,
    });
    track = null;
    const after = at + span;
    setSpan(1);
    // `state` is this render's, so the moments just flagged still read as open: skip them.
    setAt(next(after, new Set(shown.map((m) => m.id))));
  };

  const step = (d: number) => {
    setSpan(1);
    setPage(0);
    setAt((i) => Math.max(0, Math.min(list.length - 1, i + d)));
  };
  const more = () => setSpan((s) => Math.max(1, Math.min(s + 1, list.length - at)));
  const fewer = () => setSpan((s) => Math.max(1, s - 1));
  const shownStacks = shown.filter((m) => m.kind === 'stack');
  // Stacks shown together merge, even with single frames between them (they stay single).
  const canMerge = shownStacks.length > 1;
  const merging = useRef(false);
  const merge = async () => {
    if (!canMerge || merging.current) return;
    merging.current = true;
    const from = shownStacks.map((m) => m.photoIds);
    let into: string;
    try {
      into = await mergeStacks(shownStacks.map((m) => m.id));
    } catch (e) {
      dispatch({ type: 'toast', text: `Couldn't merge the Stacks: ${String((e as Error)?.message ?? e)}` });
      return;
    } finally {
      merging.current = false;
    }
    track = { ...(track ?? { opened: performance.now(), looked: [], zoomed: new Set() }), key: into, mergedFrom: from };
    const n = shownStacks.reduce((a, m) => a + m.photoIds.length, 0);
    setSpan(1);
    setRev((r) => r + 1);
    dispatch({ type: 'toast', text: `Merged ${shownStacks.length} Stacks into one of ${n}` });
  };

  // Arrows: ← → move within a row; ↑ ↓ move between rows when several show, else between moments.
  const move = (d: number) => {
    if (!cur) return;
    const row = span === 1 ? onScreenIds : (shown[rowOf(cur.id)]?.photoIds ?? []);
    const i = row.indexOf(cur.id) + d;
    if (i >= 0 && i < row.length) setFocusId(row[i]);
  };
  const vertical = (d: number) => {
    if (span === 1 || !cur) return step(d);
    const r = rowOf(cur.id) + d;
    if (r < 0 || r >= shown.length) return;
    const col = shown[rowOf(cur.id)].photoIds.indexOf(cur.id);
    const row = shown[r].photoIds;
    setFocusId(row[Math.min(col, row.length - 1)]);
  };
  const zoom = (id = cur?.id) => {
    if (!id) return;
    track?.zoomed.add(id);
    openFrom({ name: 'moments', id: list[at]?.id, pass }, { name: 'loupe', id });
  };

  useHotkeys(['right', 'l'], () => move(1), [cur, key, page, onScreenIds.join()]);
  useHotkeys(['left', 'h'], () => move(-1), [cur, key, page, onScreenIds.join()]);
  // ] and [ page through a big Stack without flagging (↵ flags the page and turns it).
  useHotkeys('bracketright', () => setPage((p) => Math.min(p + 1, book.length - 1)), [book.length]);
  useHotkeys('bracketleft', () => setPage((p) => Math.max(0, Math.min(p, book.length - 1) - 1)), [book.length]);
  useHotkeys(['down', 'j'], (e) => (e.preventDefault(), vertical(1)), [cur, key, span]);
  useHotkeys(['up', 'k'], (e) => (e.preventDefault(), vertical(-1)), [cur, key, span]);
  useHotkeys('tab', () => step(1), { preventDefault: true }, [list]);
  useHotkeys('shift+tab', () => step(-1), { preventDefault: true }, [list]);
  useHotkeys('shift+down', more, { preventDefault: true }, [at, list]);
  useHotkeys('shift+up', fewer, { preventDefault: true });
  useHotkeys('shift+m', () => void merge(), [key, canMerge]);
  // Flags stick (Greg, 2026-11-01): Z on a Pick keeps it a Pick; U clears.
  useHotkeys(['z', 'p'], () => cur && set(cur.id, 'pick'), [cur, state]);
  useHotkeys('x', () => cur && set(cur.id, 'reject'), [cur, state]);
  useHotkeys('u', () => cur && set(cur.id, 'unflagged'), [cur]);
  useHotkeys(['0', '1', '2', '3', '4', '5'], (e) => cur && stars(cur.id, Number(e.key)), [cur]);
  useHotkeys('enter', keep, { preventDefault: true }, [key, cur, state, at, span, page, box.w, box.h]);
  useHotkeys('space', () => zoom(), { preventDefault: true }, [cur, at, list]);
  useHotkeys('d', () => cur && go({ name: 'edit', id: cur.id }), [cur]);
  // R starts the next Round once this one is done (all Stacks as rows is not built, so R is free).
  useHotkeys('r', () => canNextRef.current && nextRef.current(), []);
  const leave = () => {
    if (unflagged) dispatch({ type: 'toast', text: `${unflagged} ${unflagged === 1 ? 'Photo is' : 'Photos are'} still ${pass > 1 ? `to keep or cut in Round ${pass}` : 'unflagged'}. ⌘2 brings you back to them.` });
    go({ name: 'cull' });
  };
  useHotkeys(['escape', 'g'], leave, ESC_OUTSIDE_DIALOGS, [unflagged]);

  const total = list.length;
  const finished = at >= total && unflagged === 0;
  // An Undo after the last moment reopens one: go back to it rather than show an empty view.
  useEffect(() => {
    if (at >= total && unflagged > 0) setAt(next(0));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [at, total, unflagged]);
  const first = shown[0];
  const pageNote = span === 1 && book.length > 1 ? (finals ? ' · your Picks from it' : ` · page ${page + 1} of ${book.length}`) : '';
  const round = pass > 1 ? `Round ${pass} · ` : '';
  const title = finished || !total ? (pass > 1 ? `Round ${pass}` : 'Cull') : `${round}${span > 1 ? `Moments ${at + 1} to ${at + span} together` : `${label(first, at + 1)} of ${total}${pageNote}`}`;
  // The next Round goes over this one's keepers (Round 1's are your Picks).
  const keepers = imported.filter((p) => roundIn(state, p.id).round >= pass).length;
  const nextRound = () => go({ name: 'moments', pass: pass + 1 });
  const canNext = finished && keepers >= 2;
  const best = first?.kind === 'stack' ? PHOTO_BY_ID[first.photoIds[0]] : undefined;
  const pickedCount = frames.filter((p) => (pass > 1 ? markOf(p.id) === 'pick' : verdictOf(state, p.id) === 'pick')).length;

  canNextRef.current = canNext;
  nextRef.current = nextRound;

  const cell = (p: Photo, m: Moment, i: number, size: 'full' | 'thumb') => {
    const v = verdictOf(state, p.id);
    const r = starsOf(state, p.id);
    // Your flag, told apart at a glance (Greg, 2026-11-01): a Pick gets a thick blue ring and a
    // solid badge, a Reject fades with a red cross, and a frame you haven't flagged says so.
    const mine = markOf(p.id);
    return (
      <figure
        key={p.id}
        className={`burst-cell ${p.id === cur?.id ? 'focus' : ''} ${mine === 'reject' ? 'faded' : ''} ${mine === 'pick' ? 'kept' : ''} ${mine ? '' : 'needs'}`}
        style={{ '--ar': photoRatio(p) } as React.CSSProperties}
        onClick={() => setFocusId(p.id)}
        onDoubleClick={() => zoom(p.id)}
        data-testid={`burst-${p.id}`}
        data-verdict={v ?? 'unflagged'}
      >
        <PhotoView photo={p} size={size} recipe={state.recipes[p.id]} look={state.look} edits />
        {mine && <span className={`cell-flag ${mine}`}>{mine === 'pick' ? <Flag size={12} strokeWidth={2.4} fill="currentColor" /> : <X size={13} strokeWidth={2.8} />}</span>}
        <figcaption>
          {/* Until you flag it, the AI's Verdict is only a suggestion and reads as one. */}
          <span className={`v-chip ${mine ?? 'needs'}`} title={mine ? undefined : pass > 1 ? `Not marked in Round ${pass} yet: Z keeps, X cuts` : 'Not flagged by you yet: Z keeps, X rejects'}>
            {mine === 'pick' && <Flag size={10} strokeWidth={2.2} fill="currentColor" />}
            {mine === 'reject' && <X size={10} strokeWidth={2.6} />}
            {mine ? markWord[mine] : pass > 1 ? 'Keep or cut' : v === 'pick' || v === 'reject' ? `Needs a flag · AI ${VERDICT_LABEL[v].toLowerCase()}` : 'Needs a flag'}
          </span>
          {m.kind === 'stack' && i === 0 && (
            <span className="v-chip accent" title="Best frame of this Stack: the AI cull's pick, else the sharpest frame with eyes open">
              <Sparkles size={10} /> Best
            </span>
          )}
          {size === 'full' && <span className="mono faint trunc">{p.file}</span>}
          <span className="grow" />
          <Stars n={r.stars} you={r.you} onSet={(n) => stars(p.id, n)} size={10} />
        </figcaption>
      </figure>
    );
  };

  return (
    <div className="cull-mode" data-testid="bursts">
      <nav className="moment-rail" aria-label="Moments" data-testid="moment-rail">
        <div className="rail-head">
          <span>{pass > 1 ? `Round ${pass}` : 'Moments'}</span>
          <span className="mono faint" data-testid="bursts-progress-count">
            {doneCount} of {total} done
          </span>
        </div>
        <span className="burst-progress rail-bar" title={`${doneCount} of ${total} moments done`} data-testid="bursts-progress">
          <span style={{ width: `${total ? (doneCount / total) * 100 : 0}%` }} />
        </span>
        <div className="rail-list">
          {list.map((m, i) => {
            const cover = PHOTO_BY_ID[m.photoIds[0]];
            const kept = m.photoIds.filter((id) => (pass > 1 ? markOf(id) === 'pick' : verdictOf(state, id) === 'pick')).length;
            const left = m.photoIds.filter((id) => !settled(id)).length;
            const on = i >= at && i < at + span;
            return (
              <button key={m.id} className={`moment-row ${on ? 'on' : ''}`} onClick={() => (setSpan(1), setAt(i))} ref={on && i === at ? keepInView : undefined}>
                <PhotoView photo={cover} size="tile" edits style={{ width: 44, height: 30, borderRadius: 4, flex: 'none' }} />
                <span className="moment-text">
                  <span className="trunc">{label(m, i + 1)}</span>
                  <span className="faint">{m.kind === 'stack' ? `${m.photoIds.length} frames` : m.photoIds.length === 1 ? '1 frame' : `${m.photoIds.length} frames, no burst`}</span>
                </span>
                <span className={`mono moment-state ${!left ? 'done' : left < m.photoIds.length ? 'warn' : 'faint'}`} title={left ? `${left} unflagged` : undefined}>
                  {!left ? (kept ? `kept ${kept}` : 'none') : left < m.photoIds.length ? `${left} left` : m.photoIds.length}
                </span>
              </button>
            );
          })}
        </div>
        <div className="rail-foot" data-testid="unflagged">
          {unflagged ? (
            <>
              <span className="warn-dot" />
              <span className="grow">
                <span className="mono">{unflagged}</span> {unflagged === 1 ? 'Photo' : 'Photos'} {pass > 1 ? 'to keep or cut' : 'unflagged'}
              </span>
              <Button variant="ghost" size="sm" onClick={() => (setSpan(1), setAt(next(0)))} data-testid="next-unflagged">
                Go to next
              </Button>
            </>
          ) : (
            <span className="faint">{pass > 1 ? `Every Photo of Round ${pass} is marked` : 'Every Photo is flagged'}</span>
          )}
        </div>
      </nav>
      <div className="panel main view" style={{ flexDirection: 'column', borderRadius: 0, border: 'none', background: 'var(--bg)' }}>
        <header className="panel-head view-head">
          <ModeSwitch mode="cull" />
          <span className="vsep" />
          <span className="view-file">{title}</span>
          {!finished && cur && <span className="mono faint view-meta">{cur.file}</span>}
          <span className="grow" />
          {!finished && (
            <>
              <Tip label="Show the next moment too, as another row" keys="⇧↓">
                <Button variant="ghost" size="sm" onClick={more} disabled={at + span >= total} data-testid="show-next">
                  <Layers size={12} /> Show next too
                </Button>
              </Tip>
              {canMerge && (
                <Tip label="One burst the AI split: make these Stacks one" keys="⇧M">
                  <Button variant="ghost" size="sm" onClick={() => void merge()} data-testid="merge-stacks">
                    <Merge size={12} /> Merge into one Stack
                  </Button>
                </Tip>
              )}
            </>
          )}
          {canNext && (
            <Tip label={`Restack your ${keepers} keepers and go through them again`} keys="R">
              <Button variant="ghost" size="sm" onClick={nextRound} data-testid="next-round-head">
                <Repeat size={12} /> Round {pass + 1}
              </Button>
            </Tip>
          )}
        </header>
        {!total || finished ? (
          <div className="burst-empty">
            <EmptyState
              art={total ? 'culled' : 'tray'}
              title={total ? (pass > 1 ? `Round ${pass} done: ${keepers} of ${list.reduce((a, m) => a + m.photoIds.length, 0)} kept` : `All ${total} moments culled`) : pass > 1 ? `Nothing to go through in Round ${pass}` : 'Nothing to cull yet'}
              testId="bursts-done"
            >
              {!total && (pass > 1 ? `Round ${pass} runs over Round ${pass - 1}'s keepers, and there are none yet.` : 'Import a Shoot first.')}
              {canNext ? (
                <>
                  <span>{pass > 1 ? `Go through your ${keepers} keepers again, restacked into moments.` : `Go through your ${keepers} Picks again, restacked into moments. Cuts stay Picks.`}</span>
                  <span className="row" style={{ gap: 8 }}>
                    <Button size="sm" onClick={nextRound} data-testid="next-round">
                      <Repeat size={12} /> Start Round {pass + 1} <Kbd>R</Kbd>
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => go({ name: 'cull' })}>
                      Back to the Library <Kbd>Esc</Kbd>
                    </Button>
                  </span>
                </>
              ) : (
                <Button size="sm" onClick={() => go({ name: 'cull' })}>
                  Back to the Library <Kbd>Esc</Kbd>
                </Button>
              )}
            </EmptyState>
          </div>
        ) : (
          <div className="burst-body">
            {span === 1 ? (
              <div className="burst-grid" ref={box.ref} style={fitStyle} data-testid="survey-grid" data-page={finals ? 'finals' : page} data-pages={book.length}>
                {onScreen.map((p) => cell(p, first, first.photoIds.indexOf(p.id), g.width > 420 ? 'full' : 'thumb'))}
              </div>
            ) : (
              <div className="moment-rows" data-testid="moment-rows" style={{ '--cols': Math.min(6, Math.max(3, ...shown.map((m) => m.photoIds.length))) } as React.CSSProperties}>
                {shown.map((m, r) => (
                  <section key={m.id} className={`moment-band ${cur && rowOf(cur.id) === r ? 'on' : ''}`}>
                    <div className="moment-band-head">
                      <span>{label(m, at + r + 1)}</span>
                      <span className="mono faint">{m.photoIds.length === 1 ? '1 frame' : `${m.photoIds.length} frames`}</span>
                    </div>
                    <div className="moment-band-frames">{m.photoIds.map((id, i) => PHOTO_BY_ID[id] && cell(PHOTO_BY_ID[id], m, i, 'thumb'))}</div>
                  </section>
                ))}
              </div>
            )}
            {span === 1 && best?.reason && (
              <div className="ai-bar" data-testid="bursts-ai">
                <Sparkles size={12} />
                <span>
                  {aiName()} ranks {best.file} best: {best.reason.charAt(0).toLowerCase() + best.reason.slice(1)}
                </span>
                <span className="grow" />
                <Button size="sm" onClick={keep}>
                  Keep {view.some((p) => yourPick(p.id)) ? (pass > 1 ? 'what I kept' : 'my Picks') : 'this frame'}, {pass > 1 ? 'cut' : 'reject'} the rest{book.length > 1 && !finals && page < book.length - 1 ? ', next page' : ''} <Kbd>↵</Kbd>
                </Button>
              </div>
            )}
          </div>
        )}
        <StatusFoot className="view-foot">
          <span>{frames.length ? `${frames.length} ${frames.length === 1 ? 'frame' : 'frames'} · ${pickedCount} kept` : ''}</span>
          <span className="grow" />
          <Hints items={[['← →', 'Frame'], ['Z', 'Keep'], ['X', pass > 1 ? 'Cut' : 'Reject'], ['↵', pass > 1 ? 'Keep, cut rest' : 'Keep, reject rest'], ['⇧↓', 'Show next too'], [span > 1 ? '↑ ↓' : 'Tab', span > 1 ? 'Row' : 'Moment'], ['Space', 'Loupe'], ['Esc', 'Library']]} />
        </StatusFoot>
      </div>
    </div>
  );
}
