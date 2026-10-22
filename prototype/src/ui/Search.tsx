// Search across the Library (engine `search_photos`) and Albums, its saved searches
// (`list_albums`, `save_album`, `delete_album`). ⌘K shows the best few Photos and the
// matching Keywords; ⌘↵ opens every match in the grid, which can be saved as an Album.

import { useEffect, useState } from 'react';
import { BookImage } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/menu';
import { call } from '../data/backend';
import { LIVE, foundShoots, openAlbum, openQuery, openSearch, openShoot, type Album } from '../data/live';
import { SHOOT, PHOTOS } from '../data/dummy';
import { usePoll, refreshPolls } from '../data/poll';
import { keywordCounts, keywordLabel, type KeywordCount } from '../data/tags';

export interface Found {
  count: number;
  shoots: number;
  /** `id` is the Photo's id as the UI keys it; `preview` is Library-relative (live only). */
  photos: { id: string; file_name: string; preview: string; matched: string[] }[];
  keywords: KeywordCount[];
}

/** What a query finds, for the palette: null while it is being asked. */
export function useSearch(query: string): Found | null {
  const [found, setFound] = useState<Found | null>(null);
  useEffect(() => {
    const q = query.trim();
    setFound(null);
    if (!q) return;
    let live = true;
    const t = setTimeout(async () => {
      if (LIVE) {
        const r = await call<Omit<Found, 'photos'> & { photos: { photo_id: number; file_name: string; preview: string; matched: string[] }[] }>('search_photos', { query: q, limit: 6 }).catch(() => null);
        if (live) setFound(r && { ...r, photos: r.photos.map((p) => ({ ...p, id: String(p.photo_id) })) });
        return;
      }
      // Dummy data: file names only, plus the demo Keywords.
      const hits = PHOTOS.filter((p) => p.file.toLowerCase().includes(q.toLowerCase()) || (p.reason ?? '').toLowerCase().includes(q.toLowerCase()));
      const keywords = await keywordCounts(q.split(/\s+/).pop() ?? '');
      if (live) setFound({ count: hits.length, shoots: hits.length ? 1 : 0, photos: hits.slice(0, 6).map((p) => ({ id: p.id, file_name: p.file, preview: '', matched: [p.reason] })), keywords });
    }, 120);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [query]);
  return found;
}


/** Save the words as an Album and open it. */
export async function saveAsAlbum(query: string) {
  const q = query.trim();
  if (!q || !LIVE) return;
  const name = q.charAt(0).toUpperCase() + q.slice(1);
  const album = await call<Album>('save_album', { name, query: q });
  refreshPolls('list_albums');
  openAlbum(album.id);
}

/** The grid header's line for a search or an Album: how much it found, and Save as Album. */
export function FoundLine() {
  if (!/^(search|album):/.test(SHOOT.id)) return null;
  const q = openQuery();
  return (
    <span className="row found-line">
      <span className="faint mono">
        {SHOOT.count} in {foundShoots} {foundShoots === 1 ? 'Shoot' : 'Shoots'}
      </span>
      {q && (
        <Button size="sm" variant="ghost" onClick={() => void saveAsAlbum(q)} data-testid="save-album">
          <BookImage size={13} /> Save as Album
        </Button>
      )}
    </span>
  );
}

/** The sidebar's Albums, above the Shoots (hidden until there is one). */
export function AlbumList() {
  const albums = usePoll<Album[]>(LIVE ? 'list_albums' : null, {}, 30000);
  if (!albums?.length) return null;
  return (
    <>
      <div className="nav-label">Albums</div>
      {albums.map((a) => {
        const on = SHOOT.id === `album:${a.id}`;
        return (
          <ContextMenu key={a.id}>
            <ContextMenuTrigger asChild>
              <button className={`nav-item ${on ? 'active' : ''}`} aria-current={on || undefined} onClick={() => openAlbum(a.id)} title={a.query.text ? `Finds “${a.query.text}”` : undefined} data-testid="nav-album">
                <BookImage size={14} />
                <span className="trunc">{a.name}</span>
                <span className="count">{a.photos.toLocaleString()}</span>
              </button>
            </ContextMenuTrigger>
            <ContextMenuContent>
              <ContextMenuItem onSelect={() => openSearch(a.query.text ?? '')}>Show as a search</ContextMenuItem>
              <ContextMenuItem
                onSelect={async () => {
                  await call('delete_album', { album_id: a.id });
                  refreshPolls('list_albums');
                  if (on) openShoot('all');
                }}
              >
                Delete Album
              </ContextMenuItem>
            </ContextMenuContent>
          </ContextMenu>
        );
      })}
    </>
  );
}

/** One palette row's caption for a hit: what matched, as chip labels. */
export const matchedText = (m: string[]) => m.map((x) => (x.includes('|') ? keywordLabel(x) : x)).join(', ');

