// The People view's data (engine `list_people`, `get_person`, `name_faces`, `hide_faces`,
// `not_person`, `rename_person`, `read_faces`, `face_crops`). Faces are grouped by who they
// look like on the Mac; only the person names anyone, and a name keeps `People|<name>` on
// each Photo. With dummy data the painted Shoot's people stand in: one person per look.

import { useEffect, useState } from 'react';
import { call, libraryUrl } from './backend';
import { LIVE } from './live';
import { PHOTOS, PHOTO_BY_ID } from './dummy';

/** One face: its Photo (as the UI keys Photos) and its number in that Photo, left to right. */
export interface Face {
  photo: string;
  face: number;
}
export const faceKey = (f: Face) => `${f.photo}-${f.face}`;

export interface PersonRow {
  id: string;
  name: string;
  photos: number;
  faces: number;
  cover: Face;
}

export interface FaceGroup {
  id: string;
  faces: Face[];
  photos: number;
  cover: Face;
  /** The named person it looks most like, when alike enough to ask. */
  maybe: { id: string; name: string } | null;
}

export interface People {
  people: PersonRow[];
  groups: FaceGroup[];
  faces: number;
  /** Photos whose faces aren't read yet. */
  unread: number;
}

export interface PersonPage {
  id: string;
  name: string;
  photos: string[];
  faces: Face[];
  suggestions: Face[];
}

type EngineFace = { photo_id: number; face: number };
const toFace = (f: EngineFace): Face => ({ photo: String(f.photo_id), face: f.face });
const toEngine = (f: Face): EngineFace => ({ photo_id: Number(f.photo), face: f.face });

// Every screen showing People re-reads after a change.
let version = 0;
const subs = new Set<() => void>();
function changed() {
  version++;
  subs.forEach((f) => f());
}
function useVersion() {
  const [v, setV] = useState(version);
  useEffect(() => {
    const on = () => setV(version);
    subs.add(on);
    return () => void subs.delete(on);
  }, []);
  return v;
}

/** Everyone named and the groups of faces to name; null while loading. */
export function usePeople(): People | null {
  const v = useVersion();
  const [people, setPeople] = useState<People | null>(null);
  useEffect(() => {
    let live = true;
    void loadPeople().then((p) => live && setPeople(p));
    return () => {
      live = false;
    };
  }, [v]);
  return people;
}

/** One person's page; null while loading or once they're gone (merged away). */
export function usePerson(id: string): PersonPage | null | undefined {
  const v = useVersion();
  const [page, setPage] = useState<PersonPage | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    void loadPerson(id).then(
      (p) => live && setPage(p),
      () => live && setPage(null),
    );
    return () => {
      live = false;
    };
  }, [id, v]);
  return page;
}

/** How many people are named, once the People view has loaded (for the sidebar's count). */
let named: number | null = null;
export function useNamedCount(): number | null {
  const [n, setN] = useState(named);
  useEffect(() => {
    const on = () => setN(named);
    subs.add(on);
    return () => void subs.delete(on);
  }, []);
  return n;
}

async function loadPeople(): Promise<People> {
  const p = await fetchPeople();
  if (p.people.length !== named) {
    named = p.people.length;
    subs.forEach((f) => f());
  }
  return p;
}

async function fetchPeople(): Promise<People> {
  if (!LIVE) return demoPeople();
  const r = await call<{
    people: { id: number; name: string; photos: number; faces: number; cover: EngineFace }[];
    groups: { id: string; faces: EngineFace[]; photos: number; cover: EngineFace; maybe: { person_id: number; name: string } | null }[];
    faces: number;
    unread_photos: number;
  }>('list_people');
  return {
    people: r.people.map((p) => ({ ...p, id: String(p.id), cover: toFace(p.cover) })),
    groups: r.groups.map((g) => ({ ...g, faces: g.faces.map(toFace), cover: toFace(g.cover), maybe: g.maybe && { id: String(g.maybe.person_id), name: g.maybe.name } })),
    faces: r.faces,
    unread: r.unread_photos,
  };
}

