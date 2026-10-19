// A scripted stand-in for the Agent. It reads a prompt with a few keyword
// rules and answers with timed steps, prose, and Library actions, which is
// the same shape the real tool-use loop will produce.

import { PHOTOS, PHOTO_BY_ID } from '../data/dummy';
import { LOOK_BY_ID } from '../domain/looks';
import type { Adjust, Correction, NumKey, LocalAdjust, Photo, Verdict } from '../domain/types';
import { recipeOf, verdictOf, type Action, type State } from '../store/library';

export interface AgentStep {
  label: string;
  wait: number;
  shown: string;
}

export interface AgentPlan {
  steps: AgentStep[];
  prose: string;
  actions: Action[];
  batchLabel?: string;
  changed?: string[];
  /** Human-readable list of what the batch changes, for propose mode. */
  summary?: string[];
}

const step = (label: string, wait: number, shown?: string): AgentStep => ({ label, wait, shown: shown ?? `${(wait / 260).toFixed(1)}s` });

const words = (n: number) => ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven', 'twelve'][n] ?? String(n);
const cap1 = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

export function initialCull(state: State): AgentPlan {
  const verdicts: Record<string, Verdict> = {};
  const stars: Record<string, number> = {};
  PHOTOS.slice(0, state.imported).forEach((p) => {
    verdicts[p.id] = p.aiVerdict;
    if (p.aiStars) stars[p.id] = p.aiStars;
  });
  const count = (v: Verdict) => Object.values(verdicts).filter((x) => x === v).length;
  const stacks = new Set(PHOTOS.filter((p) => p.stackId).map((p) => p.stackId)).size;
  return {
    steps: [step('Scored 112 photos', 1100, '3.8s'), step(`Found ${stacks} bursts`, 500, '0.4s'), step('Wrote Verdicts and stars', 300, '0.1s')],
    prose: `I went through all ${PHOTOS.length}. ${cap1(words(count('pick')))} are keepers, ${words(count('unflagged'))} are close calls I left unflagged, and the rest are soft, mid-blink or duplicates. ${cap1(words(stacks))} bursts are collapsed to their best frame.`,
    actions: [{ type: 'cull/write', verdicts, stars }],
  };
}

function cull(prompt: string, s: State, selected: Photo | null): AgentPlan {
  const q = prompt.toLowerCase();
  const all = PHOTOS.slice(0, s.imported);
  const now = (p: Photo) => verdictOf(s, p.id);
  if (/(why|explain)/.test(q) && selected) {
    const sig = selected.signals;
    return {
      steps: [step(`Looked at ${selected.file}`, 700)],
      prose: `${selected.reason}. Focus scores ${sig.focus.toFixed(2)}, ${sig.eyesOpen[1] ? `${sig.eyesOpen[0]} of ${sig.eyesOpen[1]} have their eyes open` : 'there are no faces'}, and exposure is ${sig.exposure >= 0 ? '+' : ''}${sig.exposure} EV. ${selected.stackId ? 'It sits in a burst, so I compared it with its neighbours.' : ''}`,
      actions: [],
    };
  }
  if (/(strict|blur|soft|sharp)/.test(q)) {
    const group = /group/.test(q);
    const hits = all.filter((p) => now(p) === 'pick' && (!group || p.group) && p.signals.focus < (group ? 0.92 : 0.86)).slice(0, 8);
    if (!hits.length) return { steps: [step('Checked focus on Picks', 600)], prose: 'Every Pick already clears that bar. Nothing to change.', actions: [] };
    return {
      steps: [step(`Re-scored focus on ${group ? 'group shots' : 'Picks'}`, 900), step('Updated Verdicts', 250, '0.1s')],
      prose: `Done. ${cap1(words(hits.length))} ${group ? 'group shots' : 'Picks'} are unflagged now; they're sharp enough to keep but not your best.`,
      actions: hits.map((p) => ({ type: 'verdict/set', id: p.id, verdict: 'unflagged', by: 'ai' }) as Action),
      batchLabel: `Agent: ${hits.length} Picks unflagged`,
      changed: hits.map((p) => p.id),
    };
  }
  if (/(eye|blink)/.test(q)) {
    const hits = all.filter((p) => p.signals.eyesOpen[0] < p.signals.eyesOpen[1] && now(p) !== 'reject');
    return {
      steps: [step('Checked eyes on every face', 1000), step('Updated Verdicts', 250, '0.1s')],
      prose: hits.length ? `Rejected ${words(hits.length)} more photos where someone is mid-blink.` : 'Nobody is mid-blink in what is left.',
      actions: hits.map((p) => ({ type: 'verdict/set', id: p.id, verdict: 'reject', by: 'ai' }) as Action),
      batchLabel: `Agent: ${hits.length} blinks rejected`,
      changed: hits.map((p) => p.id),
    };
  }
  if (/(lenient|loose|keep more|generous)/.test(q)) {
    const hits = all.filter((p) => now(p) === 'unflagged' && /soft|expression/i.test(p.reason)).slice(0, 8);
    return {
      steps: [step('Re-read the close calls', 800), step('Updated Verdicts', 250, '0.1s')],
      prose: `Picked ${words(hits.length)} of the unflagged close calls. The soft ones still read well at normal size.`,
      actions: hits.map((p) => ({ type: 'verdict/set', id: p.id, verdict: 'pick', by: 'ai' }) as Action),
      batchLabel: `Agent: ${hits.length} unflagged to Pick`,
      changed: hits.map((p) => p.id),
    };
  }
  const count = (v: Verdict) => all.filter((p) => now(p) === v).length;
  return {
    steps: [step('Read the Shoot', 500)],
    prose: `You have ${count('pick')} Picks, ${count('unflagged')} unflagged and ${count('reject')} Rejects. I can be stricter on blur, reject blinks, be more generous, or explain any Verdict you select.`,
    actions: [],
  };
}

