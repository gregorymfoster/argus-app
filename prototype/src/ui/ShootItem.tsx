// A Shoot in the sidebar. Double-click or Enter renames it in place (Esc cancels), as in
// Lightroom's Collections and Finder; right-click has Rename, Suggest a name, Show in
// Finder, and for the open Shoot Auto all, the AI cull, a Look and Export Picks. The engine stores the name by the naming rules
// (`2026-10-10_Boat-Tour`) and moves the Shoot's originals folder with it.

import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import type { Shoot } from '../domain/types';
import { LIVE, aiName, renameShoot, revealInFinder, suggestShootName } from '../data/live';
import { Download, Folder, FolderOpen, Pencil, Rows3, Sparkles, Wand2, Trash2 } from 'lucide-react';
import { askRemove } from './Remove';
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from '@/components/ui/menu';
import { Kbd, MenuKeys } from './kit';

const message = (e: unknown) => String(e).replace(/^Error: /, '');

/** Select the title of `2026-10-10_Boat-Tour`; after a bare date, add `_` and put the caret
 * there, so typing "Boat tour" gives `2026-10-10_Boat-tour`. Other names select whole. */
function selectTitle(input: HTMLInputElement) {
  const date = /^\d{4}-\d{2}-\d{2}/.exec(input.value)?.[0];
  if (!date) return input.select();
  if (input.value === date) input.value += '_';
  input.setSelectionRange(date.length + 1, input.value.length);
}

export function ShootItem({
  shoot,
  active,
  current,
  progress,
  onOpen,
  onExport,
  onRenamed,
  onAuto,
  onCull,
  onLook,
}: {
  shoot: Shoot;
  active?: boolean;
  current?: boolean;
  /** Percent imported, while an Import fills this Shoot. */
  progress?: number | null;
  onOpen: () => void;
  onExport: () => void;
  onRenamed: (name: string) => void;
  onAuto?: () => void;
  onCull?: () => void;
  onLook?: () => void;
}) {
  // null: showing the name; a string: editing, with that as the starting text.
  const [draft, setDraft] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const row = useRef<HTMLButtonElement>(null);
  const done = useRef(false);

  useEffect(() => {
    if (draft == null || !input.current) return;
    input.current.focus();
    selectTitle(input.current);
  }, [draft]);

  const start = (text = shoot.name) => {
    done.current = false;
    setDraft(text);
  };
  const stop = () => {
    setDraft(null);
    requestAnimationFrame(() => row.current?.focus());
  };
  const commit = async (text: string) => {
    if (done.current) return;
    done.current = true;
    const typed = text.trim();
    if (!typed || typed === shoot.name) return stop();
    try {
      const name = await renameShoot(shoot.id, typed);
      onRenamed(name);
    } catch (e) {
      toast(`Not renamed: ${message(e)}`, { id: 'argus' });
    }
    stop();
  };
  const suggest = async () => {
    setAsking(true);
    try {
      const s = await suggestShootName(shoot.id);
      if (s.note) toast(`${s.by === 'ai' ? '' : 'No AI name: '}${s.note}`, { id: 'argus' });
      start(s.name);
    } catch (e) {
      toast(`No name suggested: ${message(e)}`, { id: 'argus' });
    } finally {
      setAsking(false);
    }
  };
  const reveal = async () => {
    try {
      await revealInFinder({ shoot_id: Number(shoot.id) });
    } catch (e) {
      toast(message(e), { id: 'argus' });
    }
  };

  if (draft != null) {
    return (
      <div className="shoot-editing">
        <div className={`nav-item editing ${active ? 'active' : ''}`}>
          <input
            ref={input}
            className="nav-rename mono"
            defaultValue={draft}
            aria-label="Shoot name"
            spellCheck={false}
            data-testid="shoot-rename"
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === 'Enter') void commit(e.currentTarget.value);
              if (e.key === 'Escape') {
                done.current = true;
                stop();
              }
            }}
            onBlur={(e) => void commit(e.currentTarget.value)}
          />
        </div>
        <div className="rename-hint">
          <Kbd quiet>↵</Kbd> save <Kbd quiet>esc</Kbd> cancel <span className="grow" /> folder follows
        </div>
      </div>
    );
  }

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <button
          ref={row}
          className={`nav-item shoot ${active ? 'active' : ''} ${current ? 'current' : ''}`}
          onClick={onOpen}
          onDoubleClick={() => start()}
          onKeyDown={(e) => {
            if (e.key !== 'Enter') return;
            e.preventDefault();
            e.stopPropagation();
            start();
          }}
          aria-current={current || undefined}
          title={`${shoot.name}: double-click to rename`}
          data-testid={`shoot-${shoot.id}`}
        >
          {current ? <FolderOpen size={14} /> : <Folder size={14} />}
          <span className="nav-name">{asking ? `Asking ${aiName()}…` : shoot.name}</span>
          <span className={`count ${progress != null ? 'busy' : ''}`}>{progress != null ? `${progress}%` : shoot.count}</span>
        </button>
      </ContextMenuTrigger>
      <ContextMenuContent>
        {!current && <ContextMenuItem onSelect={onOpen}>Open</ContextMenuItem>}
        <ContextMenuItem onSelect={() => start()}>
          <Pencil size={13} /> Rename <MenuKeys keys={['↵']} />
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => void suggest()} disabled={asking}>
          <Sparkles size={13} /> Suggest a name
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => void reveal()} disabled={!LIVE}>
          <Folder size={13} /> Show in Finder
        </ContextMenuItem>
        {current && (
          <>
            <ContextMenuSeparator />
            {onAuto && (
              <ContextMenuItem onSelect={onAuto}>
                <Wand2 size={13} /> Auto all <MenuKeys keys={['⇧', 'A']} />
              </ContextMenuItem>
            )}
            {onCull && (
              <ContextMenuItem onSelect={onCull}>
                <Sparkles size={13} /> Run AI cull again
              </ContextMenuItem>
            )}
            {onLook && (
              <ContextMenuItem onSelect={onLook}>
                <Rows3 size={13} /> Set a Look <MenuKeys keys={['L']} />
              </ContextMenuItem>
            )}
          </>
        )}
        <ContextMenuItem onSelect={onExport}>
          <Download size={13} /> Export Picks… <MenuKeys keys={['⌘', '⇧', 'E']} />
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => askRemove({ shoot: { id: shoot.id, name: shoot.name, count: shoot.count } })} data-testid="shoot-remove">
          <Trash2 size={13} /> Remove from Library…
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
