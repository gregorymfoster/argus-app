// Settings › Library › Removed: what Remove from Library took out in the last 30 days, each
// with Restore, and Empty to forget them for good (their previews go; originals never do).

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { emptyRemoved, listRemoved, restoreRemoved, type Removal } from '../data/remove';
import { openShoot } from '../data/live';
import { SHOOT } from '../data/dummy';

export function RemovedSection() {
  const [list, setList] = useState<Removal[] | null>(null);
  const load = () => void listRemoved().then(setList, () => setList([]));
  useEffect(load, []);
  if (!list || list.length === 0) return null;
  const fail = (e: unknown) => toast(String(e).replace(/^Error: /, ''), { id: 'argus' });
  return (
    <>
      <h3>Removed</h3>
      {list.map((r) => (
        <div key={r.removal} className="lib-row" data-testid="removed-row">
          <span>{r.label}</span>
          <span className="faint mono">{r.at.slice(0, 10)}</span>
          <span className="grow" />
          <Button size="sm" onClick={() => void restoreRemoved(r.removal).then(() => openShoot(SHOOT.id), fail)}>
            Restore
          </Button>
        </div>
      ))}
      <div className="lib-row">
        <span className="faint">Kept 30 days so they can come back. Originals are never deleted here.</span>
        <span className="grow" />
        <Button size="sm" variant="ghost" onClick={() => void emptyRemoved().then(load, fail)}>
          Empty
        </Button>
      </div>
    </>
  );
}
