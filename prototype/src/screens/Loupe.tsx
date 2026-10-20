import { useEffect, useState } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { askRemove } from '../ui/Remove';
import { Award, Flag, X } from 'lucide-react';
import { PHOTOS, PHOTO_BY_ID, SHOOT, stackOf } from '../data/dummy';
import type { Photo, Verdict } from '../domain/types';
import { starsOf, useLibrary, verdictOf } from '../store/library';
import { go } from '../router';
import type { Suggestion } from '../domain/types';
import { PhotoView, usePreview } from '../ui/PhotoView';
import { Filmstrip } from '../ui/Filmstrip';
import { STAR_KEYS, Stars } from '../ui/Stars';
import { LIVE, aiName, focusPhoto } from '../data/live';
import { useSuggestionImage, useSuggestions } from '../data/suggestions';
import { Kbd, Seg, VERDICT_KEYS, VERDICT_LABEL } from '../ui/kit';
import { BackToGrid, Hints, QueueMeter, ViewSwitch } from '../ui/ViewBar';
import { Histogram } from '../ui/Histogram';
import { useSort } from '../ui/Sort';
import { sortPhotos } from '../domain/sort';
import { Agent } from '../agent/AgentPanel';
import { eyeFlagsOf, useEyeFlags, usePhotoFacts, type FaceFacts } from '../data/facts';
import { EyeBadges } from '../ui/EyeBadges';
import { longEdge, useNeed, useZoomPan, useZoomed } from '../ui/zoomPan';
import { ZoomReadout } from '../ui/Navigator';
import { useDetailRender } from '../data/liveEdit';

/** Face boxes (F shows them all; closed eyes always show). Live, from the faces Analyzer's Facts. */
export function Faces({ p, all = true, found }: { p: Photo; all?: boolean; found?: FaceFacts[] }) {
  const boxes = found
    ? found.map((f) => ({ left: f.x, top: f.y, w: f.w, h: f.h, closed: !!f.eyes_closed }))
    : p.faces.map((f) => ({ left: f.x - f.r, top: f.y - f.r * 1.5, w: f.r * 2, h: f.r * 3, closed: f.eyesClosed }));
  return (
    <>
      {boxes.filter((f) => all || f.closed).map((f, i) => (
        <div key={i} className={`facebox ${f.closed ? 'closed' : ''}`} style={{ left: `${f.left * 100}%`, top: `${f.top * 100}%`, width: `${f.w * 100}%`, height: `${f.h * 100}%` }}>
          {f.closed && <span className="tag">Eyes closed</span>}
        </div>
      ))}
    </>
  );
}