export function correctionsFor(ids: string[], facesOnly = false): Record<string, Correction> {
  const out: Record<string, Correction> = {};
  for (const id of ids) {
    const p = PHOTO_BY_ID[id];
    const face = p.faces.length && p.capture.faceShadow > 0.3 ? Math.round(p.capture.faceShadow * 9) / 10 : 0;
    out[id] = {
      exposure: facesOnly ? 0 : Math.abs(p.capture.ev) > 0.15 ? Math.round(-p.capture.ev * 8) / 10 : 0,
      wbK: facesOnly ? 0 : p.scene.kind === 'cabin' ? -Math.round((250 + p.capture.wb * 100) / 50) * 50 : 0,
      face,
    };
  }
  return out;
}

function look(prompt: string, s: State, picks: string[]): AgentPlan {
  const q = prompt.toLowerCase();
  const tune: [RegExp, NumKey, number, string][] = [
    [/warmer/, 'temp', 8, 'warmer'],
    [/cooler/, 'temp', -8, 'cooler'],
    [/more contrast|punchier/, 'contrast', 10, 'punchier'],
    [/less contrast|softer/, 'contrast', -10, 'softer'],
    [/less grain|no grain/, 'grain', -100, 'without grain'],
    [/more grain/, 'grain', 15, 'grainier'],
  ];
  if (s.look) {
    const hits = tune.filter(([re]) => re.test(q));
    if (hits.length) {
      const actions: Action[] = hits.map(([, key, d]) => ({ type: 'look/adjust', key, value: Math.max(key === 'grain' ? 0 : -100, Math.min(100, (s.look!.adjust[key] ?? 0) + d)) }));
      return {
        steps: [step(`Tuned ${s.look.name}`, 500)],
        prose: `Made the Look ${hits.map((h) => h[3]).join(' and ')}. It updates all ${picks.length} Picks at once.`,
        actions,
        batchLabel: `Agent: tuned ${s.look.name}`,
        changed: picks,
      };
    }
  }
  const id = /cool|moody|blue/.test(q) ? 'cool-moody' : /bright|airy|light/.test(q) ? 'bright-airy' : /natural|clean|true/.test(q) ? 'clean-natural' : /warm|film|soft|analog/.test(q) ? 'warm-film' : null;
  const facesOnly = !id && /face|people|skin/.test(q);
  if (!id && !facesOnly) {
    return { steps: [step('Read your Picks', 400)], prose: 'Tell me a mood ("warm and soft", "cool and moody", "bright and airy") and I will build a Look and set Corrections per photo.', actions: [] };
  }
  const corrections = correctionsFor(picks, facesOnly);
  const lifted = picks.filter((i) => corrections[i].face > 0).length;
  const wb = picks.filter((i) => corrections[i].wbK !== 0).length;
  const exp = picks.filter((i) => corrections[i].exposure !== 0).length;
  const lk = id ? LOOK_BY_ID[id] : s.look ?? LOOK_BY_ID['clean-natural'];
  const summary = [
    id ? `${lk.name} Look on ${picks.length} Picks` : null,
    lifted ? `Face light lifted on ${lifted}` : null,
    wb ? `White balance cooled on ${wb} indoor shots` : null,
    exp ? `Exposure evened on ${exp}` : null,
  ].filter(Boolean) as string[];
  return {
    steps: [
      step(`Looked at ${picks.length} Picks`, 900, '2.1s'),
      ...(id ? [step(`Built the ${lk.name} Look`, 500)] : []),
      step('Set per-photo Corrections', 700, '1.4s'),
    ],
    prose: id
      ? `I made a ${lk.name} Look and applied it to all ${picks.length} Picks. I also set per-photo Corrections: ${lifted} got face light lifted, ${wb} indoor shots had white balance cooled to match, and ${exp} had exposure evened out. Nothing is reshaped.`
      : `Lifted the light on faces in ${lifted} Picks, so nobody is left in shadow.`,
    actions: [{ type: 'look/apply', look: { ...lk, adjust: { ...lk.adjust } }, corrections, ids: picks }],
    batchLabel: id ? `Agent: ${lk.name} on ${picks.length}` : `Agent: face light on ${lifted}`,
    changed: picks,
    summary,
  };
}

