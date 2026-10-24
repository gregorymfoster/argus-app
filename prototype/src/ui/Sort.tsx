// The grid's Sort menu (Lightroom's Sort: Capture time, Rating, Pick, File name, with a
// direction). Remembered for this viewer; the Loupe and its filmstrip follow the same order.

import { Fragment } from 'react';
import { ArrowDownWideNarrow, ArrowUpNarrowWide, Check, Sparkles } from 'lucide-react';
import { SORTS, sortBy, sortFromString, sortToString, type Sort } from '../domain/sort';
import { usePref } from '../store/variants';
import { MenuKeys, Tip } from './kit';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu';

export function useSort(): [Sort, (s: Sort) => void] {
  const [v, set] = usePref<string>('grid-sort', 'capture:asc');
  return [sortFromString(v), (s) => set(sortToString(s))];
}

/** ⇧S: Stars, most first, and back to Capture time. */
export const toggleStars = (s: Sort): Sort => (s.key === 'stars' ? sortBy('capture') : sortBy('stars'));
/** ⇧K: Keepers first, and back to Capture time. */
export const toggleKeepers = (s: Sort): Sort => (s.key === 'keepers' ? sortBy('capture') : sortBy('keepers'));

export function SortMenu({ sort, onChange, rank }: { sort: Sort; onChange: (s: Sort) => void; rank?: boolean }) {
  const def = SORTS.find((s) => s.key === sort.key)!;
  const Dir = sort.key === 'keepers' ? Sparkles : sort.desc ? ArrowDownWideNarrow : ArrowUpNarrowWide;
  // Best Photos keep their ranked order while the sort is Capture time.
  const label = rank && sort.key === 'capture' ? 'Rank' : def.short;
  return (
    <DropdownMenu>
      <Tip label={`Sort: ${rank && sort.key === 'capture' ? 'Best Photos rank' : `${def.label}, ${def.dirs[sort.desc ? 1 : 0].toLowerCase()}`}`} keys="⇧S">
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" aria-label="Sort" data-testid="sort" data-sort={sortToString(sort)} className="stars-trigger">
            <Dir size={12} /> <span className="wide-only">{label}</span>
          </Button>
        </DropdownMenuTrigger>
      </Tip>
      <DropdownMenuContent align="end">
        <div className="menu-label">Sort by</div>
        {SORTS.map((s) => (
          <Fragment key={s.key}>
            {s.key === 'keepers' && <div className="menu-sep" />}
            <DropdownMenuItem data-testid={`sort-${s.key}`} onSelect={() => s.key !== sort.key && onChange(sortBy(s.key))}>
              <span style={{ width: 14 }}>{s.key === sort.key && <Check size={12} />}</span>
              {s.key === 'keepers' && <Sparkles size={12} className="accent-text" />}
              <span className="grow">{s.label}</span>
              {s.key === 'stars' && <MenuKeys keys={['⇧', 'S']} />}
              {s.key === 'keepers' && <MenuKeys keys={['⇧', 'K']} />}
            </DropdownMenuItem>
          </Fragment>
        ))}
        {sort.key === 'keepers' ? (
          <div className="menu-note">Unflagged frames ahead re-order as you flag, likeliest keeper first. What you've passed never moves; Stacks stay together.</div>
        ) : (
          <>
            <div className="menu-sep" />
            {def.dirs.map((d, i) => (
              <DropdownMenuItem key={d} data-testid={`sort-${i ? 'desc' : 'asc'}`} onSelect={() => onChange({ key: sort.key, desc: i === 1 })}>
                <span style={{ width: 14 }}>{sort.desc === (i === 1) && <Check size={12} />}</span>
                <span className="grow">{d}</span>
              </DropdownMenuItem>
            ))}
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