export function LoupeScreen({ id }: { id: string }) {
  const { state, dispatch } = useLibrary();
  // Face boxes clutter the photo; F shows them all, and a closed eye always shows.
  const [faces, setFaces] = useState(false);
  const [before, setBefore] = useState(false);
  const [tab, setTab] = useState<'info' | 'agent'>('info');
  const p = PHOTO_BY_ID[id] ?? PHOTOS[0];
  // The grid's sort order, Stacks kept together.
  const [sort] = useSort();
  const order = sortPhotos(PHOTOS.slice(0, state.imported), sort, { stars: (x) => starsOf(state, x).stars, verdict: (x) => verdictOf(state, x) }).map((x) => x.id);
  const pos = order.indexOf(p.id);
  const nextKey = order.slice(pos + 1, pos + 4).join(',');
  useEffect(() => focusPhoto(p.id, nextKey ? nextKey.split(',') : []), [p.id, nextKey]);
  // Lightroom's zoom: pinch, Z, click, Space-drag, ⌘+ / ⌘−; it stays on for the next photo.
  const zp = useZoomPan({ photoKey: p.id, natural: longEdge(p) });
  const zoomed = useZoomed(zp.z);
  const need = useNeed(zp.z);
  const detail = useDetailRender(p.id, null, need, 1536, longEdge(p), LIVE && !before);
  const v = verdictOf(state, p.id);
  const ai = state.aiVerdicts[p.id];
  // Only a real AI Verdict (one a cull wrote) is the AI's suggestion; unflagged is no opinion.
  const hasAi = ai === 'pick' || ai === 'reject';
  const stack = stackOf(p);
  const stackIds = stack.length > 1 ? stack.map((s) => s.id) : [];
  const rating = starsOf(state, p.id);
  const aiStars = state.aiStars[p.id];
  const culled = Object.keys(state.aiVerdicts).length > 0;
  const thumb = usePreview(p, 'thumb');
  // What the specialist models saw (live), or the dummy Photo's stand-in.
  const facts = usePhotoFacts(LIVE ? p.id : null, LIVE);
  const demoEyes = useEyeFlags();
  const eyes = LIVE ? eyeFlagsOf(facts) : demoEyes(p.id);
  const move = (d: number) => {
    const n = order[pos + d];
    if (n) go({ name: 'loupe', id: n });
  };
  // Greg's pick: setting a Verdict in the loupe moves on to the next photo.
  const set = (x: Verdict) => {
    dispatch({ type: 'verdict/set', id: p.id, verdict: x, by: 'you' });
    setTimeout(() => move(1), 160);
  };
  const rate = (n: number) => {
    dispatch({ type: 'stars/set', id: p.id, stars: n, by: 'you' });
    setTimeout(() => move(1), 160);
  };
  const compare = () => stackIds.length && go({ name: 'compare', ids: stackIds });

  // Delete removes this Photo from the Library (after asking) and shows the next one.
  useHotkeys(
    ['backspace', 'delete'],
    () => {
      const next = order[pos + 1] ?? order[pos - 1];
      askRemove({ photos: [p.id], after: () => go(next ? { name: 'loupe', id: next } : { name: 'cull' }) });
    },
    { preventDefault: true },
    [p.id, pos],
  );
  useHotkeys('right', () => move(1), [pos]);
  useHotkeys('left', () => move(-1), [pos]);
  useHotkeys(['escape', 'g'], () => go({ name: 'cull' }));
  useHotkeys(['p', 'x', 'u'], (e) => set(VERDICT_KEYS[e.key.toLowerCase()]), [p.id, pos]);
  useHotkeys(STAR_KEYS, (e) => rate(Number(e.key)), [p.id, pos]);
  useHotkeys('f', () => setFaces((f) => !f));
  useHotkeys('backslash', () => setBefore((b) => !b));
  useHotkeys('d', () => go({ name: 'edit', id: p.id }), [p.id]);
  useHotkeys('c', compare, [stackIds.join(',')]);

  return (
    <div className="panel main view" style={{ flexDirection: 'column', borderRadius: 0, border: 'none', background: 'var(--bg)' }}>
      <header className="panel-head view-head">
        <BackToGrid />
        <span className="mono view-file">{p.file}</span>
        <span className="mono faint view-meta">
          {pos + 1} / {order.length}
        </span>
        <span className="mono faint view-meta trunc">{p.exif}</span>
        <span className="grow" />
        <ViewSwitch view="loupe" id={p.id} compareIds={stackIds} />
        <QueueMeter />
      </header>
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          <div className="stage" data-testid="loupe" {...zp.stageProps}>
            <div className="stage-fit">
              <PhotoView photo={p} size="full" recipe={state.recipes[p.id]} look={state.look} original={before} rendered={detail.url} edits>
                <Faces p={p} all={faces} found={LIVE ? (facts?.faces ?? []) : undefined} />
                {before && <span className="look-label">Before</span>}
              </PhotoView>
            </div>
            <ZoomReadout z={zp.z} loading={detail.loading} />
          </div>
          <div className="loupe-bar" data-testid="loupe-strip">
            <FlagSeg v={v} onSet={set} />
            <span className="row" style={{ gap: 6 }}>
              <Stars n={rating.stars} you={rating.you} onSet={(n) => dispatch({ type: 'stars/set', id: p.id, stars: n, by: 'you' })} size={12} />
              {!rating.you && aiStars ? <span className="faint">AI {aiStars}, press {aiStars} to confirm</span> : null}
            </span>
            {culled && p.best != null && (
              <span className="v-chip best">
                <Award size={11} /> Best #{p.best}
              </span>
            )}
            <EyeBadges flags={eyes} inline />
            <span className="grow" />
            <Hints items={[['Z', zoomed ? 'Fit' : '100%'], ['← →', 'Move'], ['\\', before ? 'After' : 'Before'], ...(stackIds.length ? ([['C', 'Compare']] as [string, string][]) : [])]} />
          </div>
        </div>
        <aside className="side info-side" aria-label="Info">
          <div className="side-tabs">
            <Seg label="Panel" value={tab} onChange={setTab} items={[{ value: 'info', label: 'Info' }, { value: 'agent', label: 'Agent' }]} />
          </div>
          {tab === 'agent' ? (
            <Agent embed ctx={`edit:${p.id}`} chips={[p.file]} placeholder="Ask about or fix this Photo" selected={p} />
          ) : (
            <div className="scroll info-scroll">
              <section style={{ padding: '10px 12px 8px' }}>
                <Histogram src={thumb} exif={p.exif} />
              </section>
              {(hasAi || p.reason) && (
                <section>
                  <div className="row side-title">
                    <h2>{hasAi ? 'AI read' : 'Read'}</h2>
                    <span className="grow" />
                    {hasAi && <span className="faint mono">{aiName()}</span>}
                  </div>
                  <p className="ai-read">
                    {hasAi && v === ai ? `${VERDICT_LABEL[ai]}. ` : ''}
                    {p.reason ? `${p.reason}. ` : ''}
                    {p.stackId ? ((p.stackRank ?? 0) === 0 ? `Best frame in a Stack of ${stack.length}.` : `Frame ${(p.stackRank ?? 0) + 1} of ${stack.length} in its Stack.`) : ''}
                    {v && hasAi && v !== ai && (!LIVE || p.reason) ? ` ${aiName()} said ${VERDICT_LABEL[ai]}; your choice wins and is logged as an Override.` : ''}
                  </p>
                  {/* The engine doesn't score focus, eyes or exposure yet, so real Photos have no Signals. */}
                  {!LIVE && <SignalBars p={p} />}
                </section>
              )}
              {facts?.summary && (
                <section>
                  <div className="row side-title">
                    <h2>Seen</h2>
                    <span className="grow" />
                    <span className="faint mono">on-device models</span>
                  </div>
                  <p className="ai-read" data-testid="facts-summary">
                    {facts.summary.charAt(0).toUpperCase() + facts.summary.slice(1)}.
                  </p>
                </section>
              )}
              {stackIds.length > 0 && (
                <section>
                  <div className="row side-title">
                    <h2>Stack · best of {stack.length}</h2>
                    <span className="grow" />
                    <span className="faint mono">
                      {stack.findIndex((s) => s.id === p.id) + 1} of {stack.length}
                    </span>
                  </div>
                  <div className="side-thumbs">
                    {stack.map((s) => (
                      <button key={s.id} className={s.id === p.id ? 'on' : ''} onClick={() => go({ name: 'loupe', id: s.id })} aria-label={s.file}>
                        <PhotoView photo={s} size="tile" recipe={state.recipes[s.id]} look={state.look} edits style={{ width: '100%', height: '100%', borderRadius: 0 }} />
                      </button>
                    ))}
                  </div>
                </section>
              )}
              <Takes p={p} />
              <section style={{ borderBottom: 'none' }}>
                <h2>Metadata</h2>
                <Metadata p={p} pos={pos} total={order.length} stack={stack.length} />
              </section>
            </div>
          )}
        </aside>
      </div>
      <Filmstrip order={order} pos={pos} stackIds={stackIds} open={(id) => go({ name: 'loupe', id })} />
    </div>
  );
}

