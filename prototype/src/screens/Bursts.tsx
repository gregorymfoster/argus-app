// Burst culling: one Stack at a time, every frame side by side (Lightroom's Survey), so
// keeping the one or two best frames of each burst is a few keys. Focus starts on the AI's
// best frame; Enter keeps it (or the frames you picked with P) and moves to the next Stack.

import { useEffect, useMemo, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Flag, Sparkles, X } from 'lucide-react';
import { PHOTOS, PHOTO_BY_ID, STACKS } from '../data/dummy';
import type { Photo, Verdict } from '../domain/types';
import { starsOf, useLibrary, verdictOf, type Action } from '../store/library';
import { go } from '../router';
import { PhotoView } from '../ui/PhotoView';
import { Stars } from '../ui/Stars';
import { Button } from '@/components/ui/button';
import { Kbd, Seg, VERDICT_KEYS, VERDICT_LABEL, ESC_OUTSIDE_DIALOGS } from '../ui/kit';
import { BackToGrid, Hints, QueueMeter, ViewSwitch } from '../ui/ViewBar';
import { aiName } from '../data/live';

/** What Enter does with the frames you didn't keep. */
type Rest = 'unflagged' | 'reject';
const REST_KEY = 'argus.burstRest';
const readRest = (): Rest => {
  try {
    return localStorage.getItem(REST_KEY) === 'reject' ? 'reject' : 'unflagged';
  } catch {
    return 'unflagged';
  }
};

/** Survey layout: columns and rows that keep every frame as big as the view allows. */
function layout(n: number) {
  const cols = n <= 4 ? n : n <= 6 ? 3 : n <= 8 ? 4 : n <= 10 ? 5 : 6;
  return { '--cols': cols, '--rows': Math.ceil(n / cols) } as React.CSSProperties;
}

/** Stacks kept in this view this session. Live, the person's own Verdicts also say so (the
 *  dummy store forgets a Verdict that matches the AI's). */
const keptStacks = new Set<string>();
/** Frames you picked here (same reason). */
const pickedHere = new Set<string>();

const byRank = (a: Photo, b: Photo) => (a.stackRank ?? 99) - (b.stackRank ?? 99);

