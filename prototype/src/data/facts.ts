// What the engine's specialist models found in a Photo (its Facts, ADR 0013), through the
// `analyze_photo` tool. The one place screens read Facts: a new Analyzer shows up here
// without new plumbing. Cached per Photo for the session.

import { useEffect, useState } from 'react';
import { call } from './backend';
import { LIVE, liveShootId } from './live';
import { PHOTOS } from './dummy';

export interface FaceFacts {
  /** Index, left to right. */
  face: number;
  /** Box as fractions of the image: left, top, width, height. */
  x: number;
  y: number;
  w: number;
  h: number;
  score: number;
  /** Only for faces near enough to read. */
  eyes_closed?: boolean;
  /** Blink score per eye as seen in the image, [left, right], 0 open to 1 shut. */
  blink?: [number, number];
  /** Eye sharpness over the frame's sharpest part: ~1 sharp, under 0.25 soft. */
  focus?: number;
}

export interface PhotoFacts {
  photo_id: number;
  faces: FaceFacts[];
  coverage: { people: number; skin: number; hair: number; sky: number };
  signals: { sharpness: number; brightness: number; clipped: number; crushed: number } | null;
  /** The few words the cull's Contact sheet gets ("eyes closed on 1 of 2 faces, faces sharp"). */
  summary: string;
}

/** Eye focus under which a face reads as soft (the engine's `eyes::SOFT`). */
export const SOFT_FOCUS = 0.25;

const cache = new Map<string, Promise<PhotoFacts>>();

/** A Photo's Facts, computed by the engine on first ask (a second or two) and then cached. */
export function photoFacts(photoId: string): Promise<PhotoFacts> {
  let p = cache.get(photoId);
  if (!p) {
    p = call<PhotoFacts>('analyze_photo', { photo_id: Number(photoId) });
    p.catch(() => cache.delete(photoId));
    cache.set(photoId, p);
  }
  return p;
}

/** A Photo's Facts for a screen: null while loading, when `enabled` is false, or on failure. */
export function usePhotoFacts(photoId: string | null, enabled = true): PhotoFacts | null {
  const [facts, setFacts] = useState<PhotoFacts | null>(null);
  useEffect(() => {
    if (!photoId || !enabled) return;
    let on = true;
    setFacts(null);
    photoFacts(photoId)
      .then((f) => on && setFacts(f))
      .catch((e) => console.error('analyze_photo failed', e));
    return () => {
      on = false;
    };
  }, [photoId, enabled]);
  return facts;
}

/** The grid's eye badges for one Photo: a read face has its eyes shut, or every read face is soft. */
export interface EyeFlags {
  closed: boolean;
  soft: boolean;
}

/** Eye flags from a Photo's full Facts (Loupe). */
export function eyeFlagsOf(f: PhotoFacts | null): EyeFlags | null {
  const read = f?.faces.filter((x) => x.eyes_closed != null) ?? [];
  if (!read.length) return null;
  const flags = { closed: read.some((x) => x.eyes_closed), soft: read.every((x) => (x.focus ?? 1) < SOFT_FOCUS) };
  return flags.closed || flags.soft ? flags : null;
}

// The prototype's dummy Photos: their written reason stands in for the eyes model.
function demoFlags(): Map<string, EyeFlags> {
  const m = new Map<string, EyeFlags>();
  for (const p of PHOTOS) {
    const r = (p.reason ?? '').toLowerCase();
    const flags = { closed: /eyes? closed|mid-blink|blink/.test(r), soft: /soft|missed focus|motion/.test(r) };
    if (flags.closed || flags.soft) m.set(p.id, flags);
  }
  return m;
}

/**
 * Eye flags for every Photo of the open Shoot, from the Facts the background `analyze`
 * job already stored (`eye_flags` never computes). Refreshed while that job works through
 * the Shoot. Informational only: closed eyes barely predict Greg's picks (he keeps laughing
 * shots), so nothing is rejected on them.
 */
export function useEyeFlags(): (id: string) => EyeFlags | null {
  const [flags, setFlags] = useState<Map<string, EyeFlags>>(() => (LIVE ? new Map() : demoFlags()));
  useEffect(() => {
    if (!LIVE) return;
    let on = true;
    const load = () =>
      call<{ id: number; closed: boolean; soft: boolean }[]>('eye_flags', { shoot_id: liveShootId() || undefined })
        .then((rows) => on && setFlags(new Map(rows.map((r) => [String(r.id), { closed: r.closed, soft: r.soft }]))))
        .catch(() => {});
    void load();
    const t = setInterval(load, 15000);
    return () => {
      on = false;
      clearInterval(t);
    };
  }, []);
  return (id) => flags.get(id) ?? null;
}
