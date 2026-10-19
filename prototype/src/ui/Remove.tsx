// The Remove from Library confirm, like Lightroom's: what goes, that originals stay unless
// you tick the box, and that Undo brings it all back. Ask from anywhere with `askRemove`.

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Trash2 } from 'lucide-react';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { removePhotos, removeShoot, undoRemoval, type Removed } from '../data/remove';
import { useLibrary } from '../store/library';
import { PHOTOS } from '../data/dummy';
import { Kbd } from './kit';

type Ask = { photos: string[]; after?: () => void } | { shoot: { id: string; name: string; count: number } };

/** Open the Remove confirm for Photos of the open Shoot, or a whole Shoot. */
export function askRemove(what: Ask) {
  window.dispatchEvent(new CustomEvent('argus:remove', { detail: what }));
}

const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;

export function RemoveDialog() {
  const { dispatch } = useLibrary();
  const [ask, setAsk] = useState<Ask | null>(null);
  const [originals, setOriginals] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const on = (e: Event) => {
      setAsk((e as CustomEvent<Ask>).detail);
      setOriginals(false);
    };
    window.addEventListener('argus:remove', on);
    return () => window.removeEventListener('argus:remove', on);
  }, []);

  const undo = () =>
    void undoRemoval().then(
      (n) => n != null && dispatch({ type: 'photos/changed', imported: n }),
      (e) => toast(`Couldn’t put it back: ${String(e).replace(/^Error: /, '')}`, { id: 'argus' }),
    );

  const confirm = async () => {
    if (!ask || busy) return;
    setBusy(true);
    try {
      let removed: Removed | null;
      if ('photos' in ask) {
        const r = await removePhotos(ask.photos, originals);
        removed = r.removed;
        dispatch({ type: 'photos/changed', imported: r.shown });
        ask.after?.();
      } else {
        removed = await removeShoot(ask.shoot.id, originals);
        dispatch({ type: 'photos/changed', imported: PHOTOS.length });
      }
      setAsk(null);
      if (removed) showRemoved(removed, undo);
    } catch (e) {
      toast(`Not removed: ${String(e).replace(/^Error: /, '')}`, { id: 'argus' });
    } finally {
      setBusy(false);
    }
  };

  const n = !ask ? 0 : 'photos' in ask ? ask.photos.length : ask.shoot.count;
  const title = !ask ? '' : 'photos' in ask ? `Remove ${plural(n, 'Photo')} from the Library?` : `Remove ${ask.shoot.name} from the Library?`;
  return (
    <Dialog open={!!ask} onOpenChange={(o) => !o && !busy && setAsk(null)}>
      <DialogContent className="remove-dialog" closable={false} data-testid="remove-dialog" onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), void confirm())}>
        <DialogTitle className="remove-title">{title}</DialogTitle>
        <DialogDescription className="remove-note">
          {ask && 'shoot' in ask ? `Its ${plural(n, 'Photo')} go with their` : n === 1 ? 'It goes with its' : 'They go with their'} flags, stars, edits and Suggestions. Undo, or ⌘Z right after, brings everything back.
        </DialogDescription>
        <label className="remove-check">
          <Checkbox checked={originals} onCheckedChange={(c) => setOriginals(c === true)} data-testid="remove-originals" />
          <span>
            Also move the original files to the Trash
            <span className="remove-sub">Otherwise they stay where they are. Undo puts them back while they’re still in the Trash.</span>
          </span>
        </label>
        <div className="remove-foot">
          <button className="import-link" onClick={() => setAsk(null)} disabled={busy}>
            <Kbd>esc</Kbd> cancel
          </button>
          <span className="grow" />
          <Button variant="reject" size="sm" onClick={() => void confirm()} disabled={busy} data-testid="remove-confirm">
            <Trash2 size={12} /> {originals ? 'Remove and trash originals' : 'Remove'} <Kbd>↵</Kbd>
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** The toast after a removal, with Undo. Also used after the reload a Shoot removal causes. */
export function showRemoved(r: Removed, undo: () => void) {
  toast(r.label, {
    id: 'argus',
    description: r.errors.length
      ? `${plural(r.errors.length, 'file')} couldn’t move to the Trash: ${r.errors[0]}`
      : r.originals_trashed
        ? `${plural(r.originals_trashed, 'original')} moved to the Trash`
        : 'The originals stay where they are',
    action: { label: 'Undo', onClick: undo },
    duration: 10000,
  });
}