export function BurstsScreen({ start }: { start?: string }) {
  const { state, dispatch } = useLibrary();
  const imported = useMemo(() => new Set(PHOTOS.slice(0, state.imported).map((p) => p.id)), [state.imported]);
  // Every Stack of the Shoot in capture order, frames best first.
  const stacks = useMemo(
    () =>
      STACKS.map((s) => ({ id: s.id, frames: s.photoIds.filter((id) => imported.has(id)).map((id) => PHOTO_BY_ID[id]).sort(byRank) })).filter((s) => s.frames.length > 1),
    [imported, state.imported],
  );
  // A Stack is done once you've kept frames from it, or flagged any of its frames yourself.
  const isDone = (s: { id: string; frames: Photo[] }) => keptStacks.has(s.id) || s.frames.some((p) => p.id in state.overrides);
  const done = (i: number) => !!stacks[i] && isDone(stacks[i]);
  const doneCount = stacks.filter((_, i) => done(i)).length;
  const [at, setAt] = useState(() => {
    const i = stacks.findIndex((s) => s.id === start);
    if (i >= 0) return i;
    const open = stacks.findIndex((s) => !isDone(s));
    return Math.max(0, open);
  });
  const stack = stacks[Math.min(at, stacks.length - 1)];
  const frames = stack?.frames ?? [];
  const [focus, setFocus] = useState(0);
  // Each Stack opens on the frame you kept, else the AI's best.
  useEffect(() => {
    const kept = frames.findIndex((p) => yourPick(p.id));
    setFocus(Math.max(0, kept));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stack?.id]);
  const [rest, setRestState] = useState<Rest>(readRest);
  const setRest = (r: Rest) => {
    setRestState(r);
    // Hand the arrow keys back to the frames.
    (document.activeElement as HTMLElement | null)?.blur();
    try {
      localStorage.setItem(REST_KEY, r);
    } catch {
      /* private window: the choice lasts this session */
    }
  };

  const cur = frames[Math.min(focus, frames.length - 1)];
  const set = (id: string, v: Verdict) => {
    if (v === 'pick') pickedHere.add(id);
    else pickedHere.delete(id);
    dispatch({ type: 'verdict/set', id, verdict: v, by: 'you' });
  };
  const yourPick = (id: string) => verdictOf(state, id) === 'pick' && (state.overrides[id] === 'pick' || pickedHere.has(id));
  const stars = (id: string, n: number) => dispatch({ type: 'stars/set', id, stars: n, by: 'you' });

  // Enter: keep the frames you picked here (none yet: the focused one), drop the AI's other
  // Picks, give the rest the Rest verdict, then go to the next Stack you haven't done.
  const keep = () => {
    if (!stack || !cur) return;
    const yours = frames.filter((p) => yourPick(p.id)).map((p) => p.id);
    const kept = new Set(yours.length ? yours : state.overrides[cur.id] === 'reject' ? [] : [cur.id]);
    const actions: Action[] = [];
    for (const p of frames) {
      const v = verdictOf(state, p.id) ?? 'unflagged';
      const target: Verdict = kept.has(p.id) ? 'pick' : v === 'reject' ? 'reject' : rest;
      if (!(p.id in state.overrides && v === target)) actions.push({ type: 'verdict/set', id: p.id, verdict: target, by: 'you' });
    }
    dispatch({ type: 'batch', label: `Kept ${kept.size} of ${frames.length}`, actions });
    keptStacks.add(stack.id);
    const next = stacks.findIndex((s, i) => i > at && !isDone(s));
    if (next >= 0) setAt(next);
    else if (at < stacks.length - 1) setAt(at + 1);
    else setAt(stacks.length);
  };
  const step = (d: number) => setAt((i) => Math.max(0, Math.min(stacks.length - 1, i + d)));

  useHotkeys(['right', 'l'], () => setFocus((f) => Math.min(frames.length - 1, f + 1)), [frames]);
  useHotkeys(['left', 'h'], () => setFocus((f) => Math.max(0, f - 1)), [frames]);
  useHotkeys(['down', 'j', 'tab'], () => step(1), { preventDefault: true }, [stacks]);
  useHotkeys(['up', 'k', 'shift+tab'], () => step(-1), { preventDefault: true }, [stacks]);
  useHotkeys(['p', 'x', 'u'], (e) => cur && set(cur.id, VERDICT_KEYS[e.key.toLowerCase()]), [cur]);
  useHotkeys(['0', '1', '2', '3', '4', '5'], (e) => cur && stars(cur.id, Number(e.key)), [cur]);
  useHotkeys('enter', keep, { preventDefault: true }, [stack, cur, state, rest, at]);
  useHotkeys(['z', 'space'], () => cur && go({ name: 'loupe', id: cur.id }), { preventDefault: true }, [cur]);
  useHotkeys('d', () => cur && go({ name: 'edit', id: cur.id }), [cur]);
  useHotkeys(['escape', 'g'], () => go({ name: 'cull' }), ESC_OUTSIDE_DIALOGS);

  const total = stacks.length;
  const pct = total ? (doneCount / total) * 100 : 0;
  const finished = at >= total;
  const best = frames[0];

  return (
    <div className="panel main view" style={{ flexDirection: 'column', borderRadius: 0, border: 'none', background: 'var(--bg)' }} data-testid="bursts">
      <header className="panel-head view-head">
        <BackToGrid />
        <span className="view-file">{finished || !total ? 'Stacks' : `Stack ${at + 1} of ${total}`}</span>
        {!finished && cur && <span className="mono faint view-meta">{cur.file}</span>}
        <span className="burst-progress" title={`${doneCount} of ${total} Stacks done`} data-testid="bursts-progress">
          <span style={{ width: `${pct}%` }} />
        </span>
        <span className="mono faint view-meta">
          {doneCount}/{total} done
        </span>
        <span className="grow" />
        <span className="faint view-meta wide-only">The rest</span>
        <Seg
          quiet
          label="What Enter does with the frames you don't keep"
          value={rest}
          onChange={setRest}
          items={[
            { value: 'unflagged', label: 'Unflagged', title: 'Leave the frames you don’t keep Unflagged' },
            { value: 'reject', label: 'Rejected', title: 'Reject the frames you don’t keep' },
          ]}
        />
        <ViewSwitch view="bursts" id={cur?.id} compareIds={frames.map((p) => p.id)} />
        <QueueMeter />
      </header>
      {!total || finished ? (
        <div className="burst-empty" data-testid="bursts-done">
          <span>{total ? `All ${total} Stacks culled.` : 'No Stacks in this Shoot yet. Bursts stack themselves once Import has looked at the Photos, or after an AI cull.'}</span>
          <Button size="sm" onClick={() => go({ name: 'cull' })}>
            Back to the grid <Kbd>Esc</Kbd>
          </Button>
        </div>
      ) : (
        <div className="burst-body">
          <div className="burst-grid" style={layout(frames.length)}>
            {frames.map((p, i) => {
              const v = verdictOf(state, p.id);
              const r = starsOf(state, p.id);
              return (
                <figure
                  key={p.id}
                  className={`burst-cell ${i === focus ? 'focus' : ''} ${v === 'reject' ? 'faded' : ''} ${v === 'pick' ? 'kept' : ''}`}
                  onClick={() => setFocus(i)}
                  onDoubleClick={() => go({ name: 'loupe', id: p.id })}
                  data-testid={`burst-${p.id}`}
                  data-verdict={v ?? 'unflagged'}
                >
                  <PhotoView photo={p} size={frames.length <= 4 ? 'full' : 'thumb'} recipe={state.recipes[p.id]} look={state.look} edits />
                  <figcaption>
                    <span className={`v-chip ${v === 'pick' || v === 'reject' ? v : ''}`}>
                      {v === 'pick' && <Flag size={10} strokeWidth={2.2} />}
                      {v === 'reject' && <X size={10} strokeWidth={2.6} />}
                      {VERDICT_LABEL[v ?? 'unflagged']}
                    </span>
                    {i === 0 && p.stackRank === 0 && (
                      <span className="v-chip accent" title="Best frame of this Stack: the AI cull's pick, else the sharpest frame with eyes open">
                        <Sparkles size={10} /> Best
                      </span>
                    )}
                    <span className="grow" />
                    <Stars n={r.stars} you={r.you} onSet={(n) => stars(p.id, n)} size={10} />
                  </figcaption>
                </figure>
              );
            })}
          </div>
          {best?.reason && (
            <div className="ai-bar" data-testid="bursts-ai">
              <Sparkles size={12} />
              <span>
                {aiName()} ranks {best.file} best: {best.reason.charAt(0).toLowerCase() + best.reason.slice(1)}
              </span>
              <span className="grow" />
              <Button size="sm" onClick={keep}>
                Keep {frames.some((p) => yourPick(p.id)) ? 'my Picks' : 'this frame'} <Kbd>↵</Kbd>
              </Button>
            </div>
          )}
        </div>
      )}
      <footer className="view-foot">
        <span>{frames.length ? `${frames.length} frames · ${frames.filter((p) => verdictOf(state, p.id) === 'pick').length} kept` : ''}</span>
        <span className="grow" />
        <Hints items={[['← →', 'Frame'], ['P X U', 'Flag'], ['1–5', 'Stars'], ['↵', 'Keep & next Stack'], ['↑ ↓', 'Stack'], ['Z', 'Loupe'], ['Esc', 'Grid']]} />
      </footer>
    </div>
  );
}
