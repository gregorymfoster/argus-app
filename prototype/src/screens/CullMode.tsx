// Cull mode (⌘2, mocked as MO-Cull and MO-Cull-Together, Greg 2026-11-01): the Shoot one
// moment at a time, every frame of it side by side (Lightroom's Survey). A moment is a Stack,
// best frame first, or a short run of single frames, so one pass decides every Photo. Z keeps,
// X rejects, ↵ keeps and moves on. ⇧↓ pulls the next moment in as another row, to keep the best
// across a couple of Stacks or merge Stacks the AI split (⇧M). Every ↵ is logged as bench data.

import { useEffect, useMemo, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Flag, Layers, Merge, Sparkles, X } from 'lucide-react';
import { PHOTOS, PHOTO_BY_ID } from '../data/dummy';
import type { Photo, Verdict } from '../domain/types';
import { fitGrid, keptOnEnter, moments, type Moment } from '../domain/moments';
import { logMoment, mergeStacks } from '../data/moments';
import { starsOf, useLibrary, verdictOf, type Action } from '../store/library';
import { go, openFrom } from '../router';
import { PhotoView, photoRatio } from '../ui/PhotoView';
import { Stars } from '../ui/Stars';
import { EmptyState } from '../ui/Empty';
import { Button } from '@/components/ui/button';
import { Kbd, Tip, VERDICT_LABEL, ESC_OUTSIDE_DIALOGS } from '../ui/kit';
import { Hints, ModeSwitch, QueueMeter } from '../ui/ViewBar';
import { aiName } from '../data/live';

/** What ↵ does with the frames of a Stack you didn't keep. Single frames you pass stay Unflagged. */
/** Survey layout: the column count that shows every frame biggest in the space there is. */
function useFit(n: number, aspect: number) {
  const [box, setBox] = useState({ w: 0, h: 0 });
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setBox({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(el);
    return () => ro.disconnect();
  }, [el]);
  const g = fitGrid(n, box.w, box.h, 8, 25, aspect);
  const style = { '--cols': g.cols, '--rows': g.rows, ...(g.width ? { '--cell-w': `${g.width}px`, '--cell-h': `${Math.floor(g.width / aspect)}px` } : {}) } as React.CSSProperties;
  return { ref: setEl, style, width: g.width };
}

/** Frames you flagged here this session (the dummy store forgets a Verdict that matches the AI's). */
const flaggedHere = new Set<string>();
/** Frames you picked here (same reason). */
const pickedHere = new Set<string>();

/** What the bench gets for the moment in hand: when it opened, what had focus, what was zoomed.
 *  Module-level, so a trip to the Loupe (Space) and back keeps it. */
let track: { key: string; opened: number; looked: string[]; zoomed: Set<string>; mergedFrom?: string[][] } | null = null;

const label = (m: Moment, n: number) => (m.kind === 'stack' ? `Stack ${n}` : m.photoIds.length === 1 ? `Single ${n}` : `Singles ${n}`);

