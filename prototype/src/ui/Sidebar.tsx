import { Clock, Download, Flag, Images, Plus, Search } from 'lucide-react';
import { OTHER_SHOOTS, PHOTOS, SHOOT } from '../data/dummy';
import { useState } from 'react';
import { LIVE, inCollection, openShoot, openShootToExport } from '../data/live';
import { ShootItem } from './ShootItem';
import { useLibrary, verdictOf } from '../store/library';
import { go, type Route } from '../router';
import { QueueProgress } from './Queue';
import { UpdateButton } from './Updates';
import { Kbd } from './kit';

/**
 * The Library sidebar (Design pass v2): search, the Library's views, every Shoot newest
 * first with the open one marked, then Import and the background work at the foot.
 * Views of one Shoot (grid, loupe, compare, edit) live in the Cull header instead.
 */
export function Sidebar({ route, onPalette, onExport, onAuto, onCull }: { route: Route; onPalette: () => void; onExport: () => void; onAuto: () => void; onCull: () => void }) {
  const { state, dispatch } = useLibrary();
  const picks = PHOTOS.slice(0, state.imported).filter((p) => verdictOf(state, p.id) === 'pick').length;
  // A rename changes SHOOT / OTHER_SHOOTS in place; the toast re-renders every screen
  // that shows the name (the Cull header too).
  const [, setRenames] = useState(0);
  const renamed = (name: string) => {
    setRenames((n) => n + 1);
    dispatch({ type: 'toast', text: `Renamed to ${name}` });
  };
  // The prototype's dummy data is one Shoot; the real Library opens these views.
  const later = () => dispatch({ type: 'toast', text: 'Library-wide views need a real Library' });
  const collection = inCollection();
  const view = (id: 'all' | 'picks') => (SHOOT.id === id ? go({ name: 'cull' }) : LIVE ? openShoot(id) : later());
  const current = { ...SHOOT, count: state.imported };
  const shoots = (SHOOT.id === 'none' || collection ? OTHER_SHOOTS : [current, ...OTHER_SHOOTS]).slice().sort((a, b) => b.name.localeCompare(a.name));
  const total = shoots.reduce((n, s) => n + s.count, 0);
  // The newest Import: live Shoot ids grow with each one; dummy Shoots go by name.
  const newest = LIVE ? shoots.reduce<(typeof shoots)[number] | undefined>((a, s) => (!a || Number(s.id) > Number(a.id) ? s : a), undefined) : shoots[0];
  // While an Import fills the open Shoot, its row shows how far along it is.
  const importPct = state.importing && state.importTotal ? Math.round((100 * (state.importDone ?? 0)) / state.importTotal) : null;
  return (
    <nav className="sidebar" aria-label="Library">
      <div className="sidebar-top" aria-hidden="true" />
      <button className="search" onClick={onPalette}>
        <Search size={13} />
        Search or jump
        <span className="grow" />
        <kbd>⌘K</kbd>
      </button>
      <div className="nav-label">Library</div>
      <button className={`nav-item ${SHOOT.id === 'all' && route.name !== 'import' ? 'active' : ''}`} onClick={() => view('all')} aria-current={SHOOT.id === 'all' || undefined} data-testid="nav-all">
        <Images size={14} />
        All photos
        <span className="count">{total.toLocaleString()}</span>
      </button>
      <button className={`nav-item ${SHOOT.id === 'picks' && route.name !== 'import' ? 'active' : ''}`} onClick={() => view('picks')} aria-current={SHOOT.id === 'picks' || undefined} data-testid="nav-picks">
        <Flag size={14} />
        Picks
        <span className="count">{SHOOT.id === 'picks' ? state.imported : collection ? '' : picks || ''}</span>
      </button>
      <button className="nav-item" onClick={() => newest && (newest.id === SHOOT.id ? go({ name: 'cull' }) : LIVE ? openShoot(newest.id) : later())} disabled={!newest}>
        <Clock size={14} />
        Last import
        <span className="count">{newest?.count ?? ''}</span>
      </button>
      <div className="nav-label row">
        Shoots
        <span className="grow" />
        <button className="nav-add" title="Import a Shoot" aria-label="Import a Shoot" onClick={() => go({ name: 'import' })}>
          <Plus size={13} />
        </button>
      </div>
      <div className="shoot-list">
        {shoots.map((s) =>
          s.id === SHOOT.id ? (
            <ShootItem key={s.id} shoot={s} current active={route.name !== 'import'} progress={importPct} onOpen={() => go({ name: 'cull' })} onExport={onExport} onRenamed={renamed} onAuto={onAuto} onCull={onCull} onLook={() => go({ name: 'look' })} />
          ) : (
            <ShootItem
              key={s.id}
              shoot={s}
              onOpen={() => (LIVE ? openShoot(s.id) : dispatch({ type: 'toast', text: 'The prototype has one Shoot' }))}
              onExport={() => (LIVE ? openShootToExport(s.id) : dispatch({ type: 'toast', text: 'The prototype has one Shoot' }))}
              onRenamed={renamed}
            />
          ),
        )}
      </div>
      <span className="grow" />
      <UpdateButton />
      <button className={`nav-import ${route.name === 'import' ? 'active' : ''}`} onClick={() => go({ name: 'import' })} data-testid="nav-import">
        <Download size={13} />
        Import
        <span className="grow" />
        <span className="keys">
          <Kbd>⌘</Kbd>
          <Kbd>⇧</Kbd>
          <Kbd>I</Kbd>
        </span>
      </button>
      <QueueProgress />
    </nav>
  );
}
