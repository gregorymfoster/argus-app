// The People view (as mocked, 2026-10-22), like Lightroom's: everyone named, then groups
// of faces that look like one person to name in one go. A person's page shows their Photos
// and asks "Is this Maya?" about faces that might be them. The AI groups and asks; only the
// person names anyone (engine `people`, data/people.ts).

import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { useHotkeys } from 'react-hotkeys-hook';
import { ChevronRight, Check, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/menu';
import { Kbd, Seg } from '../ui/kit';
import { go } from '../router';
import { LIVE, openShoot } from '../data/live';
import { call, libraryUrl } from '../data/backend';
import { PHOTO_BY_ID } from '../data/dummy';
import { previewUrl } from '../data/painter';
import { Agent } from '../agent/AgentPanel';
import { useLibrary } from '../store/library';
import {
  demoFace,
  faceKey,
  hideFaces,
  nameFaces,
  notPerson,
  readFaces,
  renamePerson,
  useFaceCrop,
  usePeople,
  usePerson,
  type Face,
  type FaceGroup,
  type PersonRow,
} from '../data/people';

export function PeopleScreen({ id }: { id?: string }) {
  return (
    <>
      <main className="panel main people-view">{id ? <PersonPage id={id} /> : <Overview />}</main>
      <Agent ctx="people" chips={['@ People']} placeholder="Ask about your photos…" />
    </>
  );
}

/** A face, cropped round: the engine's crop live, the painted Photo with dummy data. */
export function FaceCircle({ face, size, className = '' }: { face: Face; size: number; className?: string }) {
  const url = useFaceCrop(face);
  const style: CSSProperties = { width: size, height: size };
  if (LIVE) return <span className={`face-circle ${className}`} style={style}>{url && <img src={url} alt="" draggable={false} />}</span>;
  const d = demoFace(face);
  const photo = PHOTO_BY_ID[face.photo];
  if (d && photo) {
    // The painted Photo is 3:2; a square three face-radii wide around the face.
    const bw = size / (3 * d.r);
    style.backgroundImage = `url(${previewUrl(photo, 'thumb')})`;
    style.backgroundSize = `${bw}px ${(bw * 2) / 3}px`;
    style.backgroundPosition = `${size / 2 - d.x * bw}px ${size / 2 - (d.y * bw * 2) / 3}px`;
  }
  return <span className={`face-circle ${className}`} style={style} />;
}

type Sort = 'photos' | 'name';

function Overview() {
  const people = usePeople();
  const { dispatch } = useLibrary();
  const [sort, setSort] = useState<Sort>('photos');
  const [find, setFind] = useState('');
  const [singles, setSingles] = useState(false);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [reading, setReading] = useState(false);
  const toast = (text: string) => dispatch({ type: 'toast', text });

  const named = useMemo(() => {
    const list = (people?.people ?? []).filter((p) => p.name.toLowerCase().includes(find.trim().toLowerCase()));
    return sort === 'name' ? [...list].sort((a, b) => a.name.localeCompare(b.name)) : list;
  }, [people, sort, find]);
  const groups = (people?.groups ?? []).filter((g) => singles || g.faces.length > 1);
  const single = (people?.groups ?? []).filter((g) => g.faces.length === 1).length;
  const names = (people?.people ?? []).map((p) => p.name);

  /** Name a group (and any others picked with ⇧-click) as one person. */
  const name = async (g: FaceGroup, who: string) => {
    const picked = sel.has(g.id) ? groups.filter((x) => sel.has(x.id)) : [g];
    const faces = picked.flatMap((x) => x.faces);
    await nameFaces(faces, who);
    setSel(new Set());
    toast(`${faces.length === 1 ? '1 face' : `${faces.length} faces`} named ${who.trim()}`);
  };
  const hide = async (g: FaceGroup) => {
    const picked = sel.has(g.id) ? groups.filter((x) => sel.has(x.id)) : [g];
    await hideFaces(picked.flatMap((x) => x.faces));
    setSel(new Set());
    toast(picked.length === 1 ? 'Hidden: not someone to name' : `${picked.length} groups hidden`);
  };

  return (
    <>
      <header className="panel-head toolbar">
        <h1>People</h1>
        {people && (
          <span className="head-meta faint">
            {people.people.length} named · {people.groups.filter((g) => g.faces.length > 1).length} groups to name
          </span>
        )}
        <span className="grow" />
        <Seg quiet label="Sort people" value={sort} onChange={setSort} items={[{ value: 'photos', label: 'Most photos' }, { value: 'name', label: 'Name' }]} />
        <input className="people-find" placeholder="Find a person" value={find} onChange={(e) => setFind(e.target.value)} aria-label="Find a person" />
      </header>
      <div className="people-body">
        {people && people.unread > 0 && (
          <p className="people-unread faint">
            {reading ? `Reading faces in ${people.unread.toLocaleString()} Photos in the background` : `${people.unread.toLocaleString()} Photos haven't had their faces read yet.`}
            {!reading && (
              <Button
                size="sm"
                variant="ghost"
                onClick={async () => {
                  setReading(true);
                  await readFaces();
                }}
              >
                Read faces
              </Button>
            )}
          </p>
        )}
        {people && !people.people.length && !people.groups.length && (
          <p className="people-empty faint">No faces yet. Faces are found after Import, then grouped here by who they look like, on this Mac.</p>
        )}
        {named.length > 0 && (
          <section>
            <h2 className="people-h">
              Named <span className="faint">click to see their Photos</span>
            </h2>
            <div className="people-grid">
              {named.map((p) => (
                <PersonTile key={p.id} person={p} onDropGroup={(gid) => {
                  const g = groups.find((x) => x.id === gid);
                  if (g) void name(g, p.name);
                }} />
              ))}
            </div>
          </section>
        )}
        {groups.length > 0 && (
          <section>
            <h2 className="people-h">
              To name <span className="faint">faces that look like the same person, grouped on this Mac</span>
              <span className="grow" />
              {single > 0 && (
                <Button size="sm" variant="ghost" onClick={() => setSingles((s) => !s)}>
                  {singles ? 'Hide single faces' : `Show ${single} single ${single === 1 ? 'face' : 'faces'}`}
                </Button>
              )}
            </h2>
            <div className="people-grid">
              {groups.map((g) => (
                <GroupTile
                  key={g.id}
                  group={g}
                  names={names}
                  selected={sel.has(g.id)}
                  onSelect={(add) => setSel((s) => {
                    const n = new Set(add ? s : []);
                    if (n.has(g.id) && add) n.delete(g.id);
                    else n.add(g.id);
                    return n;
                  })}
                  onName={(who) => void name(g, who)}
                  onHide={() => void hide(g)}
                />
              ))}
            </div>
          </section>
        )}
      </div>
      <footer className="panel-foot people-foot">
        <span><Kbd quiet>↵</Kbd> name</span>
        <span><Kbd quiet>⇧</Kbd> click several, name them together</span>
        <span>drag onto a person to merge</span>
        <span><Kbd quiet>⌫</Kbd> not someone to name</span>
        <span className="grow" />
        {people && <span className="faint">{people.faces.toLocaleString()} faces · grouped on this Mac</span>}
      </footer>
    </>
  );
}

function PersonTile({ person, onDropGroup }: { person: PersonRow; onDropGroup: (group: string) => void }) {
  const [over, setOver] = useState(false);
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <button
          className={`people-tile ${over ? 'drop' : ''}`}
          onClick={() => go({ name: 'people', id: person.id })}
          onDragOver={(e) => {
            if (!e.dataTransfer.types.includes('text/x-face-group')) return;
            e.preventDefault();
            setOver(true);
          }}
          onDragLeave={() => setOver(false)}
          onDrop={(e) => {
            setOver(false);
            const g = e.dataTransfer.getData('text/x-face-group');
            if (g) onDropGroup(g);
          }}
          data-testid="person-tile"
        >
          <FaceCircle face={person.cover} size={76} />
          <span className="people-name">{person.name}</span>
          <span className="people-count">{person.photos === 1 ? '1 photo' : `${person.photos.toLocaleString()} photos`}</span>
        </button>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={() => go({ name: 'people', id: person.id })}>Open</ContextMenuItem>
        {LIVE && <ContextMenuItem onSelect={() => openShoot(`person:${person.id}`)}>Show their Photos in the grid</ContextMenuItem>}
      </ContextMenuContent>
    </ContextMenu>
  );
}