let seq = 0;
const newId = () => `m${Date.now().toString(36)}${seq++}`;

function edit(prompt: string, s: State, photoId: string): AgentPlan {
  const photo = PHOTO_BY_ID[photoId];
  const recipe = recipeOf(s, photoId);
  const clauses = prompt.toLowerCase().split(/,| and |;|\bthen\b/).map((c) => c.trim()).filter(Boolean);
  const actions: Action[] = [];
  const said: string[] = [];
  const summary: string[] = [];
  const steps: AgentStep[] = [];
  const findLocal = (name: string) => recipe.local.find((l) => l.name === name);
  const upsert = (name: string, mask: LocalAdjust['mask'], adjust: Adjust, line: string) => {
    const existing = findLocal(name);
    const local: LocalAdjust = existing ? { ...existing, adjust: { ...existing.adjust, ...adjust } } : { id: newId(), name, mask, adjust, source: 'ai' };
    actions.push({ type: 'recipe/local-upsert', id: photoId, local, label: `Agent: ${name}` });
    if (!existing) steps.push(step(`Made a ${name} Mask`, 650, `${(0.6 + Math.random()).toFixed(1)}s`));
    summary.push(line);
    said.push(name);
  };
  for (const c of clauses) {
    const bigger = /(more|lot)/.test(c) && !/less/.test(c);
    if (/(face|skin|her|him|them|people|eyes)/.test(c)) {
      if (!photo.faces.length) {
        said.push('no faces');
        continue;
      }
      const k = bigger ? 1.6 : 1;
      upsert('Face', { kind: 'faces' }, { exposure: 0.4 * k, skin: 20, eyes: 10 }, `Face: Exposure +${(0.4 * k).toFixed(2)}, Skin even 20, Eyes +10`);
    } else if (/sky/.test(c)) {
      if (photo.horizon < 0.1) {
        said.push('no sky');
        continue;
      }
      const t = /warm/.test(c) ? 12 : -12;
      upsert('Sky', { kind: 'sky' }, { temp: t, highlights: -15 }, `Sky: Temperature ${t > 0 ? '+' : '−'}12, Highlights −15`);
    } else if (/(shadow|left|dark)/.test(c)) {
      upsert('Left shadows', { kind: 'gradient', side: 'left', extent: 0.45 }, { shadows: 25 }, 'Left shadows: Shadows +25');
    } else if (/(background|behind|foreground|bottom|ground)/.test(c)) {
      upsert('Foreground', { kind: 'gradient', side: 'bottom', extent: 0.4 }, { exposure: -0.25, saturation: -10 }, 'Foreground: Exposure −0.25, Saturation −10');
    } else if (/(more|less|bit)/.test(c) && recipe.local.length) {
      const last = recipe.local[recipe.local.length - 1];
      const k = /less/.test(c) ? 0.6 : 1.5;
      const adjust = Object.fromEntries(Object.entries(last.adjust).map(([key, v]) => [key, Math.round(v * k * 100) / 100])) as Adjust;
      actions.push({ type: 'recipe/local-upsert', id: photoId, local: { ...last, adjust }, label: `Agent: ${last.name} ${k > 1 ? 'more' : 'less'}` });
      summary.push(`${last.name}: everything ×${k}`);
      said.push(`${k > 1 ? 'more' : 'less'} on ${last.name}`);
    } else if (/(warm|cool)/.test(c)) {
      const v = (recipe.global.temp ?? 0) + (/warm/.test(c) ? 10 : -10);
      actions.push({ type: 'recipe/global', id: photoId, key: 'temp', value: v, by: 'ai' });
      summary.push(`Temperature ${v > 0 ? '+' : '−'}${Math.abs(v)}`);
      said.push(/warm/.test(c) ? 'warmer overall' : 'cooler overall');
    } else if (/(bright|dark|exposure)/.test(c)) {
      const v = Math.round(((recipe.global.exposure ?? 0) + (/dark/.test(c) ? -0.3 : 0.3)) * 100) / 100;
      actions.push({ type: 'recipe/global', id: photoId, key: 'exposure', value: v, by: 'ai' });
      summary.push(`Exposure ${v >= 0 ? '+' : '−'}${Math.abs(v).toFixed(2)}`);
      said.push(/dark/.test(c) ? 'darker overall' : 'brighter overall');
    }
  }
  if (!actions.length) {
    return {
      steps: [step('Looked at the photo', 500)],
      prose: said.includes('no sky') ? 'There is no sky in this one; it was shot indoors. Try the faces, the shadows or the overall exposure.' : said.includes('no faces') ? 'There are no faces in this one. Try the sky, the shadows or the overall exposure.' : 'Try something like "brighten her face, cool the sky a bit, and lift the shadows on the left".',
      actions: [],
    };
  }
  steps.unshift(step(`Looked at ${photo.file}`, 500, '0.9s'));
  steps.push(step('Wrote the Recipe', 250, '0.1s'));
  const masks = said.filter((x) => !/overall|more|less/.test(x));
  return {
    steps,
    prose: masks.length
      ? `Done. I ${masks.length > 1 ? `made ${words(masks.length)} Masks (${masks.join(', ')})` : `worked on the ${masks[0]} Mask`} and adjusted ${masks.length > 1 ? 'each one' : 'it'}. The values are on the sliders so you can fine-tune.`
      : `Done: ${said.join(', ')}. Nudge the sliders if it's not quite right.`,
    actions,
    batchLabel: `Agent: ${said.join(', ')}`,
    changed: [photoId],
    summary,
  };
}