async function loadPerson(id: string): Promise<PersonPage> {
  if (!LIVE) return demoPerson(id);
  const r = await call<{ person: { id: number; name: string }; photos: number[]; faces: EngineFace[]; suggestions: EngineFace[] }>('get_person', { person_id: Number(id) });
  return { id, name: r.person.name, photos: r.photos.map(String), faces: r.faces.map(toFace), suggestions: r.suggestions.map(toFace) };
}

/** Name faces as someone (new or existing); returns the person's id. */
export async function nameFaces(faces: Face[], name: string): Promise<string> {
  let id: string;
  if (LIVE) id = String((await call<{ id: number }>('name_faces', { faces: faces.map(toEngine), name })).id);
  else id = demo.name(faces, name);
  changed();
  return id;
}

/** Hide faces as not someone to name, or bring them back. */
export async function hideFaces(faces: Face[], hide = true) {
  if (LIVE) await call('hide_faces', { faces: faces.map(toEngine), unhide: !hide });
  else for (const f of faces) hide ? demo.hidden.add(faceKey(f)) : demo.hidden.delete(faceKey(f));
  changed();
}

/** These faces aren't this person. */
export async function notPerson(faces: Face[], person: string) {
  if (LIVE) await call('not_person', { faces: faces.map(toEngine), person_id: Number(person) });
  else
    for (const f of faces) {
      demo.not.add(`${faceKey(f)}|${person}`);
      if (demo.named.get(faceKey(f)) === person) demo.named.delete(faceKey(f));
    }
  changed();
}

/** Rename a person; a name someone else has merges them. Returns who they are now. */
export async function renamePerson(id: string, name: string): Promise<string> {
  let into = id;
  if (LIVE) into = String((await call<{ id: number }>('rename_person', { person_id: Number(id), name })).id);
  else into = demo.rename(id, name);
  changed();
  return into;
}

/** Queue the face models for Photos not read yet. */
export async function readFaces(): Promise<number> {
  if (!LIVE) return 0;
  const r = await call<{ photos: number }>('read_faces');
  return r.photos;
}

// Face crops: the engine writes small square JPEGs; ask in batches as tiles appear.
const crops = new Map<string, string | null>();
const wanted = new Map<string, Face>();
let timer: ReturnType<typeof setTimeout> | null = null;
const cropSubs = new Set<() => void>();

function ask() {
  timer = null;
  const batch = [...wanted.values()].slice(0, 120);
  for (const f of batch) wanted.delete(faceKey(f));
  if (!batch.length) return;
  void call<Record<string, string | null>>('face_crops', { faces: batch.map(toEngine), edge: 160 })
    .then((r) => {
      for (const f of batch) crops.set(faceKey(f), r[`${f.photo}-${f.face}`] ? libraryUrl(r[`${f.photo}-${f.face}`]!) : null);
    })
    .catch(() => batch.forEach((f) => crops.set(faceKey(f), null)))
    .finally(() => {
      cropSubs.forEach((s) => s());
      if (wanted.size) timer = setTimeout(ask, 0);
    });
}

/** A live face crop's URL: undefined while it's made, null if there is none. */
export function useFaceCrop(face: Face): string | null | undefined {
  const key = faceKey(face);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!LIVE) return;
    const on = () => tick((n) => n + 1);
    cropSubs.add(on);
    if (!crops.has(key)) {
      wanted.set(key, face);
      timer ??= setTimeout(ask, 16);
    }
    return () => void cropSubs.delete(on);
  }, [key]);
  return LIVE ? crops.get(key) : undefined;
}

// ---- Dummy data: each painted person's look is one person. ----

const look = (photo: string, face: number) => {
  const p = PHOTO_BY_ID[photo]?.scene.people[face];
  return p ? `${p.skin}${p.hair}${p.shirt}` : '';
};

