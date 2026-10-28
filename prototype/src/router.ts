import { useEffect, useState } from 'react';
import { routeSwap } from './ui/motion';

export type Route =
  | { name: 'import' }
  | { name: 'cull' }
  | { name: 'loupe'; id: string }
  | { name: 'compare'; ids: string[] }
  | { name: 'bursts'; id?: string }
  | { name: 'look' }
  | { name: 'edit'; id: string }
  | { name: 'people'; id?: string };

function parse(hash: string): Route {
  const [, name, arg] = hash.replace(/^#/, '').split('/');
  if ((name === 'loupe' || name === 'edit') && arg) return { name, id: arg };
  if (name === 'compare' && arg) return { name, ids: arg.split(',') };
  if (name === 'bursts' || name === 'people') return arg ? { name, id: arg } : { name };
  if (name === 'cull' || name === 'look' || name === 'import') return { name };
  return { name: 'import' };
}

function href(r: Route) {
  if (r.name === 'compare') return `#/compare/${r.ids.join(',')}`;
  return 'id' in r && r.id ? `#/${r.name}/${r.id}` : `#/${r.name}`;
}

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
