import { useEffect, useState } from 'react';
import { Slider } from '@/components/ui/slider';
import { Check, Sparkles, X } from 'lucide-react';
import type { Look, Photo, Recipe, Suggestion } from '../domain/types';
import { useSuggestionImage, type TakesJob } from '../data/suggestions';
import { PhotoView } from './PhotoView';
import { Kbd } from './kit';
import { Button } from '@/components/ui/button';

function Tile({ photo, recipe, look, s, label, title, why, selected, onPick }: { photo: Photo; recipe: Recipe; look: Look | null; s: Suggestion | null; label: string; title: string; why: string; selected: boolean; onPick: () => void }) {
  const url = useSuggestionImage(s, 320);
  return (
    <button className={`take ${selected ? 'sel' : ''}`} onClick={onPick} aria-pressed={selected} data-testid="take" title={`${title}: ${why}`}>
      <PhotoView photo={photo} size="thumb" recipe={recipe} look={look} rendered={url} />
      <span className="take-title">
        <span className="mono faint">{label}</span>
        <span className="take-name">{title}</span>
        {s?.referenceId && <Sparkles size={11} className="faint" aria-label="Matched to a Hero Photo" />}
        {s?.status === 'accepted' && <Check size={11} style={{ color: 'var(--done)' }} aria-label="Accepted" />}
      </span>
      <span className="take-why">{why}</span>
    </button>
  );
}

/** A take on its way: the photo, dimmed, developing under a shimmer, with its stage. */
function Pending({ photo, recipe, look, n, stage, delay }: { photo: Photo; recipe: Recipe; look: Look | null; n: number; stage: string; delay: number }) {
  return (
    <div className="take pending" data-testid="take-pending" style={{ animationDelay: `${delay}ms` }} aria-label={`Take ${n}: ${stage}`}>
      <div className="take-dev">
        <PhotoView photo={photo} size="thumb" recipe={recipe} look={look} />
        <div className="shimmer" style={{ animationDelay: `${delay * 2}ms` }} />
      </div>
      <span className="take-title">
        <span className="mono faint">+</span>
        <span className="take-name faint">New take {n}</span>
      </span>
      <span className="take-why">{stage}</span>
    </div>
  );
}

/** "0:42 left", "~12 min left". */
function left(s: number): string {
  const n = Math.max(1, Math.round(s));
  if (n < 600) return `${Math.floor(n / 60)}:${String(n % 60).padStart(2, '0')} left`;
  return `~${Math.round(n / 60)} min left`;
}

/** The job as of now: its bar and ETA move between the engine's reads. */
function useLive(job: TakesJob | null): { progress: number | null; eta: number | null; elapsed: number | null } {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!job) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [job]);
  if (!job) return { progress: null, eta: null, elapsed: null };
  const since = (Date.now() - job.at) / 1000;
  const eta = job.eta_s == null ? null : Math.max(1, job.eta_s - since);
  if (job.status !== 'running') return { progress: null, eta, elapsed: null };
  const elapsed = (job.elapsed_s ?? 0) + since;
  // The engine's steps, or time against recent takes, whichever is further; never "done".
  const byTime = job.avg_s ? Math.min(0.95, elapsed / job.avg_s) : 0;
  return { progress: Math.max(job.progress ?? 0, byTime), eta, elapsed };
}

/** What the takes on their way are doing, in a few words. */
function jobLine(job: TakesJob, held: string | null): string {
  const what = `Making ${job.takes} ${job.matched ? 'matched takes' : 'takes'}`;
  if (held) return `${what} · ${held}`;
  if (job.status === 'running') return `${what} · ${job.stage}`;
  if (!job.ahead) return `${what} · up next`;
  return `${what} · waiting behind ${job.behind ?? 'other work'}${job.ahead > 1 ? ` and ${job.ahead - 1} more` : ''}`;
}

