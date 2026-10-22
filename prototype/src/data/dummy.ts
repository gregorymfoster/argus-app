// Deterministic dummy Shoot: 112 Photos with Scenes, capture faults, signals,
// AI Verdicts with reasons, and Stacks. Same seed, same Shoot, every load.

import type { Capture, FaceBox, Person, Photo, Scene, SceneKind, Shoot, Signals, Stack, Verdict } from '../domain/types';

export function rng(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return ((s >>> 0) % 100000) / 100000;
  };
}

const SKINS = ['#F1C7A5', '#E0A982', '#C68863', '#9C6644', '#6F4A33', '#F5D3B8'];
const SHIRTS = ['#3D5A80', '#B5443C', '#E9C46A', '#2A9D8F', '#F4F1DE', '#6D597A', '#264653', '#E76F51', '#8AB17D'];
const HAIRS = ['#2B1D14', '#4A3222', '#7A5230', '#C9A26B', '#1C1C1C', '#8C8C8C'];
const KINDS: SceneKind[] = ['lake', 'forest', 'cabin', 'dusk', 'street'];

// SHOOT, OTHER_SHOOTS, PHOTOS and STACKS are filled in place by data/live.ts in live mode.
export const SHOOT: Shoot = { id: 'tahoe', name: 'Tahoe, Oct 12', count: 112, live: true };
export const OTHER_SHOOTS: Shoot[] = [
  { id: 'dinner', name: 'Birthday dinner', count: 86, live: false },
  { id: 'lisbon', name: 'Lisbon', count: 240, live: false },
];

/** Geometry shared by the painter and face boxes. */
export function personGeometry(p: Person, groundY: number) {
  const headR = 0.045 * p.scale;
  const bodyH = 0.3 * p.scale;
  const headY = groundY - bodyH - headR * 1.4;
  return { headR, bodyH, headY };
}

export const GROUND_Y: Record<SceneKind, number> = { lake: 0.98, forest: 0.98, cabin: 0.98, dusk: 0.98, street: 0.98 };
const HORIZON: Record<SceneKind, number> = { lake: 0.42, forest: 0.3, cabin: 0.0, dusk: 0.5, street: 0.36 };

function makePeople(r: () => number, n: number): Person[] {
  const people: Person[] = [];
  const span = n === 1 ? 0 : Math.min(0.62, 0.2 * n);
  for (let i = 0; i < n; i++) {
    const x = n === 1 ? 0.35 + r() * 0.3 : 0.5 - span / 2 + (span * i) / (n - 1) + (r() - 0.5) * 0.04;
    people.push({
      x,
      scale: (n > 3 ? 0.8 : 1.05) * (0.9 + r() * 0.25),
      skin: SKINS[Math.floor(r() * SKINS.length)],
      shirt: SHIRTS[Math.floor(r() * SHIRTS.length)],
      hair: HAIRS[Math.floor(r() * HAIRS.length)],
      eyesClosed: false,
      smile: r() > 0.3,
    });
  }
  return people;
}

interface Moment {
  scene: Scene;
  frames: number;
}

function moments(): Moment[] {
  const r = rng(20261012);
  const list: Moment[] = [];
  let total = 0;
  let bursts = 0;
  let i = 0;
  while (total < 112) {
    const wantBurst = bursts < 9 && i % 4 === 1;
    const frames = Math.min(wantBurst ? 3 + Math.floor(r() * 4) : 1, 112 - total);
    if (frames > 1) bursts++;
    const kind = KINDS[Math.floor(r() * KINDS.length)];
    const n = r() < 0.15 ? 0 : r() < 0.55 ? 1 + Math.floor(r() * 2) : 3 + Math.floor(r() * 3);
    list.push({ scene: { kind, seed: Math.floor(r() * 1e9), people: makePeople(r, n) }, frames });
    total += frames;
    i++;
  }
  return list;
}

function faceBoxes(scene: Scene): FaceBox[] {
  const g = GROUND_Y[scene.kind];
  return scene.people.map((p) => {
    const { headR, headY } = personGeometry(p, g);
    return { x: p.x, y: headY, r: headR * 1.35, eyesClosed: p.eyesClosed };
  });
}

function judge(sig: Signals, cap: Capture, people: number, dupe: boolean, burstSize: number, rank: number): [Verdict, string] {
  if (dupe) return ['reject', rank === 1 ? 'Near duplicate of a Pick' : 'Weaker frame in its Stack'];
  if (sig.focus < 0.55) return ['reject', cap.motion > 0 ? 'Motion blur' : 'Missed focus'];
  if (people && sig.eyesOpen[0] < sig.eyesOpen[1]) {
    return sig.eyesOpen[1] > 2 ? ['unflagged', `Eyes closed on ${sig.eyesOpen[1] - sig.eyesOpen[0]} of ${sig.eyesOpen[1]}`] : ['reject', 'Eyes closed'];
  }
  if (Math.abs(sig.exposure) > 1.4) return ['reject', sig.exposure < 0 ? 'Badly underexposed' : 'Blown highlights'];
  if (sig.focus < 0.72) return ['unflagged', 'Slightly soft'];
  if (cap.faceShadow > 0.55) return ['unflagged', 'Faces underexposed'];
  if (sig.expression === 'Awkward') return ['unflagged', 'Awkward expression'];
  if (burstSize > 1) return ['pick', `Best of ${burstSize} in the burst`];
  if (sig.expression === 'Great') return ['pick', 'Great expression'];
  if (!people) return ['pick', 'Strong composition'];
  return ['pick', 'Sharp, good light'];
}

