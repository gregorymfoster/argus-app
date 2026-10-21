// Lightroom's Compare view: the Select (white frame) beside a Candidate (terracotta), so
// the best of a burst is one decision at a time. ← → bring up the next Candidate, ↑ makes
// the Candidate the Pick and the Select, and the AI says which of the two it prefers.

import { useEffect, useMemo, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { Flag, Sparkles, X } from 'lucide-react';
import { PHOTO_BY_ID } from '../data/dummy';
import type { Photo, Verdict } from '../domain/types';
import { starsOf, useLibrary, verdictOf } from '../store/library';
import { go } from '../router';
import { PhotoView } from '../ui/PhotoView';
import { Stars } from '../ui/Stars';
import { Button } from '@/components/ui/button';
import { Kbd, VERDICT_KEYS, VERDICT_LABEL, ESC_OUTSIDE_DIALOGS } from '../ui/kit';
import { BackToGrid, Hints, QueueMeter, ViewSwitch } from '../ui/ViewBar';
import { LIVE, aiName } from '../data/live';

/** Best first: a cull's Stack rank, then (dummy Photos) the sharper frame. */
const byRank = (a: Photo, b: Photo) => (a.stackRank ?? 99) - (b.stackRank ?? 99) || b.signals.focus - a.signals.focus;

/** Why the AI prefers `a` over `b`, in a few words. */
function because(a: Photo, b: Photo): string {
  if (LIVE) return a.reason ? a.reason.charAt(0).toLowerCase() + a.reason.slice(1) : 'it ranks higher in the Stack';
  const out: string[] = [];
  if (a.signals.focus - b.signals.focus > 0.05) out.push(`sharper (${a.signals.focus.toFixed(2)} vs ${b.signals.focus.toFixed(2)})`);
  const shut = (p: Photo) => p.signals.eyesOpen[1] - p.signals.eyesOpen[0];
  if (shut(a) < shut(b)) out.push(shut(a) === 0 ? 'nobody is mid-blink' : 'fewer closed eyes');
  return out.join(' and ') || a.reason.toLowerCase() || 'it ranks higher in the Stack';
}

export function CompareScreen({ ids }: { ids: string[] }) {
  const { state, dispatch } = useLibrary();
  const photos = useMemo(() => ids.map((id) => PHOTO_BY_ID[id]).filter(Boolean), [ids]);
  // The engine has no Signals yet, so in the app "best" comes only from a cull's Stack ranks.
  const ranked = !LIVE || photos.some((p) => p.stackRank != null);
  const order = useMemo(() => (ranked ? [...photos].sort(byRank) : photos), [photos, ranked]);
  const best = order[0];
  // The Select starts as the frame you already picked, else the best one; the Candidate as the next best.
  const [selId, setSelId] = useState(() => (photos.find((p) => verdictOf(state, p.id) === 'pick') ?? best)?.id);
  const [candId, setCandId] = useState(() => order.find((p) => p.id !== selId)?.id ?? selId);
  const [dismissed, setDismissed] = useState(false);
  useEffect(() => setDismissed(false), [selId, candId]);
  const sel = PHOTO_BY_ID[selId!] ?? photos[0];
  const cand = PHOTO_BY_ID[candId!] ?? photos[1] ?? photos[0];
  const others = order.filter((p) => p.id !== sel?.id);

  const set = (id: string, v: Verdict) => dispatch({ type: 'verdict/set', id, verdict: v, by: 'you' });
  const nextCandidate = (d: number) => {
    if (others.length < 2) return;
    const i = others.findIndex((p) => p.id === cand.id);
    setCandId(others[(i + d + others.length) % others.length].id);
  };
  // ↑: the Candidate becomes the Pick and the Select; the old Select is unflagged and becomes the Candidate.
  const promote = () => {
    if (!cand || cand.id === sel.id) return;
    const was = verdictOf(state, sel.id);
    dispatch({
      type: 'batch',
      label: `${cand.file} is the Pick`,
      actions: [{ type: 'verdict/set', id: cand.id, verdict: 'pick', by: 'you' }, ...(was === 'pick' ? [{ type: 'verdict/set' as const, id: sel.id, verdict: 'unflagged' as const, by: 'you' as const }] : [])],
    });
    setSelId(cand.id);
    setCandId(sel.id);
  };
  const keepSelect = () => {
    if (verdictOf(state, sel.id) !== 'pick') set(sel.id, 'pick');
    setDismissed(true);
  };
  // K: keep the Select, reject the rest of the Stack.
  const keepOnly = () =>
    dispatch({ type: 'batch', label: `Kept 1 of ${photos.length}`, actions: photos.map((p) => ({ type: 'verdict/set', id: p.id, verdict: p.id === sel.id ? 'pick' : 'reject', by: 'you' })) });
  // P / X / U judge the Candidate; a Reject moves on to the next one.
  const flag = (v: Verdict) => {
    if (!cand) return;
    set(cand.id, v);
    if (v === 'reject') nextCandidate(1);
  };

  useHotkeys('right', () => nextCandidate(1), [others, cand]);
  useHotkeys('left', () => nextCandidate(-1), [others, cand]);
  useHotkeys('up', promote, [sel, cand, state]);
  useHotkeys(['p', 'x', 'u'], (e) => flag(VERDICT_KEYS[e.key.toLowerCase()]), [cand, others]);
  useHotkeys('k', keepOnly, [sel, photos]);
  useHotkeys(['escape', 'g'], () => go({ name: 'cull' }), ESC_OUTSIDE_DIALOGS);
  useHotkeys(['e', 'enter'], () => cand && go({ name: 'loupe', id: cand.id }), [cand]);
  useHotkeys('d', () => cand && go({ name: 'edit', id: cand.id }), [cand]);

  if (!sel || !cand) return null;
  const prefers = ranked && !dismissed && best && best.id === cand.id && cand.id !== sel.id;
  const inStack = photos.every((p) => p.stackId && p.stackId === photos[0].stackId);

  return (
    <div className="panel main view" style={{ flexDirection: 'column', borderRadius: 0, border: 'none', background: 'var(--bg)' }}>
      <header className="panel-head view-head">
        <BackToGrid />
        <span className="mono view-file">{sel.file}</span>
        <span className="mono faint view-meta">{inStack ? `Stack of ${photos.length}` : `Comparing ${photos.length}`}</span>
        <span className="mono faint view-meta trunc">{sel.exif}</span>
        <span className="grow" />
        <ViewSwitch view="compare" id={cand.id} compareIds={ids} />
        <QueueMeter />
      </header>
      <div className="compare-body">
        <div className="compare-pair">
          <Cell p={sel} role="select" onOpen={() => go({ name: 'loupe', id: sel.id })} />
          <Cell p={cand} role="candidate" onOpen={() => go({ name: 'loupe', id: cand.id })} />
        </div>
        {prefers && (
          <div className="ai-bar" data-testid="compare-ai">
            <Sparkles size={12} />
            <span>
              {aiName()} prefers the Candidate: {because(cand, sel)}.
            </span>
            <span className="grow" />
            <Button size="sm" onClick={promote}>
              Make it the Pick <Kbd>↑</Kbd>
            </Button>
            <Button size="sm" variant="ghost" onClick={keepSelect}>
              Keep Select
            </Button>
          </div>
        )}
        <div className="stack-row" aria-label="Frames">
          <span className="stack-label">
            {inStack ? 'Stack' : 'Comparing'} · {photos.length} frames
          </span>
          {photos.map((p) => {
            const v = verdictOf(state, p.id);
            const cls = [p.id === sel.id ? 'select' : p.id === cand.id ? 'candidate' : '', v === 'reject' ? 'rejected' : ''].join(' ');
            return (
              <button key={p.id} className={cls} onClick={() => p.id !== sel.id && setCandId(p.id)} onDoubleClick={() => go({ name: 'loupe', id: p.id })} aria-label={p.file} data-testid={`survey-${p.id}`} data-verdict={v ?? ''}>
                <PhotoView photo={p} recipe={state.recipes[p.id]} look={state.look} edits style={{ width: '100%', height: '100%', borderRadius: 0 }} />
              </button>
            );
          })}
        </div>
      </div>
      <footer className="view-foot">
        <span>White frame is the Select, terracotta the Candidate</span>
        <span className="grow" />
        <Hints items={[['← →', 'Next candidate'], ['↑', 'Swap'], ['P', 'Pick'], ['X', 'Reject'], ['K', 'Keep only the Select'], ['Esc', 'Grid']]} />
      </footer>
    </div>
  );
}

function Cell({ p, role, onOpen }: { p: Photo; role: 'select' | 'candidate'; onOpen: () => void }) {
  const { state, dispatch } = useLibrary();
  const v = verdictOf(state, p.id);
  const rating = starsOf(state, p.id);
  return (
    <figure className={`compare-cell ${role} ${v === 'reject' ? 'faded' : ''}`} data-testid={`compare-${role}`}>
      <div className="well" onDoubleClick={onOpen}>
        <PhotoView photo={p} size="full" recipe={state.recipes[p.id]} look={state.look} edits>
          <span className="lbl">{role === 'select' ? 'Select' : 'Candidate'}</span>
        </PhotoView>
      </div>
      <figcaption>
        <span className={`v-chip ${v === 'pick' || v === 'reject' ? v : ''}`}>
          {v === 'pick' && <Flag size={10} strokeWidth={2.2} />}
          {v === 'reject' && <X size={10} strokeWidth={2.6} />}
          {VERDICT_LABEL[v ?? 'unflagged']}
        </span>
        <Stars n={rating.stars} you={rating.you} onSet={(n) => dispatch({ type: 'stars/set', id: p.id, stars: n, by: 'you' })} size={11} />
        {!LIVE && p.signals.eyesOpen[1] > 0 && (
          <span className="mono">
            eyes {p.signals.eyesOpen[0]}/{p.signals.eyesOpen[1]} · focus {p.signals.focus.toFixed(2)}
          </span>
        )}
        <span className="reason">{p.reason}</span>
      </figcaption>
    </figure>
  );
}