export function CullModeScreen({ start }: { start?: string }) {
  const { state, dispatch } = useLibrary();
  const [rev, setRev] = useState(0); // bumps after a merge re-stacks the Shoot
  const list = useMemo(
    () => moments(PHOTOS.slice(0, state.imported)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [state.imported, rev],
  );
  // Cull makes you flag every Photo (Greg, 2026-11-01): one is settled once you Picked or Rejected
  // it yourself, and a moment is done only when all its frames are. An AI Verdict alone isn't yours.
  const settled = (id: string) => {
    const o = state.overrides[id];
    return o === 'pick' || o === 'reject' || (flaggedHere.has(id) && (verdictOf(state, id) ?? 'unflagged') !== 'unflagged');
  };
  const isDone = (m: Moment) => m.photoIds.every(settled);
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
  const fit = useFit(frames.length, Math.max(0.5, ...frames.map(photoRatio)));
  const [focusId, setFocusId] = useState<string | undefined>();
  const cur = frames.find((p) => p.id === focusId) ?? frames[0];
  const rowOf = (id: string) => shown.findIndex((m) => m.photoIds.includes(id));

  const key = shown.map((m) => m.id).join('|');
  // A new moment starts a new record; adding rows keeps the clock and what you looked at.
  if (shown[0] && track?.key !== shown[0].id) track = { key: shown[0].id, opened: performance.now(), looked: [], zoomed: new Set() };
  useEffect(() => {
    // Each moment opens on the frame you kept, else the AI's best.
    const kept = frames.find((p) => yourPick(p.id));
    setFocusId((kept ?? frames[0])?.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  useEffect(() => {
    if (cur && track && track.looked.at(-1) !== cur.id) track.looked.push(cur.id);
  }, [cur?.id]);

  const set = (id: string, v: Verdict) => {
    if (v === 'pick') pickedHere.add(id);
    else pickedHere.delete(id);
    if (v === 'unflagged') flaggedHere.delete(id);
    else flaggedHere.add(id);
    dispatch({ type: 'verdict/set', id, verdict: v, by: 'you' });
  };
  const yourPick = (id: string) => verdictOf(state, id) === 'pick' && (state.overrides[id] === 'pick' || pickedHere.has(id));
  const toggleKeep = (id: string) => set(id, yourPick(id) ? 'unflagged' : 'pick');
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
    const picked = new Set(frames.filter((p) => yourPick(p.id)).map((p) => p.id));
    const kept = keptOnEnter(shown, picked, cur && state.overrides[cur.id] !== 'reject' ? cur.id : undefined);
    const actions: Action[] = [];
    const rejected: string[] = [];
    for (const m of shown)
      for (const id of m.photoIds) {
        const v = verdictOf(state, id) ?? 'unflagged';
        const target: Verdict = kept.has(id) ? 'pick' : 'reject';
        if (target === 'reject') rejected.push(id);
        flaggedHere.add(id);
        if (target === 'pick') pickedHere.add(id);
        if (!(state.overrides[id] === target && v === target)) actions.push({ type: 'verdict/set', id, verdict: target, by: 'you' });
      }
    dispatch({ type: 'batch', label: `Kept ${kept.size}, rejected ${frames.length - kept.size}`, actions });
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
    });
    track = null;
    const after = at + span;
    setSpan(1);
    // `state` is this render's, so the moments just flagged still read as open: skip them.
    setAt(next(after, new Set(shown.map((m) => m.id))));
  };

  const step = (d: number) => {
    setSpan(1);
    setAt((i) => Math.max(0, Math.min(list.length - 1, i + d)));
  };
  const more = () => setSpan((s) => Math.min(s + 1, list.length - at));
  const fewer = () => setSpan((s) => Math.max(1, s - 1));
  const shownStacks = shown.filter((m) => m.kind === 'stack');
  // Stacks shown together merge, even with single frames between them (they stay single).
  const canMerge = shownStacks.length > 1;
  const merge = async () => {
    if (!canMerge) return;
    const from = shownStacks.map((m) => m.photoIds);
    const into = await mergeStacks(shownStacks.map((m) => m.id));
    track = { ...(track ?? { opened: performance.now(), looked: [], zoomed: new Set() }), key: into, mergedFrom: from };
    const n = shownStacks.reduce((a, m) => a + m.photoIds.length, 0);
    setSpan(1);
    setRev((r) => r + 1);
    dispatch({ type: 'toast', text: `Merged ${shownStacks.length} Stacks into one of ${n}` });
  };

  // Arrows: ← → move within a row; ↑ ↓ move between rows when several show, else between moments.
  const move = (d: number) => {
    if (!cur) return;
    const r = rowOf(cur.id);
    const row = shown[r]?.photoIds ?? [];
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
    openFrom({ name: 'moments', id: list[at]?.id }, { name: 'loupe', id });
  };

  useHotkeys(['right', 'l'], () => move(1), [cur, key]);
  useHotkeys(['left', 'h'], () => move(-1), [cur, key]);
  useHotkeys(['down', 'j'], (e) => (e.preventDefault(), vertical(1)), [cur, key, span]);
  useHotkeys(['up', 'k'], (e) => (e.preventDefault(), vertical(-1)), [cur, key, span]);
  useHotkeys('tab', () => step(1), { preventDefault: true }, [list]);
  useHotkeys('shift+tab', () => step(-1), { preventDefault: true }, [list]);
  useHotkeys('shift+down', more, { preventDefault: true }, [at, list]);
  useHotkeys('shift+up', fewer, { preventDefault: true });
  useHotkeys('shift+m', () => void merge(), [key, canMerge]);
  useHotkeys(['z', 'p'], () => cur && toggleKeep(cur.id), [cur, state]);
  useHotkeys('x', () => cur && set(cur.id, verdictOf(state, cur.id) === 'reject' && cur.id in state.overrides ? 'unflagged' : 'reject'), [cur, state]);
  useHotkeys('u', () => cur && set(cur.id, 'unflagged'), [cur]);
  useHotkeys(['0', '1', '2', '3', '4', '5'], (e) => cur && stars(cur.id, Number(e.key)), [cur]);
  useHotkeys('enter', keep, { preventDefault: true }, [key, cur, state, at, span]);
  useHotkeys('space', () => zoom(), { preventDefault: true }, [cur, at, list]);
  useHotkeys('d', () => cur && go({ name: 'edit', id: cur.id }), [cur]);
  const leave = () => {
    if (unflagged) dispatch({ type: 'toast', text: `${unflagged} ${unflagged === 1 ? 'Photo is' : 'Photos are'} still unflagged. ⌘2 brings you back to them.` });
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
  const title = finished || !total ? 'Cull' : span > 1 ? `Moments ${at + 1} to ${at + span} together` : `${label(first, at + 1)} of ${total}`;
  const best = first?.kind === 'stack' ? PHOTO_BY_ID[first.photoIds[0]] : undefined;
  const pickedCount = frames.filter((p) => verdictOf(state, p.id) === 'pick').length;

  const cell = (p: Photo, m: Moment, i: number, size: 'full' | 'thumb') => {
    const v = verdictOf(state, p.id);
    const r = starsOf(state, p.id);
    return (
      <figure
        key={p.id}
        className={`burst-cell ${p.id === cur?.id ? 'focus' : ''} ${v === 'reject' ? 'faded' : ''} ${v === 'pick' ? 'kept' : ''}`}
        style={{ '--ar': photoRatio(p) } as React.CSSProperties}
        onClick={() => setFocusId(p.id)}
        onDoubleClick={() => zoom(p.id)}
        data-testid={`burst-${p.id}`}
        data-verdict={v ?? 'unflagged'}
      >
        <PhotoView photo={p} size={size} recipe={state.recipes[p.id]} look={state.look} edits />
        <figcaption>
          {/* Until you flag it, the AI's Verdict is only a suggestion and reads as one. */}
          <span className={`v-chip ${settled(p.id) && (v === 'pick' || v === 'reject') ? v : ''}`} title={settled(p.id) ? undefined : 'Not flagged by you yet'}>
            {v === 'pick' && <Flag size={10} strokeWidth={2.2} />}
            {v === 'reject' && <X size={10} strokeWidth={2.6} />}
            {settled(p.id) || !v || v === 'unflagged' ? VERDICT_LABEL[v ?? 'unflagged'] : `AI ${VERDICT_LABEL[v].toLowerCase()}`}
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
          <span>Moments</span>
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
            const kept = m.photoIds.filter((id) => verdictOf(state, id) === 'pick').length;
            const left = m.photoIds.filter((id) => !settled(id)).length;
            const on = i >= at && i < at + span;
            return (
              <button key={m.id} className={`moment-row ${on ? 'on' : ''}`} onClick={() => (setSpan(1), setAt(i))} ref={on && i === at ? (el) => el?.scrollIntoView({ block: 'nearest' }) : undefined}>
                <PhotoView photo={cover} size="tile" edits style={{ width: 44, height: 30, borderRadius: 3, flex: 'none' }} />
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
                <span className="mono">{unflagged}</span> {unflagged === 1 ? 'Photo' : 'Photos'} unflagged
              </span>
              <Button variant="ghost" size="sm" onClick={() => (setSpan(1), setAt(next(0)))} data-testid="next-unflagged">
                Go to next
              </Button>
            </>
          ) : (
            <span className="faint">Every Photo is flagged</span>
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
          <QueueMeter />
        </header>
        {!total || finished ? (
          <div className="burst-empty">
            <EmptyState art={total ? 'culled' : 'tray'} title={total ? `All ${total} moments culled` : 'Nothing to cull yet'} testId="bursts-done">
              {!total && 'Import a Shoot first.'}
              <Button size="sm" onClick={() => go({ name: 'cull' })}>
                Back to the Library <Kbd>Esc</Kbd>
              </Button>
            </EmptyState>
          </div>
        ) : (
          <div className="burst-body">
            {span === 1 ? (
              <div className="burst-grid" ref={fit.ref} style={fit.style} data-testid="survey-grid">
                {first.photoIds.map((id, i) => PHOTO_BY_ID[id] && cell(PHOTO_BY_ID[id], first, i, fit.width > 420 ? 'full' : 'thumb'))}
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
                  Keep {frames.some((p) => yourPick(p.id)) ? 'my Picks' : 'this frame'}, reject the rest <Kbd>↵</Kbd>
                </Button>
              </div>
            )}
          </div>
        )}
        <footer className="view-foot">
          <span>{frames.length ? `${frames.length} frames · ${pickedCount} kept · ${unflagged} unflagged in the Shoot` : ''}</span>
          <span className="grow" />
          <Hints items={[['← →', 'Frame'], ['Z', 'Keep'], ['X', 'Reject'], ['↵', 'Keep, reject rest'], ['⇧↓', 'Show next too'], [span > 1 ? '↑ ↓' : 'Tab', span > 1 ? 'Row' : 'Moment'], ['Space', 'Loupe'], ['Esc', 'Library']]} />
        </footer>
      </div>
    </div>
  );
}
