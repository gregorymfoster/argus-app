import { useEffect, useState } from 'react';
import { LIVE } from '../data/live';
import { FlaskConical, X } from 'lucide-react';
import { PHOTOS } from '../data/dummy';
import { useLibrary } from '../store/library';
import { DECISIONS, feedbackMarkdown, useVariants, type Screen } from '../store/variants';
import { go } from '../router';
import { CommandDialog, CommandGroup, CommandInput, CommandFooter, CommandItem, CommandList } from '@/components/ui/command';
import { Button } from '@/components/ui/button';
import { Kbd, MenuKeys } from './kit';
import { openSearch } from '../data/live';
import { PHOTO_BY_ID } from '../data/dummy';
import { libraryUrl } from '../data/backend';
import { PhotoView } from './PhotoView';
import { keywordLabel } from '../data/tags';
import { matchedText, saveAsAlbum, useSearch } from './Search';

export interface Command {
  label: string;
  hint?: string;
  run: () => void;
}

/** Hand a request to the grid's Agent, opening the grid first if another screen is up. */
function askFromPalette(text: string) {
  go({ name: 'cull' });
  setTimeout(() => window.dispatchEvent(new CustomEvent('argus:ask', { detail: text })), 150);
}

/** ⌘K, built on cmdk (fuzzy filter and keyboard handling come from it). */
export function Palette({ open, onOpenChange, extra }: { open: boolean; onOpenChange: (o: boolean) => void; extra: Command[] }) {
  const { state, dispatch } = useLibrary();
  const [query, setQuery] = useState('');
  useEffect(() => {
    if (!open) setQuery('');
  }, [open]);
  const found = useSearch(open ? query : '');
  const q = query.trim();
  const showAll = () => {
    onOpenChange(false);
    if (LIVE) openSearch(q);
    else dispatch({ type: 'toast', text: 'Search across Shoots needs a real Library' });
  };
  const groups: [string, Command[]][] = [
    [
      'Go',
      [
        { label: 'Cull', hint: 'G', run: () => go({ name: 'cull' }) },
        { label: 'Set a Look for the Shoot', hint: 'L', run: () => go({ name: 'look' }) },
        { label: 'Import a Shoot', hint: '⇧⌘I', run: () => go({ name: 'import' }) },
      ],
    ],
    [
      'Actions',
      [
        ...extra,
        { label: 'Undo', hint: '⌘Z', run: () => dispatch({ type: 'undo' }) },
        { label: 'Redo', hint: '⇧⌘Z', run: () => dispatch({ type: 'redo' }) },
        // Dummy data only: live, the Library is the truth and a reset would hide it.
        ...(LIVE ? [] : [{ label: 'Reset the demo', run: () => dispatch({ type: 'reset' }) }]),
      ],
    ],
    ['Photos', PHOTOS.slice(0, state.imported).map((p) => ({ label: `Open ${p.file}`, hint: p.reason, run: () => go({ name: 'loupe', id: p.id }) }))],
  ];
  return (
    <CommandDialog open={open} onOpenChange={onOpenChange} label="Command palette">
      <CommandInput
        placeholder="Search photos by Keyword or caption, type a command, or ask the Agent…"
        value={query}
        onValueChange={setQuery}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && q) {
            e.preventDefault();
            showAll();
          }
        }}
      />
      <CommandList>
        {found && found.count > 0 && (
          <CommandGroup heading={`Photos · ${found.count} in ${found.shoots} ${found.shoots === 1 ? 'Shoot' : 'Shoots'}`} forceMount data-testid="palette-photos">
            <div className="found-strip">
              {found.photos.map((h) => (
                <button
                  key={h.id}
                  className="found-thumb"
                  title={h.file_name}
                  onClick={() => {
                    onOpenChange(false);
                    if (PHOTO_BY_ID[h.id] && !LIVE) go({ name: 'loupe', id: h.id });
                    else showAll();
                  }}
                >
                  {LIVE ? <img src={libraryUrl(h.preview)} alt="" loading="lazy" /> : PHOTO_BY_ID[h.id] && <PhotoView photo={PHOTO_BY_ID[h.id]} size="tile" style={{ width: '100%', height: '100%', borderRadius: 0 }} />}
                  <span className="trunc">{matchedText(h.matched) || h.file_name}</span>
                </button>
              ))}
            </div>
            <CommandItem value="search-show-all" forceMount onSelect={showAll} data-testid="palette-show-all">
              <span className="grow">Show all {found.count} in the grid</span>
              <MenuKeys keys={['⌘', '↵']} />
            </CommandItem>
          </CommandGroup>
        )}
        {found && found.keywords.length > 0 && (
          <CommandGroup heading="Keywords" forceMount>
            {found.keywords.map((k) => (
              <CommandItem
                key={k.keyword}
                value={`keyword ${k.keyword}`}
                forceMount
                onSelect={() => {
                  const leaf = k.keyword.split('|').pop() ?? '';
                  onOpenChange(false);
                  if (LIVE) openSearch(leaf);
                  else setQuery(leaf);
                }}
              >
                <span className="grow">{k.keyword.split('|')[0]} › {keywordLabel(k.keyword)}</span>
                <span className="faint" style={{ fontSize: 12 }}>
                  {k.photos.toLocaleString()} {k.photos === 1 ? 'photo' : 'photos'}
                </span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {LIVE && found && found.count > 0 && (
          <CommandGroup heading="Search" forceMount>
            <CommandItem
              value="search-save-album"
              forceMount
              onSelect={() => {
                onOpenChange(false);
                void saveAsAlbum(q);
              }}
              data-testid="palette-save-album"
            >
              <span className="grow">Save this search as an Album</span>
              <span className="faint" style={{ fontSize: 12 }}>
                updates as you import
              </span>
            </CommandItem>
          </CommandGroup>
        )}
                {groups.map(([heading, items]) => (
          <CommandGroup key={heading} heading={heading}>
            {items.map((c) => (
              <CommandItem
                key={c.label}
                value={`${c.label} ${c.hint ?? ''}`}
                onSelect={() => {
                  onOpenChange(false);
                  c.run();
                }}
              >
                <span className="grow">{c.label}</span>
                {c.hint && (c.hint.length <= 3 ? <Kbd>{c.hint}</Kbd> : <span className="faint" style={{ fontSize: 12 }}>{c.hint}</span>)}
              </CommandItem>
            ))}
          </CommandGroup>
        ))}
        {query.trim() && (
          <CommandGroup heading="Agent" forceMount>
            <CommandItem
              value="agent-ask"
              forceMount
              onSelect={() => {
                onOpenChange(false);
                askFromPalette(query.trim());
              }}
              data-testid="palette-ask"
            >
              <span className="grow">Ask the Agent: “{query.trim()}”</span>
              <Kbd>↵</Kbd>
            </CommandItem>
          </CommandGroup>
        )}
      </CommandList>
      <CommandFooter>{found && found.count > 0 ? <span className="flex items-center gap-1.5"><MenuKeys keys={['⌘', '↵']} /> All in the grid</span> : null}</CommandFooter>
    </CommandDialog>
  );
}

/** Feedback panel for open design questions. Hidden when DECISIONS is empty. */
export function VariantsPanel({ screen }: { screen: Screen }) {
  const { choice, notes, set, note } = useVariants();
  const [open, setOpen] = useState(true);
  const [copied, setCopied] = useState(false);
  const [fallback, setFallback] = useState<string | null>(null);
  const here = DECISIONS.filter((d) => d.screens === 'all' || d.screens.includes(screen));
  const [hl, setHl] = useState<string | null>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.shiftKey && e.key.toLowerCase() === 'v' && !(e.target as HTMLElement).closest('input,textarea')) setOpen((o) => !o);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
  // Open questions are for the design prototype; the app runs the default until Greg picks.
  if (!DECISIONS.length || LIVE) return null;
  const copy = async () => {
    const md = feedbackMarkdown(choice, notes);
    try {
      await navigator.clipboard.writeText(md);
      setFallback(null);
    } catch {
      setFallback(md);
      return;
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };
  return (
    <>
      {hl && <style>{`[data-variant~="${hl}"] { outline: 2px dashed var(--shoot) !important; outline-offset: -2px; }`}</style>}
      <button className="variants-fab" onClick={() => setOpen(!open)} aria-expanded={open}>
        <FlaskConical size={13} /> Variants · {here.length} here <kbd style={{ background: 'transparent', borderColor: '#5a4a2a', color: 'var(--shoot)' }}>⇧V</kbd>
      </button>
      {open && (
        <div className="variants" role="dialog" aria-label="Design variants">
          <div className="row">
            <h2>Try the alternatives</h2>
            <span className="grow" />
            <Button variant="ghost" size="icon" onClick={() => setOpen(false)} aria-label="Close variants">
              <X size={12} />
            </Button>
          </div>
          {here.map((d) => {
            const cur = choice[d.id] ?? d.options[0].id;
            const opt = d.options.find((o) => o.id === cur)!;
            return (
              <div key={d.id} className="decision" onMouseEnter={() => setHl(d.id)} onMouseLeave={() => setHl(null)}>
                <div className="q">{d.question}</div>
                <div className="opts">
                  {d.options.map((o) => (
                    <button key={o.id} className={o.id === cur ? 'on' : ''} onClick={() => set(d.id, o.id)}>
                      {o.label}
                    </button>
                  ))}
                </div>
                <div className="note">{opt.note}</div>
                <input placeholder="Your take (optional)" value={notes[d.id] ?? ''} onChange={(e) => note(d.id, e.target.value)} aria-label={`Notes on ${d.question}`} />
              </div>
            );
          })}
          <div className="decision">
            <div className="q">Anything else on this flow?</div>
            <input placeholder="General notes" value={notes.general ?? ''} onChange={(e) => note('general', e.target.value)} aria-label="General notes" />
          </div>
          <Button onClick={copy} style={{ alignSelf: 'flex-start', borderColor: '#5a4a2a', color: 'var(--shoot)' }}>
            {copied ? 'Copied' : 'Copy all feedback to paste in the chat'}
          </Button>
          {fallback && (
            <textarea
              readOnly
              aria-label="Feedback to copy"
              value={fallback}
              rows={8}
              onFocus={(e) => e.target.select()}
              ref={(el) => el?.select()}
              style={{ background: '#110f0a', border: '1px solid #3a3222', borderRadius: 8, padding: 8, fontSize: 12, resize: 'vertical' }}
            />
          )}
        </div>
      )}
    </>
  );
}
