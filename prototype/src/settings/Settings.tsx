// Settings: ⌘, (or Argus › Settings… in the app menu). A sheet with one page per section.
// Sections are a list, so a new area of settings is one entry in SECTIONS and nothing else.

import { useEffect, useState, type ReactNode } from 'react';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { LIVE, PROVIDERS, chooseProvider, chosenProvider, loadProviders } from '../data/live';
import { call } from '../data/backend';
import { toastFailure } from '@/lib/utils';
import { toast } from 'sonner';
import { bytes } from '../data/libraries';
import type { QueueStatus } from '../data/suggestions';
import { COMPUTE_LABEL, COMPUTE_NOTE, dur } from '../ui/Queue';
import { Kbd } from '../ui/kit';
import { Switch } from '@/components/ui/switch';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';
import { ArrowUpCircle, Check, ChevronDown, Download, Keyboard, Library, RefreshCw, Settings2, Sparkles, Upload } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ago, checkForUpdates, restartToUpdate, useUpdate } from '../data/updates';
import { IN_APP } from '../data/backend';
import { Seg } from '../ui/kit';
import { LibrarySection } from './LibrarySection';
import { Row } from './Row';
import { GeneralSection } from './GeneralSection';
import { ImportSection } from './ImportSection';
import { ExportSection } from './ExportSection';

export interface SettingsSection {
  id: string;
  title: string;
  icon: ReactNode;
  render: () => ReactNode;
}

/** Ask the app to show Settings, optionally at a section, from anywhere. */
export function openSettings(section?: string) {
  window.dispatchEvent(new CustomEvent('argus:settings', { detail: section }));
}

interface Models {
  model: string;
  pinned: boolean;
  models: { name: string; bytes: number; loaded: boolean }[];
  error?: string;
}

type QueueSettings = QueueStatus & { idle_boost_min: number; overnight: boolean; overnight_hours: [number, number]; focus_first: boolean; takes: number; run_on_battery?: boolean };

const DEMO_QUEUE: QueueSettings = { running: null, queued: 0, failed: 0, done_today: 0, eta_s: null, compute: 'gentle', paused: false, idle_boost_min: 10, overnight: true, overnight_hours: [1, 7], focus_first: true, takes: 3, run_on_battery: false };
const DEMO_MODELS: Models = { model: 'qwen3.8:27b', pinned: false, models: [{ name: 'qwen3.8:27b', bytes: 17.3e9, loaded: true }] };