function build(): { photos: Photo[]; stacks: Stack[] } {
  const r = rng(77);
  const photos: Photo[] = [];
  const stacks: Stack[] = [];
  let index = 0;
  moments().forEach((m, mi) => {
    const stackId = m.frames > 1 ? `s${mi}` : undefined;
    const ids: string[] = [];
    const bestFrame = Math.floor(r() * m.frames);
    for (let f = 0; f < m.frames; f++) {
      const scene: Scene = { ...m.scene, people: m.scene.people.map((p) => ({ ...p })) };
      const isBest = f === bestFrame;
      // Faults: the best frame of a burst is clean, the others pick up faults.
      const fault = r();
      const cap: Capture = {
        blur: 0,
        motion: 0,
        ev: (r() - 0.5) * 0.6,
        wb: scene.kind === 'cabin' ? 0.6 + r() * 0.3 : (r() - 0.5) * 0.4,
        faceShadow: scene.kind === 'dusk' || scene.kind === 'forest' ? 0.3 + r() * 0.5 : r() * 0.4,
        shift: m.frames > 1 ? (f - bestFrame) * 0.006 : 0,
      };
      if (!isBest || m.frames === 1) {
        if (fault < 0.2) cap.blur = 3 + r() * 6;
        else if (fault < 0.3) cap.motion = 6 + r() * 10;
        else if (fault < 0.42 && scene.people.length) {
          const k = Math.floor(r() * scene.people.length);
          scene.people[k].eyesClosed = true;
        } else if (fault < 0.5) cap.ev = (r() < 0.5 ? -1 : 1) * (1.5 + r() * 0.8);
        else if (fault < 0.6) cap.blur = 1.2 + r() * 1.2;
      }
      const people = scene.people.length;
      const closed = scene.people.filter((p) => p.eyesClosed).length;
      const focus = Math.max(0.2, Math.min(0.98, 0.95 - cap.blur * 0.08 - cap.motion * 0.04 - r() * 0.06));
      const exprR = r();
      const signals: Signals = {
        focus: Math.round(focus * 100) / 100,
        eyesOpen: [people - closed, people],
        exposure: Math.round((cap.ev - cap.faceShadow * (people ? 0.4 : 0)) * 10) / 10,
        expression: !people ? 'Neutral' : exprR > 0.75 ? 'Great' : exprR > 0.25 ? 'Good' : exprR > 0.12 ? 'Neutral' : 'Awkward',
      };
      const dupe = m.frames > 1 && !isBest && cap.blur === 0 && cap.motion === 0 && closed === 0 && Math.abs(cap.ev) < 1.4;
      const [aiVerdict, reason] = judge(signals, cap, people, dupe, isBest ? m.frames : 1, Math.abs(f - bestFrame));
      const id = `p${index}`;
      ids.push(id);
      const shutter = ['1/250', '1/500', '1/125', '1/60', '1/1000'][Math.floor(r() * 5)];
      const ap = ['f/1.8', 'f/2.8', 'f/4', 'f/5.6'][Math.floor(r() * 4)];
      const iso = [100, 200, 400, 800, 1600][Math.floor(r() * 5)];
      photos.push({
        id,
        file: `DSC0${8001 + index}.ARW`,
        index,
        scene,
        capture: cap,
        faces: faceBoxes(scene),
        horizon: HORIZON[scene.kind],
        signals,
        aiVerdict,
        reason,
        stackId,
        stackRank: stackId ? (isBest ? 0 : 1 + Math.abs(f - bestFrame)) : undefined,
        exif: `${shutter} · ${ap} · ISO ${iso} · 35mm`,
        group: people >= 3,
      });
      index++;
    }
    if (stackId) stacks.push({ id: stackId, photoIds: ids });
  });
  return { photos, stacks };
}

/** Stars and Best Photos as the cull gives them: stars agree with the Verdict, and
 *  about one in ten frames, one per Stack, make the Best Photos. */
function rate(photos: Photo[]) {
  const quality = (p: Photo) =>
    p.signals.focus + { Great: 0.25, Good: 0.1, Neutral: 0, Awkward: -0.2 }[p.signals.expression] - Math.abs(p.signals.exposure) * 0.1 + (p.group ? 0.05 : 0);
  for (const p of photos) {
    const q = quality(p);
    p.aiStars = p.aiVerdict === 'reject' ? (q > 0.7 ? 2 : 1) : p.aiVerdict === 'unflagged' ? (q > 0.85 ? 3 : 2) : q > 1.12 ? 5 : q > 0.98 ? 4 : 3;
  }
  const used = new Set<string>();
  const best = photos
    .filter((p) => p.aiVerdict === 'pick')
    .sort((a, b) => (b.aiStars ?? 0) - (a.aiStars ?? 0) || quality(b) - quality(a))
    .filter((p) => !p.stackId || (!used.has(p.stackId) && used.add(p.stackId)))
    .slice(0, Math.max(1, Math.round(photos.length / 10)));
  best.forEach((p, i) => (p.best = i + 1));
}

const built = build();
rate(built.photos);
export const PHOTOS: Photo[] = built.photos;
export const STACKS: Stack[] = built.stacks;
export const PHOTO_BY_ID: Record<string, Photo> = Object.fromEntries(PHOTOS.map((p) => [p.id, p]));
export const STACK_BY_ID: Record<string, Stack> = Object.fromEntries(STACKS.map((s) => [s.id, s]));

export function stackOf(p: Photo): Photo[] {
  if (!p.stackId) return [p];
  return STACK_BY_ID[p.stackId].photoIds.map((id) => PHOTO_BY_ID[id]).sort((a, b) => (a.stackRank ?? 0) - (b.stackRank ?? 0));
}
