import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { LIVE, aiError, PROVIDERS as ENGINE_PROVIDERS, chooseProvider, chosenProvider, providerDown, listLooks, liveCull, readCull, readRecipes } from '../data/live';
import { askAgent, stepLabel, undoBatch, type Turn } from '../data/liveAgent';
import { loadRecipe } from '../data/liveEdit';
import { plain } from '../data/suggestions';
import { ArrowUp, Check, ChevronDown, X } from 'lucide-react';
import { useLibrary, type Action, type ChatMessage } from '../store/library';
import { LIVE_SUGGESTIONS, respond, SUGGESTIONS, type AgentPlan } from './agent';
import type { Photo } from '../domain/types';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';
import { Button } from '@/components/ui/button';
import { ProgressBar, useEngineTask } from '../ui/Progress';

const DEMO_PROVIDERS = [
  { id: 'ollama', label: 'Ollama · qwen3.8:27b', note: 'On this Mac · nothing leaves it' },
  { id: 'claude-code', label: 'Claude · opus', note: 'Your Claude plan, through the claude CLI' },
];

/** Live: the engine's real Providers, with the one a cull will use selected. */
function providerOptions() {
  if (!LIVE) return { list: DEMO_PROVIDERS, initial: DEMO_PROVIDERS[0] };
  const list = ENGINE_PROVIDERS.map((p) => ({ id: p.id, label: p.model ? `${p.label} · ${p.model}` : p.label, note: p.down ? `${p.down} · open it to use it` : p.note }));
  return { list, initial: list.find((p) => p.id === chosenProvider()) ?? list[0] };
}

let seq = 0;
const mid = () => `c${Date.now().toString(36)}${seq++}`;

/** Runs an AgentPlan: animates its steps, then applies its actions as one undoable batch. */
export function useAgentRunner(ctx: string) {
  const { dispatch } = useLibrary();
  return useCallback(
    (plan: AgentPlan, prompt?: string) => {
      if (prompt) dispatch({ type: 'chat/push', ctx, msg: { id: mid(), role: 'you', text: prompt } });
      const id = mid();
      dispatch({ type: 'chat/push', ctx, msg: { id, role: 'agent', steps: plan.steps.map((s) => ({ label: s.label, ms: 0, done: false })) } });
      let t = 0;
      plan.steps.forEach((_, i) => {
        t += plan.steps[i].wait;
        setTimeout(() => {
          dispatch({ type: 'chat/patch', ctx, msgId: id, patch: { steps: plan.steps.map((x, j) => ({ label: x.label + (j <= i ? ` ·${x.shown}` : ''), ms: 0, done: j <= i })) } });
        }, t);
      });
      setTimeout(() => {
        if (plan.actions.length) dispatch({ type: 'batch', label: plan.batchLabel ?? 'Agent', actions: plan.actions });
        const actions: ChatMessage['actions'] = [];
        if (plan.changed?.length && plan.batchLabel && ctx === 'cull') actions.push({ label: `Review ${plan.changed.length} changes`, kind: 'review', ids: plan.changed });
        if (plan.batchLabel) actions.push({ label: 'Undo batch', kind: 'undo' });
        dispatch({ type: 'chat/patch', ctx, msgId: id, patch: { text: plan.prose, actions } });
      }, t + 200);
    },
    [ctx, dispatch],
  );
}

function Message({ m, ctx, onReview }: { m: ChatMessage; ctx: string; onReview?: (ids: string[]) => void }) {
  const { dispatch } = useLibrary();
  const undo = (a: NonNullable<ChatMessage['actions']>[number]) => (a.batch != null ? void liveUndo(ctx, m.id, a.batch, dispatch) : dispatch({ type: 'undo' }));
  if (m.role === 'you') return <div className="bubble">{m.text}</div>;
  return (
    <>
      {!!m.steps?.length && (
        <div className="steps">
          {m.steps.map((s, i) => {
            const [label, time] = s.label.split(' ·');
            return (
              <div className="step" key={i}>
                {s.failed ? <X size={12} strokeWidth={2.6} color="var(--reject)" /> : s.done ? <Check size={12} strokeWidth={2.6} color="var(--done)" /> : <span className="spin" />}
                <span className="label">{label}</span>
                <span className="time">{time ?? ''}</span>
              </div>
            );
          })}
        </div>
      )}
      {LIVE && m.steps?.some((s) => s.label === CULL_STEP && !s.done) && <CullBar />}
      {m.text && <p className="prose">{m.text}</p>}
      {!!m.actions?.length && (
        <div className="row">
          {m.actions.map((a) => (
            <Button key={a.label} variant={a.kind === 'undo' ? 'ghost' : 'outline'} onClick={() => (a.kind === 'undo' ? undo(a) : onReview?.(a.ids ?? []))}>
              {a.label}
            </Button>
          ))}
        </div>
      )}
    </>
  );
}

