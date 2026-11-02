import { useEffect, useState } from 'react';
import { routeSwap } from './ui/motion';

export type Route =
  | { name: 'import' }
  | { name: 'cull' }
  | { name: 'loupe'; id: string }
  | { name: 'compare'; ids: string[] }
  // Cull mode (⌘2): the Shoot moment by moment. `id` opens on that Stack or Photo.
  // `pass` 2 and on is a Round over the Picks (#/round/2, #/round/2/<id>).
  | { name: 'moments'; id?: string; pass?: number }
  // Export mode (⌘4).
  | { name: 'export' }
  | { name: 'look' }
  | { name: 'edit'; id: string }
  | { name: 'people'; id?: string };

function parse(hash: string): Route {
  const [, name, arg, more] = hash.replace(/^#/, '').split('/');
  if (name === 'round' && Number(arg) > 1) return more ? { name: 'moments', pass: Number(arg), id: more } : { name: 'moments', pass: Number(arg) };
  if ((name === 'loupe' || name === 'edit') && arg) return { name, id: arg };
  if (name === 'compare' && arg) return { name, ids: arg.split(',') };
  // `cull` is the Library's grid (⌘1), named before the modes; `library` is its alias, and
  // `bursts` the old name of Cull mode.
  if (name === 'moments' || name === 'bursts') return arg ? { name: 'moments', id: arg } : { name: 'moments' };
  if (name === 'people') return arg ? { name, id: arg } : { name };
  if (name === 'library') return { name: 'cull' };
  if (name === 'cull' || name === 'look' || name === 'import' || name === 'export') return { name };
  return { name: 'import' };
}

function href(r: Route) {
  if (r.name === 'compare') return `#/compare/${r.ids.join(',')}`;
  if (r.name === 'moments' && r.pass && r.pass > 1) return `#/round/${r.pass}${r.id ? `/${r.id}` : ''}`;
  return 'id' in r && r.id ? `#/${r.name}/${r.id}` : `#/${r.name}`;
}

// Where Esc in the Loupe returns to when a mode opened it (Cull mode's Space), instead of the grid.
let back: Route | null = null;
export const openFrom = (from: Route, to: Route) => ((back = from), go(to));
/** The Route to return to, once; null means the grid. */
export const takeBack = (): Route | null => {
  const r = back;
  back = null;
  return r;
};

export function go(r: Route) {
  location.hash = href(r);
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parse(location.hash));
  useEffect(() => {
    // Screen to screen goes through routeSwap, so the photo can fly between grid and Loupe.
    let cur = parse(location.hash).name;
    const on = () => {
      const next = parse(location.hash);
      const from = cur;
      cur = next.name;
      routeSwap(from, next.name, () => setRoute(next));
    };
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);
  return route;
}
