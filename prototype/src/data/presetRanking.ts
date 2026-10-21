// The AI's suggested order of the Presets for the open Shoot (engine `rank_presets`, cached
// per Shoot). Live, the first look asks the engine to rank in the background when it hasn't
// yet; "Rank now" puts that job first. In the demo, a stand-in order from the Photo keeps the
// Suggested row on screen.

import { useEffect } from 'react';
import { call } from './backend';
import { LIVE, chosenProvider, liveShootId } from './live';
import { refreshPolls, usePoll } from './poll';
import type { Ranking } from '../domain/presets';
import type { Photo } from '../domain/types';

type Job = { id: number; status: string; done: number; total: number; args: { shoot_id?: number }; error?: string | null };

export interface PresetRanking {
  ranking: Ranking | null;
  /** The ranking job for this Shoot, while it's queued or running. */
  job: Job | null;
  /** Why the last one failed, if it did and nothing is queued. */
  error: string | null;
  rankNow: () => Promise<void>;
}

const asked = new Set<number>();

const DEMO_PEOPLE = ['film-portrait', 'portrait-soft', 'natural', 'portrait-polish', 'warm-film', 'bw-soft'];
const DEMO_SCENE = ['land-clear', 'natural', 'land-golden', 'sky-blue', 'film-chrome', 'bw-dark-skies'];

function demo(photo: Photo): Ranking {
  const ids = photo.faces.length ? DEMO_PEOPLE : DEMO_SCENE;
  return { ranked: ids.map((id, i) => ({ id, score: 3 - i * 0.4 })), photos: [], provider: 'demo', at: '' };
}

export function usePresetRanking(photo: Photo): PresetRanking {
  const shoot = liveShootId();
  const list = usePoll<{ ranking: Ranking | null }>(LIVE && shoot ? 'list_presets' : null, { shoot_id: shoot }, 20000);
  const jobs = usePoll<Job[]>(LIVE && shoot ? 'list_jobs' : null, { kind: 'rank_presets', limit: 20 }, 5000);
  const mine = (jobs ?? []).filter((j) => j.args?.shoot_id === shoot);
  const job = mine.find((j) => j.status === 'queued' || j.status === 'running') ?? null;
  const failed = !job ? mine.find((j) => j.status === 'failed') : undefined;
  const ranking = LIVE ? (list?.ranking ?? null) : demo(photo);

  // First look at a Shoot without a ranking: queue one in the background, once per session.
  useEffect(() => {
    if (!LIVE || !shoot || !list || list.ranking || asked.has(shoot)) return;
    asked.add(shoot);
    call('rank_presets', { shoot_id: shoot, provider: chosenProvider() })
      .then(() => refreshPolls('list_jobs'))
      .catch(() => {});
  }, [shoot, list]);

  // When a job finishes, pick its ranking up.
  const running = !!job;
  useEffect(() => {
    if (!running) refreshPolls('list_presets');
  }, [running]);

  const rankNow = async () => {
    if (!LIVE || !shoot) return;
    await call('rank_presets', { shoot_id: shoot, refresh: true, now: true, provider: chosenProvider() });
    refreshPolls('list_jobs');
  };
  return { ranking, job, error: failed?.error ?? null, rankNow };
}