const demo = {
  /** Face key → person id. */
  named: new Map<string, string>(),
  people: new Map<string, string>(),
  hidden: new Set<string>(),
  not: new Set<string>(),
  seeded: false,
  name(faces: Face[], name: string): string {
    const clean = name.trim().replace(/[|>/]/g, ' ');
    let id = [...this.people].find(([, n]) => n.toLowerCase() === clean.toLowerCase())?.[0];
    if (!id) {
      id = String(this.people.size + 1);
      this.people.set(id, clean);
    }
    for (const f of faces) {
      this.named.set(faceKey(f), id);
      this.hidden.delete(faceKey(f));
    }
    return id;
  },
  rename(id: string, name: string): string {
    const other = [...this.people].find(([k, n]) => k !== id && n.toLowerCase() === name.trim().toLowerCase())?.[0];
    if (!other) {
      this.people.set(id, name.trim());
      return id;
    }
    for (const [k, v] of this.named) if (v === id) this.named.set(k, other);
    this.people.delete(id);
    return other;
  },
};

function allFaces(): Face[] {
  return PHOTOS.flatMap((p) => p.faces.map((_, i) => ({ photo: p.id, face: i })));
}

function seed() {
  if (demo.seeded) return;
  demo.seeded = true;
  // Name the two people seen most, so the demo shows both halves of the view.
  const by = new Map<string, Face[]>();
  for (const f of allFaces()) by.set(look(f.photo, f.face), [...(by.get(look(f.photo, f.face)) ?? []), f]);
  const top = [...by.values()].sort((a, b) => b.length - a.length);
  // Leave a few of Maya's faces unnamed, so her page asks "Is this Maya?".
  if (top[0]) demo.name(top[0].slice(0, Math.max(1, top[0].length - 4)), 'Maya');
  if (top[1]) demo.name(top[1], 'Tomas');
}

const biggest = (faces: Face[]) => faces.reduce((a, f) => ((PHOTO_BY_ID[f.photo]?.faces[f.face]?.r ?? 0) > (PHOTO_BY_ID[a.photo]?.faces[a.face]?.r ?? 0) ? f : a));

function demoPeople(): People {
  seed();
  const faces = allFaces();
  const byPerson = new Map<string, Face[]>();
  const groups = new Map<string, Face[]>();
  for (const f of faces) {
    const k = faceKey(f);
    const who = demo.named.get(k);
    if (who) byPerson.set(who, [...(byPerson.get(who) ?? []), f]);
    else if (!demo.hidden.has(k)) groups.set(look(f.photo, f.face), [...(groups.get(look(f.photo, f.face)) ?? []), f]);
  }
  const people = [...byPerson].map(([id, fs]) => ({ id, name: demo.people.get(id) ?? '', photos: new Set(fs.map((f) => f.photo)).size, faces: fs.length, cover: biggest(fs) }));
  people.sort((a, b) => b.photos - a.photos);
  return {
    people,
    groups: [...groups.values()]
      .map((fs) => {
        const cover = biggest(fs);
        // A group sharing a named person's look is asked about, as the engine does by likeness.
        const who = [...demo.named].find(([k]) => { const [ph, i] = k.split(/-(?=\d+$)/); return look(ph, Number(i)) === look(cover.photo, cover.face); })?.[1];
        return { id: faceKey(cover), faces: fs, photos: new Set(fs.map((f) => f.photo)).size, cover, maybe: who ? { id: who, name: demo.people.get(who) ?? '' } : null };
      })
      .sort((a, b) => b.faces.length - a.faces.length),
    faces: faces.length,
    unread: 0,
  };
}

function demoPerson(id: string): PersonPage {
  seed();
  const name = demo.people.get(id);
  if (name == null) throw new Error('gone');
  const faces = allFaces().filter((f) => demo.named.get(faceKey(f)) === id);
  const looks = new Set(faces.map((f) => look(f.photo, f.face)));
  // Faces hidden or not yet named that share a named face's look: "Is this Maya?"
  const suggestions = allFaces().filter((f) => !demo.named.has(faceKey(f)) && looks.has(look(f.photo, f.face)) && !demo.not.has(`${faceKey(f)}|${id}`));
  return { id, name, photos: [...new Set(faces.map((f) => f.photo))], faces, suggestions };
}

/** For a dummy face: the painted Photo and where the face sits, to crop it with CSS. */
export function demoFace(face: Face): { url: string; x: number; y: number; r: number } | null {
  const p = PHOTO_BY_ID[face.photo];
  const box = p?.faces[face.face];
  return p && box ? { url: '', x: box.x, y: box.y, r: box.r } : null;
}