/** "New takes", or while they're on their way a progress bar with the stage, ETA and Cancel. */
function TakesButton({ job, held, onMore, onCancel }: { job: TakesJob | null; held: string | null; onMore: () => void; onCancel: () => void }) {
  const { progress, eta, elapsed } = useLive(job);
  if (!job)
    return (
      <Button size="sm" variant="ghost" onClick={onMore} data-testid="new-takes">
        New takes <Kbd>R</Kbd>
      </Button>
    );
  const running = job.status === 'running';
  const status = held ?? (running ? job.stage : job.ahead ? `Queued · ${job.ahead} ahead` : 'Starting');
  const time = eta != null ? left(eta) : elapsed != null ? `${Math.round(elapsed)} s so far` : 'no timing yet';
  return (
    <div className="takes-progress" data-testid="takes-progress" role="status" aria-live="polite">
      <div className="takes-progress-text">
        <span className="takes-progress-stage">{status}</span>
        <span className="mono faint">{time}</span>
      </div>
      <div className={`progress-track ${progress == null ? 'indeterminate' : ''} ${held ? 'held' : ''}`}>
        <div style={progress == null ? undefined : { width: `${Math.round(progress * 100)}%` }} />
      </div>
      <button className="takes-cancel" onClick={onCancel} title="Cancel these takes" aria-label="Cancel these takes" data-testid="cancel-takes">
        <X size={12} />
      </button>
    </div>
  );
}

/**
 * The takes under the photo in Edit, like Lightroom's preset previews: your edit first,
 * then the AI's Suggestions. Picking one previews it on the photo; nothing is saved until
 * it's accepted.
 */
export function SuggestionStrip({ photo, recipe, look, list, pick, onPick, onAccept, amount, onAmount, onDismiss, onMore, onCancel, job, held, waiting }: {
  photo: Photo;
  recipe: Recipe;
  look: Look | null;
  list: Suggestion[];
  /** -1 = your edit. */
  pick: number;
  onPick: (i: number) => void;
  onAccept: () => void;
  /** The picked take's Amount, 0..150 (100 = as proposed). */
  amount: number;
  onAmount: (n: number) => void;
  onDismiss: () => void;
  onMore: () => void;
  onCancel: () => void;
  /** New takes on their way, if any. */
  job: TakesJob | null;
  /** Why the queue isn't running them now ("Paused", "Waiting for the charger"). */
  held: string | null;
  /** Shown when there are no takes yet. */
  waiting: string | null;
}) {
  const current = list[pick] ?? null;
  return (
    <section className="takes" aria-label="Suggestions">
      <div className="row" style={{ gap: 10 }}>
        <span style={{ fontWeight: 500 }}>Suggestions</span>
        <span className="faint" style={{ fontSize: 12 }} data-testid="takes-line">{job ? jobLine(job, held) : list.length ? 'Nothing changes until you accept one' : waiting ?? 'None yet'}</span>
        <span className="grow" />
        {current ? (
          <>
            <label className="row faint" style={{ gap: 8, fontSize: 12 }} title="How strong the take is: 0 is your edit, 100 the take. Double-click for 100." onDoubleClick={() => onAmount(100)}>
              Amount
              <Slider className="w-[120px]" min={0} max={150} step={5} value={[amount]} onValueChange={([v]) => onAmount(v)} aria-label="Amount" data-testid="take-amount" />
              <span className="mono" style={{ width: 34, textAlign: 'right', color: 'var(--text)' }}>{amount}%</span>
            </label>
            <Button size="sm" onClick={onAccept} data-testid="accept-take">
              Accept <Kbd>↵</Kbd>
            </Button>
            <Button size="sm" variant="ghost" onClick={onDismiss}>
              Dismiss <Kbd>⌫</Kbd>
            </Button>
          </>
        ) : (
          list.length > 0 && <span className="faint" style={{ fontSize: 12 }}><Kbd>Tab</Kbd> to preview a take</span>
        )}
        <TakesButton job={job} held={held} onMore={onMore} onCancel={onCancel} />
      </div>
      <div className="takes-row">
        <Tile photo={photo} recipe={recipe} look={look} s={null} label="0" title="Your edit" why="What you have now" selected={pick < 0} onPick={() => onPick(-1)} />
        {list.map((s, i) => (
          <Tile key={s.id} photo={photo} recipe={s.recipe} look={look} s={s} label={String(i + 1)} title={s.title} why={s.why} selected={pick === i} onPick={() => onPick(i)} />
        ))}
        {job && Array.from({ length: job.takes }, (_, i) => <Pending key={`${job.id}-${i}`} photo={photo} recipe={recipe} look={look} n={i + 1} stage={held ?? (job.status === 'running' ? job.stage : 'Queued')} delay={i * 90} />)}
      </div>
    </section>
  );
}