/** Lightroom's flag row: Pick, Clear, Reject, with their keys. */
function FlagSeg({ v, onSet }: { v: Verdict | null; onSet: (v: Verdict) => void }) {
  return (
    <span className="flag-seg" role="group" aria-label="Flag">
      <button className={v === 'pick' ? 'on pick' : 'pick'} aria-pressed={v === 'pick'} onClick={() => onSet(v === 'pick' ? 'unflagged' : 'pick')}>
        <Flag size={11} strokeWidth={2} fill={v === 'pick' ? 'currentColor' : 'none'} /> Pick <Kbd>P</Kbd>
      </button>
      <button className={!v || v === 'unflagged' ? 'on' : ''} aria-pressed={!v || v === 'unflagged'} onClick={() => onSet('unflagged')}>
        Clear <Kbd>U</Kbd>
      </button>
      <button className={v === 'reject' ? 'on reject' : 'reject'} aria-pressed={v === 'reject'} onClick={() => onSet(v === 'reject' ? 'unflagged' : 'reject')}>
        <X size={11} strokeWidth={2.4} /> Reject <Kbd>X</Kbd>
      </button>
    </span>
  );
}

/** The AI's Signals as small bars (dummy Photos only; the engine doesn't score them yet). */
function SignalBars({ p }: { p: Photo }) {
  const s = p.signals;
  const [open, of] = s.eyesOpen;
  const rows: [string, number, string, boolean][] = [
    ['Focus', s.focus, s.focus.toFixed(2), s.focus < 0.6],
    ['Eyes open', of ? open / of : 1, of ? `${open} of ${of}` : 'No faces', open < of],
    ['Exposure', 1 - Math.min(1, Math.abs(s.exposure) / 2), `${s.exposure > 0 ? '+' : s.exposure < 0 ? '−' : ''}${Math.abs(s.exposure).toFixed(1)}`, Math.abs(s.exposure) > 1],
  ];
  return (
    <div className="signal-bars">
      {rows.map(([label, f, value, bad]) => (
        <div key={label} className="signal-bar">
          <span className="muted">{label}</span>
          <span className="track">
            <i style={{ width: `${Math.round(f * 100)}%`, background: bad ? 'var(--reject)' : undefined }} />
          </span>
          <span className="mono" style={{ color: bad ? 'var(--reject)' : undefined }}>
            {value}
          </span>
        </div>
      ))}
      <div className="signal-bar">
        <span className="muted">Expression</span>
        <span />
        <span className="mono">{s.expression}</span>
      </div>
    </div>
  );
}