function GroupTile({ group, names, selected, onSelect, onName, onHide }: { group: FaceGroup; names: string[]; selected: boolean; onSelect: (add: boolean) => void; onName: (who: string) => void; onHide: () => void }) {
  return (
    <div className={`people-tile group ${selected ? 'selected' : ''}`} data-testid="face-group">
      <button
        className="people-face-btn"
        draggable
        onDragStart={(e) => e.dataTransfer.setData('text/x-face-group', group.id)}
        onClick={(e) => onSelect(e.shiftKey || e.metaKey)}
        onKeyDown={(e) => {
          if (e.key === 'Backspace' || e.key === 'Delete') {
            e.preventDefault();
            onHide();
          }
        }}
        aria-label={`${group.faces.length} faces, ${group.photos} photos`}
        title={`${group.faces.length} faces in ${group.photos} photos. ⌫ hides them`}
      >
        <FaceCircle face={group.cover} size={76} className="unnamed" />
        {group.faces.length > 1 && <span className="people-badge">{group.faces.length}</span>}
      </button>
      <NameField names={names} maybe={group.maybe?.name} onName={onName} />
    </div>
  );
}

/** The name box under a group: autocomplete from people already named, ↵ names. */
function NameField({ names, maybe, onName }: { names: string[]; maybe?: string; onName: (who: string) => void }) {
  const [text, setText] = useState('');
  const [focus, setFocus] = useState(false);
  const [at, setAt] = useState(0);
  const q = text.trim().toLowerCase();
  const hits = q ? names.filter((n) => n.toLowerCase().startsWith(q) || n.toLowerCase().includes(` ${q}`)).slice(0, 5) : [];
  const options = q ? [...hits, ...(hits.some((h) => h.toLowerCase() === q) ? [] : [`new:${text.trim()}`])] : [];
  const pick = (o: string) => {
    onName(o.startsWith('new:') ? o.slice(4) : o);
    setText('');
  };
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') setAt((i) => Math.min(i + 1, options.length - 1));
    else if (e.key === 'ArrowUp') setAt((i) => Math.max(i - 1, 0));
    else if (e.key === 'Enter') {
      e.preventDefault();
      if (options[at]) pick(options[at]);
      else if (!q && maybe) onName(maybe);
    } else if (e.key === 'Escape') {
      setText('');
      (e.target as HTMLInputElement).blur();
    } else return;
    e.stopPropagation();
  };
  return (
    <span className="people-field">
      <input
        value={text}
        placeholder={maybe ? `${maybe}?` : 'Name'}
        onChange={(e) => {
          setText(e.target.value);
          setAt(0);
        }}
        onFocus={() => setFocus(true)}
        onBlur={() => setTimeout(() => setFocus(false), 120)}
        onKeyDown={key}
        aria-label={maybe ? `Is this ${maybe}? Enter to confirm, or type a name` : 'Name this person'}
        data-testid="name-field"
      />
      {maybe && !text && (
        <button className="people-yes" title={`Yes, ${maybe}`} aria-label={`Yes, ${maybe}`} onClick={() => onName(maybe)}>
          <Check size={12} />
        </button>
      )}
      {focus && options.length > 0 && (
        <span className="people-menu" role="listbox">
          {options.map((o, i) => (
            <button key={o} role="option" aria-selected={i === at} className={i === at ? 'on' : ''} onMouseDown={(e) => e.preventDefault()} onClick={() => pick(o)}>
              {o.startsWith('new:') ? <span className="faint">New person “{o.slice(4)}”</span> : o}
            </button>
          ))}
        </span>
      )}
    </span>
  );
}