export function respond(ctx: string, prompt: string, s: State, extra: { selected?: Photo | null; picks?: string[] }): AgentPlan {
  if (ctx === 'cull') return cull(prompt, s, extra.selected ?? null);
  if (ctx === 'look') return look(prompt, s, extra.picks ?? []);
  if (ctx.startsWith('edit:')) return edit(prompt, s, ctx.slice(5));
  return { steps: [], prose: 'Not sure what to do here.', actions: [] };
}

/** Live: requests the engine's Agent carries out for real. */
export const LIVE_SUGGESTIONS: Record<string, string[]> = {
  cull: ['Cull this shoot', 'Make the whole shoot consistent', 'Warm and a little film-like for the Picks'],
  look: ['Make the whole shoot consistent', 'Warm, soft, a little film-like', 'A bit warmer and less contrast'],
  edit: ['Fix the lighting on this one', 'Make the people look their best', 'Give it a golden-hour look', 'Make the sky more dramatic'],
};

export const SUGGESTIONS: Record<string, string[]> = {
  cull: ['Be stricter on blur in the group shots', 'Reject anyone mid-blink', 'Why this Verdict?'],
  look: ['Warm, soft, a little film-like. Make sure faces are well lit', 'Cool and moody', 'A bit warmer and less grain'],
  edit: ['Brighten her face, cool the sky a bit, and lift the shadows on the left', 'A bit more on the face', 'Darken the foreground'],
};