/** The Photo's Suggestions, small: a click opens Edit with them. */
function Takes({ p }: { p: Photo }) {
  const { list } = useSuggestions(p.id);
  const open = list.filter((s) => s.status === 'open');
  if (!open.length) return null;
  return (
    <section>
      <div className="row side-title">
        <h2>Suggestions</h2>
        <span className="grow" />
        <span className="v-chip accent">{open.length} ready</span>
      </div>
      <div className="side-thumbs takes">
        {open.slice(0, 3).map((s) => (
          <TakeThumb key={s.id} p={p} s={s} />
        ))}
      </div>
    </section>
  );
}

function TakeThumb({ p, s }: { p: Photo; s: Suggestion }) {
  const { state } = useLibrary();
  const url = useSuggestionImage(s, 240);
  return (
    <button onClick={() => go({ name: 'edit', id: p.id })} title={`${s.title}: ${s.why}`} aria-label={s.title}>
      <PhotoView photo={p} recipe={s.recipe} look={state.look} rendered={url} style={{ width: '100%', height: '100%', borderRadius: 0 }} />
    </button>
  );
}

/** Lightroom's Metadata panel, trimmed to what a cull needs. */
function Metadata({ p, pos, total, stack }: { p: Photo; pos: number; total: number; stack: number }) {
  const [shutter, aperture, iso, focal] = p.exif.split(' · ');
  return (
    <div className="signals meta">
      <span className="muted">File</span>
      <span className="mono">{p.file}</span>
      <span className="muted">Shoot</span>
      <span>{SHOOT.name}</span>
      <span className="muted">Position</span>
      <span className="mono">{pos + 1} / {total}</span>
      {stack > 1 && (
        <>
          <span className="muted">Stack</span>
          <span className="mono">{(p.stackRank ?? 0) + 1} / {stack}</span>
        </>
      )}
      {LIVE && p.meta ? (
        <>
          <span className="muted">Camera</span>
          <span className="mono" title={p.meta.camera}>{p.meta.camera}</span>
          {p.meta.lens && (
            <>
              <span className="muted">Lens</span>
              <span className="mono" title={p.meta.lens}>{p.meta.lens}</span>
            </>
          )}
          {p.meta.exposure && (
            <>
              <span className="muted">Shutter · f</span>
              <span className="mono">{p.meta.exposure.replace(' · ', '  ')}</span>
            </>
          )}
          {p.meta.isoFocal && (
            <>
              <span className="muted">ISO · Focal</span>
              <span className="mono">{p.meta.isoFocal}</span>
            </>
          )}
          <span className="muted">Captured</span>
          <span className="mono">{p.meta.captured}</span>
          <span className="muted">Size</span>
          <span className="mono">{p.meta.size}</span>
        </>
      ) : (
        <>
          <span className="muted">Shutter · f</span>
          <span className="mono">{[shutter, aperture].filter(Boolean).join('  ')}</span>
          <span className="muted">ISO · Focal</span>
          <span className="mono">{[iso?.replace('ISO ', ''), focal].filter(Boolean).join(' · ')}</span>
        </>
      )}
    </div>
  );
}