interface ShootPhotos {
  name: string;
  photos: { id: string; url: string }[];
}

function PersonPage({ id }: { id: string }) {
  const page = usePerson(id);
  const { dispatch } = useLibrary();
  const [at, setAt] = useState(0);
  const [renaming, setRenaming] = useState(false);
  const [shoots, setShoots] = useState<ShootPhotos[]>([]);
  const toast = (text: string) => dispatch({ type: 'toast', text });
  const ask = page?.suggestions ?? [];

  // Their Photos by Shoot, newest Shoot first.
  const photoKey = page?.photos.join(',');
  useEffect(() => {
    if (!page) return;
    let live = true;
    void (async () => {
      if (!LIVE) {
        const photos = page.photos.map((p) => PHOTO_BY_ID[p]).filter(Boolean);
        setShoots(photos.length ? [{ name: 'Tahoe, Oct 12', photos: photos.map((p) => ({ id: p.id, url: previewUrl(p, 'thumb') })) }] : []);
        return;
      }
      const [all, list] = await Promise.all([
        call<{ id: number; shoot_id: number; preview: string }[]>('photos', { person_id: Number(id) }),
        call<{ id: number; name: string }[]>('list_shoots'),
      ]);
      const by = new Map<number, ShootPhotos>();
      for (const p of all) {
        const s = by.get(p.shoot_id) ?? { name: list.find((x) => x.id === p.shoot_id)?.name ?? '', photos: [] };
        s.photos.push({ id: String(p.id), url: libraryUrl(p.preview) });
        by.set(p.shoot_id, s);
      }
      if (live) setShoots([...by.values()].sort((a, b) => b.name.localeCompare(a.name)));
    })();
    return () => {
      live = false;
    };
  }, [id, photoKey]);

  useEffect(() => setAt((i) => Math.min(i, Math.max(0, ask.length - 1))), [ask.length]);
  useEffect(() => {
    // Merged into someone else: their page is gone.
    if (page === null) go({ name: 'people' });
  }, [page]);

  const yes = async (faces: Face[]) => {
    if (!page || !faces.length) return;
    await nameFaces(faces, page.name);
    toast(faces.length === 1 ? `Named ${page.name}` : `${faces.length} faces named ${page.name}`);
  };
  const no = async (faces: Face[]) => {
    if (!page || !faces.length) return;
    await notPerson(faces, page.id);
  };
  useHotkeys('y', () => void yes(ask[at] ? [ask[at]] : []), [ask, at, page]);
  useHotkeys('n', () => void no(ask[at] ? [ask[at]] : []), [ask, at, page]);
  useHotkeys('left', () => setAt((i) => Math.max(0, i - 1)), [ask]);
  useHotkeys('right', () => setAt((i) => Math.min(ask.length - 1, i + 1)), [ask]);
  useHotkeys('shift+enter', () => void yes(ask), [ask, page]);
  useHotkeys('escape', () => go({ name: 'people' }), []);

  if (!page) return null;
  const count = shoots.reduce((n, s) => n + s.photos.length, 0) || page.photos.length;
  return (
    <>
      <header className="panel-head toolbar">
        <button className="people-crumb" onClick={() => go({ name: 'people' })}>
          People
        </button>
        <ChevronRight size={13} className="faint" />
        {page.faces[0] && <FaceCircle face={page.faces[0]} size={20} />}
        {renaming ? (
          <RenameField
            name={page.name}
            onDone={async (n) => {
              setRenaming(false);
              if (n && n !== page.name) {
                const into = await renamePerson(page.id, n);
                if (into !== page.id) go({ name: 'people', id: into });
              }
            }}
          />
        ) : (
          <h1 onDoubleClick={() => setRenaming(true)} title="Double-click to rename">
            {page.name}
          </h1>
        )}
        <span className="head-meta faint">
          {count === 1 ? '1 photo' : `${count.toLocaleString()} photos`}
          {shoots.length > 1 ? ` in ${shoots.length} Shoots` : ''}
        </span>
        <span className="grow" />
        <Button size="sm" variant="ghost" onClick={() => setRenaming(true)}>
          Rename
        </Button>
        {LIVE && (
          <Button size="sm" variant="ghost" onClick={() => openShoot(`person:${page.id}`)}>
            Open in grid
          </Button>
        )}
      </header>
      {ask.length > 0 && (
        <section className="people-ask" data-testid="people-ask">
          <h2 className="people-h">
            Is this {page.name}? <span className="faint">{ask.length === 1 ? '1 face that might be them' : `${ask.length} faces that might be them`}</span>
            <span className="grow" />
            <Button size="sm" variant="outline" onClick={() => void yes(ask)}>
              Confirm all <Kbd quiet>⇧↵</Kbd>
            </Button>
          </h2>
          <div className="people-ask-row">
            {ask.map((f, i) => (
              <span key={faceKey(f)} className={`people-maybe ${i === at ? 'on' : ''}`}>
                <button className="people-face-btn" onClick={() => setAt(i)} aria-label={`Face ${i + 1}`}>
                  <FaceCircle face={f} size={52} className="unnamed" />
                </button>
                <span className="people-yn">
                  <button className="yes" onClick={() => void yes([f])} title={`Yes, ${page.name} (Y)`} aria-label="Yes">
                    <Check size={12} />
                  </button>
                  <button onClick={() => void no([f])} title={`Not ${page.name} (N)`} aria-label="No">
                    <X size={12} />
                  </button>
                </span>
              </span>
            ))}
          </div>
        </section>
      )}
      <div className="people-body">
        {shoots.map((s) => (
          <section key={s.name}>
            <h2 className="people-h">
              {s.name} <span className="faint">{s.photos.length === 1 ? '1 photo' : `${s.photos.length} photos`}</span>
            </h2>
            <div className="people-photos">
              {s.photos.map((p) => (
                <button
                  key={p.id}
                  className="people-photo"
                  onClick={() => (LIVE ? openShoot(`person:${page.id}`) : go({ name: 'loupe', id: p.id }))}
                >
                  <img src={p.url} alt="" loading="lazy" draggable={false} />
                </button>
              ))}
            </div>
          </section>
        ))}
      </div>
      <footer className="panel-foot people-foot">
        <span><Kbd quiet>Y</Kbd> yes</span>
        <span><Kbd quiet>N</Kbd> not {page.name}</span>
        <span><Kbd quiet>←</Kbd><Kbd quiet>→</Kbd> move</span>
        <span><Kbd quiet>esc</Kbd> People</span>
        <span className="grow" />
        <span className="faint">Confirmed faces add People › {page.name} to the Photo</span>
      </footer>
    </>
  );
}

function RenameField({ name, onDone }: { name: string; onDone: (name: string | null) => void }) {
  const ref = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useEffect(() => ref.current?.select(), []);
  const finish = (n: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(n);
  };
  return (
    <input
      ref={ref}
      className="people-rename"
      defaultValue={name}
      aria-label="Name"
      onBlur={(e) => finish(e.target.value.trim() || null)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') finish((e.target as HTMLInputElement).value.trim() || null);
        if (e.key === 'Escape') finish(null);
      }}
    />
  );
}