const CULL_STEP = 'Looking at the whole Shoot';

/** The running cull's bar: Photos the model has answered for so far, and an ETA. */
function CullBar() {
  const task = useEngineTask('cull', true);
  if (!task) return <ProgressBar label="Reading the photos" testId="cull-progress" />;
  // Queued behind another model user: say on whom, with no bar or ETA that pretends to move.
  if (task.waiting) return <ProgressBar label={`Waiting: ${plain(task.waiting)}`} testId="cull-progress" />;
  return <ProgressBar label={task.label} done={task.done} total={task.total || undefined} eta={task.eta_s} testId="cull-progress" />;
}

function Composer({ ctx, chips, placeholder, onSend, onProvider }: { ctx: string; chips: string[]; placeholder: string; onSend: (t: string) => void; onProvider?: () => void }) {
  const [text, setText] = useState('');
  const [{ list: providers, initial }] = useState(providerOptions);
  const [provider, setProvider] = useState(initial);
  const send = () => {
    if (!text.trim()) return;
    onSend(text.trim());
    setText('');
  };
  return (
    <div className="composer">
      <div className="row" style={{ flexWrap: 'wrap', gap: 6 }}>
        {chips.map((c) => (
          <span key={c} className="context-chip">
            {c}
          </span>
        ))}
      </div>
      <label className="sr-only" htmlFor={`ask-${ctx}`}>
        Ask Argus
      </label>
      <textarea
        id={`ask-${ctx}`}
        rows={1}
        value={text}
        placeholder={placeholder}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            send();
          }
          if (e.key === 'Escape') (e.target as HTMLElement).blur();
        }}
      />
      <div className="row">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm" data-testid="provider">
              {provider?.label} <ChevronDown size={12} />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent side="top" align="start" className="min-w-[240px]">
            {providers.map((p) => (
              <DropdownMenuItem
                key={p.id}
                className="h-auto items-start py-1.5"
                onSelect={() => {
                  setProvider(p);
                  if (LIVE) chooseProvider(p.id);
                  onProvider?.();
                }}
              >
                <span className="mt-0.5 w-3 flex-none">{p.id === provider?.id && <Check size={12} />}</span>
                <span className="flex min-w-0 flex-col gap-0.5 leading-tight">
                  <span className="text-text">{p.label}</span>
                  <span className="text-[11px] text-text-3">{p.note}</span>
                </span>
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <span className="grow" />
        <button className="send" aria-label="Send" disabled={!text.trim()} onClick={send}>
          <ArrowUp size={14} strokeWidth={2.4} />
        </button>
      </div>
    </div>
  );
}

interface AgentProps {
  ctx: string;
  chips: string[];
  placeholder: string;
  selected?: Photo | null;
  picks?: string[];
  onReview?: (ids: string[]) => void;
  /** Controls shown above the conversation (Look sliders, Mask cards). */
  header?: ReactNode;
  /** A Lightroom-style tool strip down the right edge; the header then sits above the Agent's own heading, as the Edit panel. */
  strip?: ReactNode;
  /** Replaces the "Agent" heading (the Look panel names its Look). */
  title?: ReactNode;
  /** Shown after the conversation, above the composer (the Look's card). */
  after?: ReactNode;
  /** Render inside another panel (the loupe's Agent tab) instead of as its own aside. */
  embed?: boolean;
}

/** The Agent's side panel (Greg's pick, 2026-10-17). */
export function Agent({ ctx, chips, placeholder, selected, picks, onReview, header, strip, title, after, embed }: AgentProps) {
  const { state } = useLibrary();
  const run = useAgentRunner(ctx);
  const messages = state.chats[ctx] ?? [];
  const bodyRef = useRef<HTMLDivElement>(null);
  const kind = ctx.startsWith('edit:') ? 'edit' : ctx;
  const { dispatch } = useLibrary();
  // Re-read on a Provider change, so the note below follows the menu.
  const [, providerChanged] = useState(0);
  const down = providerDown();
  const others = ENGINE_PROVIDERS.filter((p) => p.id !== chosenProvider() && !p.down).map((p) => p.label);
  const send = (t: string) => {
    if (LIVE && ctx === 'cull' && /\bcull\b/i.test(t)) return void liveCullChat(t, dispatch);
    if (LIVE) return void liveAgentChat(ctx, t, ctx.startsWith('edit:') ? ctx.slice(5) : (selected?.id ?? null), messages, dispatch);
    run(respond(ctx, t, state, { selected, picks }), t);
  };
  useEffect(() => {
    bodyRef.current?.scrollTo({ top: bodyRef.current.scrollHeight, behavior: 'smooth' });
  }, [messages]);
  // ⌘K sends what matched no command to the grid's Agent (see `askFromPalette`).
  const sendRef = useRef(send);
  sendRef.current = send;
  useEffect(() => {
    if (ctx !== 'cull') return;
    const on = (e: Event) => sendRef.current((e as CustomEvent<string>).detail);
    window.addEventListener('argus:ask', on);
    return () => window.removeEventListener('argus:ask', on);
  }, [ctx]);
  const head = embed ? null : (
    <div className={strip ? 'panel-head agent-head' : 'panel-head'}>
      {title ?? (
        <>
          <div style={{ fontWeight: 500 }}>Agent</div>
        </>
      )}
    </div>
  );
  // Under the Edit panel the Agent waits as its composer alone, so the sliders get the
  // height; its heading and suggestions come back once it's in use or the composer has focus.
  const [focused, setFocused] = useState(false);
  const slim = !!strip && messages.length === 0 && !focused;
  const chat = (
    <>
      {strip ? head : header}
      <div className="agent-body" ref={bodyRef}>
        {messages.length === 0 && (
          <div className="suggest">
            {((LIVE ? LIVE_SUGGESTIONS : SUGGESTIONS)[kind] ?? []).map((s) => (
              // Keep focus in the composer: under Edit the chips hide again once it leaves, and
              // WebKit never focuses a clicked button, so the click would land on nothing.
              <button key={s} disabled={!!down} title={down ?? undefined} onMouseDown={(e) => e.preventDefault()} onClick={() => send(s)}>
                {s}
              </button>
            ))}
          </div>
        )}
        {messages.map((m) => (
          <Message key={m.id} m={m} ctx={ctx} onReview={onReview} />
        ))}
        {down && (
          <p className="prose muted" data-testid="provider-down">
            {down}, so I can't cull or edit yet. Open it{others.length ? `, or pick ${others[0]} below` : ''}.
          </p>
        )}
        {after}
      </div>
      <Composer ctx={ctx} chips={chips} placeholder={placeholder} onSend={send} onProvider={() => providerChanged((n) => n + 1)} />
    </>
  );
  const column = strip ? (
    <>
      {header}
      <div
        className={`agent-chat ${slim ? 'slim' : ''}`}
        data-testid="agent-chat"
        onFocus={() => setFocused(true)}
        onBlur={(e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && setFocused(false)}
      >
        {chat}
      </div>
    </>
  ) : (
    <>
      {head}
      {chat}
    </>
  );
  if (embed) return <div className="agent-embed">{column}</div>;
  if (!strip)
    return (
      <aside className="panel agent" aria-label="Agent">
        {column}
      </aside>
    );
  return (
    <aside className="panel agent with-strip" aria-label="Agent">
      <div className="agent-col">{column}</div>
      {strip}
    </aside>
  );
}

/** Live mode: a real Shoot-wide AI cull through the engine, then reload from the Library. */
export async function liveCullChat(prompt: string, dispatch: React.Dispatch<Action>) {
  const at = Date.now();
  dispatch({ type: 'chat/push', ctx: 'cull', msg: { id: `you-${at}`, role: 'you', text: prompt } });
  dispatch({ type: 'chat/push', ctx: 'cull', msg: { id: `agent-${at}`, role: 'agent', steps: [{ label: CULL_STEP, ms: 0, done: false }] } });
  const t0 = performance.now();
  try {
    const r = await liveCull();
    const secs = ((performance.now() - t0) / 1000).toFixed(1);
    dispatch({
      type: 'chat/patch',
      ctx: 'cull',
      msgId: `agent-${at}`,
      patch: {
        steps: [{ label: `Culled and rated the whole Shoot ·${secs}s`, ms: 0, done: true }],
        text: `${r.summary} ${r.picks ? `${r.picks} Picks, ` : ''}${r.rejects} Rejects, ${r.unflagged} left unflagged, ${r.stacks} Stacks.${r.stars?.[5] ? ` ${r.stars[5]} rated five stars.` : ''}${r.best?.length ? ` ${r.best.length} Best Photos.` : ''}${r.kept_person_verdicts || r.kept_person_ratings ? ' I left your own flags and stars alone.' : ''}`,
      },
    });
    setTimeout(() => location.reload(), 2500);
  } catch (e) {
    dispatch({ type: 'chat/patch', ctx: 'cull', msgId: `agent-${at}`, patch: { steps: [{ label: 'Cull failed', ms: 0, done: true, failed: true }], text: aiError(e) } });
  }
}

/** The conversation so far, as text turns for the Agent ("a bit more" needs the last request). */
function turns(messages: ChatMessage[]): Turn[] {
  return messages.filter((m) => m.text && m.id !== 'seed').map((m) => ({ role: m.role === 'you' ? 'user' : 'agent', text: m.text! }));
}

/**
 * Live mode: the engine's Agent carries out the request through the tool layer, as one
 * batch. The Photo in Edit picks up its new Recipe; Verdict changes reload the grid.
 */
export async function liveAgentChat(ctx: string, prompt: string, photoId: string | null, earlier: ChatMessage[], dispatch: React.Dispatch<Action>) {
  const at = Date.now();
  const id = `agent-${at}`;
  dispatch({ type: 'chat/push', ctx, msg: { id: `you-${at}`, role: 'you', text: prompt } });
  dispatch({ type: 'chat/push', ctx, msg: { id, role: 'agent', steps: [{ label: 'Looking at the Shoot', ms: 0, done: false }] } });
  const t0 = performance.now();
  try {
    const r = await askAgent(prompt, photoId, turns(earlier));
    const secs = ((performance.now() - t0) / 1000).toFixed(1);
    const steps = [{ label: `Looked at the Shoot ·${secs}s`, ms: 0, done: true }, ...r.steps.map((s) => ({ label: `${stepLabel(s)} ·${(s.ms / 1000).toFixed(1)}s`, ms: s.ms, done: true }))];
    const actions: ChatMessage['actions'] = r.batch_id != null ? [{ label: 'Undo', kind: 'undo', batch: r.batch_id }] : [];
    if (r.batch_id != null && ctx === 'cull' && r.verdicts.length) actions.unshift({ label: `Review ${r.verdicts.length} changes`, kind: 'review', ids: r.verdicts.map(String) });
    dispatch({ type: 'chat/patch', ctx, msgId: id, patch: { steps, text: r.text, actions } });
    await refresh(ctx, r.recipes, r.verdicts, dispatch);
  } catch (e) {
    dispatch({ type: 'chat/patch', ctx, msgId: id, patch: { steps: [{ label: 'The Agent stopped', ms: 0, done: true, failed: true }], text: aiError(e) } });
  }
}

/** Undo an Agent request in the Library, then show the Photos as they are now. */
async function liveUndo(ctx: string, msgId: string, batch: number, dispatch: React.Dispatch<Action>) {
  try {
    const r = await undoBatch(batch);
    dispatch({ type: 'chat/patch', ctx, msgId, patch: { actions: [] } });
    dispatch({ type: 'toast', text: `Undid ${r.label}` });
    await refresh(ctx, r.recipes, [...r.verdicts, ...(r.ratings ?? [])], dispatch);
  } catch (e) {
    dispatch({ type: 'toast', text: `Undo failed: ${String(e).replace(/^Error: /, '')}` });
  }
}

/** Read back what the engine changed: the open Photo's Recipe, and the grid's Verdicts and stars. */
async function refresh(ctx: string, recipes: number[], verdicts: number[], dispatch: React.Dispatch<Action>) {
  const open = ctx.startsWith('edit:') ? ctx.slice(5) : null;
  if (open && recipes.includes(Number(open))) {
    const recipe = await loadRecipe(open);
    if (recipe) dispatch({ type: 'recipe/load', id: open, recipe });
    window.dispatchEvent(new CustomEvent('argus:versions', { detail: open }));
  }
  if (verdicts.length) dispatch({ type: 'cull/states', states: await readCull(verdicts) });
  if (ctx === 'look') {
    const [loaded, looks] = await Promise.all([readRecipes(recipes), listLooks()]);
    for (const [id, recipe] of Object.entries(loaded)) dispatch({ type: 'recipe/load', id, recipe });
    dispatch({ type: 'look/load', looks });
  }
}