/** Settings › AI and Compute: which model (only ones really installed), how hard it works, when it may go to Full. */
function AiSection() {
  const [provider, setProvider] = useState(chosenProvider());
  const [models, setModels] = useState<Models | null>(LIVE ? null : DEMO_MODELS);
  const [q, setQ] = useState<QueueSettings | null>(LIVE ? null : DEMO_QUEUE);
  const [perPhoto, setPerPhoto] = useState<number | null>(null);
  useEffect(() => {
    if (!LIVE) return;
    void call<Models>('models').then(setModels, () => {});
    void call<QueueSettings>('queue_status').then(setQ, () => {});
    // How long a photo's Suggestions take lately, for an honest sense of pace.
    void call<{ started_at: string | null; finished_at: string | null }[]>('list_jobs', { status: 'done', kind: 'suggest', limit: 20 }).then((js) => {
      const t = js.map((j) => (j.started_at && j.finished_at ? (Date.parse(j.finished_at) - Date.parse(j.started_at)) / 1000 : null)).filter((x): x is number => x != null);
      if (t.length) setPerPhoto(t.reduce((a, b) => a + b, 0) / t.length);
    }, () => {});
  }, []);
  const set = (args: Partial<QueueSettings>) => {
    setQ((cur) => (cur ? { ...cur, ...args } : cur));
    if (LIVE) void call<QueueSettings>('set_queue', args).then(setQ, (e) => {
      toastFailure('Couldn’t save Background work settings')(e);
      void call<QueueSettings>('queue_status').then(setQ, () => {});
    });
  };
  const pick = (m: string) => {
    setModels((cur) => (cur ? { ...cur, model: m } : cur));
    if (LIVE)
      void call('set_model', { model: m }).catch((e) => {
        toastFailure(`Couldn’t switch to ${m}`)(e);
        void call<Models>('models').then(setModels, () => {});
      });
  };
  const choose = (id: string) => {
    chooseProvider(id);
    setProvider(id);
    setKeyOpen(false);
  };
  // The Claude API is offered once its key is in the Keychain (ADR 0006: Greg uses none).
  const [hasKey, setHasKey] = useState(() => PROVIDERS.some((p) => p.id === 'claude-api'));
  const [keyOpen, setKeyOpen] = useState(false);
  const [key, setKey] = useState('');
  const saveKey = () =>
    void call('set_api_key', { key })
      .then(loadProviders)
      .then(() => {
        setHasKey(true);
        setKey('');
        choose('claude-api');
        toast.success('API key saved in the Keychain');
      }, toastFailure('Couldn’t save the API key'));
  const current = models?.models.find((m) => m.name === models.model);
  const claude = PROVIDERS.find((p) => p.id === 'claude-code');
  const [h0, h1] = q?.overnight_hours ?? [1, 7];
  return (
    <div className="set-page" data-testid="settings-ai-page">
      <h3>Model</h3>
      <div className="set-card">
        <label className={`set-choice ${provider === 'ollama' ? 'on' : ''}`}>
          <input type="radio" name="provider" checked={provider === 'ollama'} onChange={() => choose('ollama')} />
          <div className="set-text">
            <div className="set-title">Ollama on this Mac</div>
            <div className="set-note">Private, free, slower. Nothing leaves your Mac.</div>
          </div>
          <span className="grow" />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button className="import-pick set-model" disabled={!models || models.pinned || models.models.length === 0} data-testid="settings-model" title={models?.pinned ? 'Set by ARGUS_OLLAMA_MODEL' : undefined}>
                <span className="mono">{models?.model ?? '…'}</span>
                <ChevronDown size={12} />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[240px]">
              <div className="menu-label">Installed on this Mac</div>
              {models?.models.map((m) => (
                <DropdownMenuItem key={m.name} onSelect={() => pick(m.name)} data-testid="settings-model-item">
                  <span className="mono">{m.name}</span>
                  <span className="grow" />
                  <span className="mono faint">{bytes(m.bytes)}</span>
                  {m.name === models.model && <Check size={12} />}
                </DropdownMenuItem>
              ))}
              <div className="menu-empty">More with `ollama pull` in Terminal</div>
            </DropdownMenuContent>
          </DropdownMenu>
          <span className={`set-status ${models?.error || (models && !current) ? 'bad' : ''}`} data-testid="settings-model-status">
            {!models ? '' : models.error ? 'Ollama not running' : !current ? 'not installed' : current.loaded ? `loaded · ${bytes(current.bytes)}` : `installed · ${bytes(current.bytes)}`}
          </span>
        </label>
        {claude && (
          <label className={`set-choice ${provider === 'claude-code' ? 'on' : ''}`}>
            <input type="radio" name="provider" checked={provider === 'claude-code'} onChange={() => choose('claude-code')} />
            <div className="set-text">
              <div className="set-title">Your Claude plan</div>
              <div className="set-note">Through the official claude CLI. Faster, and sends Contact sheets to Claude.</div>
            </div>
          </label>
        )}
        {LIVE && (
          <label className={`set-choice ${provider === 'claude-api' || keyOpen ? 'on' : ''}`}>
            <input type="radio" name="provider" checked={provider === 'claude-api' || keyOpen} onChange={() => (hasKey ? choose('claude-api') : setKeyOpen(true))} data-testid="settings-claude-api" />
            <div className="set-text">
              <div className="set-title">Claude API</div>
              <div className="set-note">Your own Anthropic API key, billed per request. Sends Contact sheets to Claude.</div>
            </div>
          </label>
        )}
        {(provider === 'claude-api' || keyOpen) && (
          <form
            className="set-key"
            onSubmit={(e) => {
              e.preventDefault();
              if (key.trim()) saveKey();
            }}
          >
            <input type="password" className="set-input mono" placeholder={hasKey ? 'Saved in the Keychain · paste a new key to replace it' : 'sk-ant-…'} value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" spellCheck={false} data-testid="settings-api-key" />
            <Button size="sm" type="submit" disabled={!key.trim()}>
              Save
            </Button>
          </form>
        )}
      </div>

      <h3>Compute</h3>
      {q && (
        <>
          <div>
            <Seg label="Compute" className="seg-wide set-seg" value={q.compute} onChange={(v) => set({ compute: v })} items={(['gentle', 'balanced', 'full'] as const).map((c) => ({ value: c, label: COMPUTE_LABEL[c] }))} />
            <p className="set-note" style={{ marginTop: 8 }}>
              {COMPUTE_NOTE[q.compute]}
              {perPhoto != null && ` Suggestions have taken about ${dur(perPhoto)} a photo lately.`}
            </p>
          </div>
          <Row title="Go to Full when the Mac is idle" note="Drops back as soon as you touch the keyboard or mouse">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button className="import-pick" disabled={!q.idle_boost_min}>
                  {q.idle_boost_min || 10} min <ChevronDown size={12} />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-[120px]">
                {[3, 5, 10, 20, 30].map((m) => (
                  <DropdownMenuItem key={m} onSelect={() => set({ idle_boost_min: m })}>
                    {m} min
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <Switch checked={q.idle_boost_min > 0} onCheckedChange={(c) => set({ idle_boost_min: c ? 10 : 0 })} aria-label="Go to Full when the Mac is idle" data-testid="settings-idle" />
          </Row>
          <Row title="Go to Full overnight" note="Culls and Suggestions catch up while you sleep">
            <span className="mono faint">
              {h0}:00 – {h1}:00
            </span>
            <Switch checked={q.overnight} onCheckedChange={(c) => set({ overnight: c })} aria-label="Go to Full overnight" data-testid="settings-overnight" />
          </Row>
          <Row title="Run AI only when you step away" note="Suggestions for a few photos run when the Mac is idle and plugged in; culls and big batches wait for overnight. Run now in Background work overrides it. The photo you’re on still runs">
            <Switch checked={q.idle_only !== false} onCheckedChange={(c) => set({ idle_only: c })} aria-label="Run AI only when you step away" data-testid="settings-idle-only" />
          </Row>
          <Row title="Run background AI on battery" note="Off: culls and Suggestions wait for the charger, and Full idle and overnight need it too. The photo you’re on still runs">
            <Switch checked={!!q.run_on_battery} onCheckedChange={(c) => set({ run_on_battery: c })} aria-label="Run background AI on battery" data-testid="settings-battery" />
          </Row>
          <Row title="The photo you’re on goes first" note="Suggestions for the open photo and the next few jump the queue and run at Full">
            <Switch checked={q.focus_first} onCheckedChange={(c) => set({ focus_first: c })} aria-label="The photo you're on goes first" data-testid="settings-focus" />
          </Row>
        </>
      )}

      <h3>Background work</h3>
      {q && (
        <Row title="Suggestions per photo" note="Each one is a whole edit, crop included. Nothing is applied until you accept it.">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button className="import-pick" data-testid="settings-takes">
                {q.takes} <ChevronDown size={12} />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[80px]">
              {[1, 2, 3, 4, 5].map((n) => (
                <DropdownMenuItem key={n} onSelect={() => set({ takes: n })}>
                  {n}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </Row>
      )}
    </div>
  );
}

const KEYS: [string, string][] = [
  ['P  X  U', 'Pick, Reject, Unflag'],
  ['E', 'Loupe'],
  ['C', 'Compare'],
  ['D', 'Edit'],
  ['⇧⌘C  ⇧⌘V', 'Copy and paste settings'],
  ['⌥⌘V', 'Paste the previous Photo’s settings'],
  ['⇧⌘S', 'Sync settings to the selection'],
  ['⌘K', 'Search or ask'],
  ['⌘Z  ⇧⌘Z', 'Undo, Redo'],
  ['⌘,', 'Settings'],
  ['Esc', 'Close a dialog'],
];

function KeysSection() {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '120px 1fr', rowGap: 8, fontSize: 13 }}>
      {KEYS.map(([k, what]) => (
        <div key={k} style={{ display: 'contents' }}>
          <span className="mono faint">{k}</span>
          <span>{what}</span>
        </div>
      ))}
    </div>
  );
}

/** Settings › Updates: the version, what the last check found, Check now and Restart to update. */
function UpdatesSection() {
  const s = useUpdate();
  const busy = s?.state === 'checking' || s?.state === 'downloading';
  const line = !s
    ? 'This build doesn’t update itself.'
    : {
        off: 'This build doesn’t update itself (dev or local build). Install from the download page.',
        idle: 'Checks soon after launch and every 6 hours.',
        checking: 'Checking…',
        downloading: `Downloading ${s.version}${s.total ? ` · ${Math.round((100 * s.done) / s.total)}%` : ''}. Keep working.`,
        ready: `Argus ${s.version} is installed. Restart to use it; your work is saved.`,
        current: 'You have the newest version.',
        error: `Couldn’t check: ${s.error ?? 'unknown error'}`,
      }[s.state];
  return (
    <div className="set-page" data-testid="settings-updates-page">
      <h3>Version</h3>
      <Row title={`Argus ${s?.current || '(browser prototype)'}`} note={line}>
        {s?.state === 'ready' ? (
          <Button variant="default" size="sm" onClick={() => void restartToUpdate()} data-testid="settings-update-restart">
            <ArrowUpCircle size={12} /> Restart to update
          </Button>
        ) : (
          <Button size="sm" onClick={() => void checkForUpdates()} disabled={busy || !s || s.state === 'off'} data-testid="settings-update-check">
            <RefreshCw size={12} className={busy ? 'spin-icon' : ''} /> Check now
          </Button>
        )}
      </Row>
      <Row title="Last checked" note="Updates download in the background and wait for a restart. Nothing changes mid-session.">
        <span className="mono faint" data-testid="settings-update-checked">
          {ago(s?.checked_at ?? null)}
        </span>
      </Row>
      {s?.notes && (s.state === 'ready' || s.state === 'downloading') && (
        <>
          <h3>What’s new in {s.version}</h3>
          <pre className="update-notes-full">{s.notes}</pre>
        </>
      )}
    </div>
  );
}

export const SECTIONS: SettingsSection[] = [
  { id: 'general', title: 'General', icon: <Settings2 size={14} />, render: () => <GeneralSection /> },
  { id: 'library', title: 'Library', icon: <Library size={14} />, render: () => <LibrarySection /> },
  { id: 'ai', title: 'AI and Compute', icon: <Sparkles size={14} />, render: () => <AiSection /> },
  { id: 'import', title: 'Import', icon: <Download size={14} />, render: () => <ImportSection /> },
  { id: 'export', title: 'Export', icon: <Upload size={14} />, render: () => <ExportSection /> },
  { id: 'keys', title: 'Shortcuts', icon: <Keyboard size={14} />, render: () => <KeysSection /> },
  { id: 'updates', title: 'Updates', icon: <RefreshCw size={14} />, render: () => <UpdatesSection /> },
];

export function SettingsDialog() {
  const [open, setOpen] = useState(false);
  const [section, setSection] = useState('ai');
  useEffect(() => {
    const show = (e: Event) => {
      const want = (e as CustomEvent<string | undefined>).detail;
      if (want && SECTIONS.some((s) => s.id === want)) setSection(want);
      setOpen(true);
    };
    window.addEventListener('argus:settings', show);
    // The app menu's Settings… item (⌘,) arrives as a Tauri event.
    let unlisten: (() => void) | undefined;
    if (IN_APP) void import('@tauri-apps/api/event').then(({ listen }) => listen('open-settings', () => openSettings()).then((u) => (unlisten = u)));
    return () => {
      window.removeEventListener('argus:settings', show);
      unlisten?.();
    };
  }, []);
  const current = SECTIONS.find((s) => s.id === section) ?? SECTIONS[0];
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        className="settings-v2 h-[min(650px,calc(100%-48px))] w-[min(980px,calc(100%-32px))] gap-0 p-0"
        closable={false}
        data-testid="settings"
        // Start focus on the selected section, not the first one: WebKit leaves focus where it was on click, so a
        // focused-but-unselected item would still look active.
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-testid="settings-${current.id}"]`)?.focus();
        }}
      >
        <nav aria-label="Settings sections" className="set-nav">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              className="settings-nav-item"
              onClick={(e) => {
                setSection(s.id);
                e.currentTarget.focus();
              }}
              aria-current={s.id === current.id ? 'page' : undefined}
              data-testid={`settings-${s.id}`}
            >
              {s.icon}
              {s.title}
            </button>
          ))}
        </nav>
        <section className="set-main">
          <header className="set-head">
            <DialogTitle className="set-h">{current.title}</DialogTitle>
            <DialogDescription className="sr-only">Argus settings</DialogDescription>
            <span className="grow" />
            <button className="import-link" onClick={() => setOpen(false)}>
              <Kbd>esc</Kbd> close
            </button>
          </header>
          <div className="set-body">{current.render()}</div>
        </section>
      </DialogContent>
    </Dialog>
  );
}
