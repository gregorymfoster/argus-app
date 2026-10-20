// Where each Photo of the open Shoot stands with the background AI (Greg, 2026-10-18: "I
// can't tell which ones have been AI processed and which are still enqueued"). Read from
// the queue: the running job and the queued ones, polled gently.

import { useMemo } from 'react';
import { usePoll } from './poll';
import { LIVE, liveShootId } from './live';
import { PHOTOS } from './dummy';

/** `working`: the AI is on it now. `queued`: waiting its turn. `null`: nothing pending. */
export type AiState = 'working' | 'queued' | null;

interface JobLite {
  id: number;
  kind: string;
  args: { photo_id?: number; shoot_id?: number };
}

export interface AiStatus {
  /** The AI state of a Photo by id. Photos that have Suggestions waiting count as done. */
  of: (id: string, hasTakes: boolean) => AiState;
  queued: number;
}

const forShoot = (j: JobLite) => j.args.shoot_id == null || !liveShootId() || j.args.shoot_id === liveShootId();

function status(running: JobLite | null, queued: JobLite[]): AiStatus {
  const workingIds = new Set<number>();
  const queuedIds = new Set<number>();
  let shootWorking = false;
  let shootQueued = false;
  if (running) {
    if (running.kind === 'suggest' && running.args.photo_id != null) workingIds.add(running.args.photo_id);
    if ((running.kind === 'cull_shoot' || running.kind === 'suggest_shoot') && forShoot(running)) shootWorking = true;
  }
  for (const j of queued) {
    if (j.kind === 'suggest' && j.args.photo_id != null) queuedIds.add(j.args.photo_id);
    if ((j.kind === 'cull_shoot' || j.kind === 'suggest_shoot') && forShoot(j)) shootQueued = true;
  }
  const of = (id: string, hasTakes: boolean): AiState => {
    // Suggestions waiting to be reviewed: done for now, even if more are queued.
    if (hasTakes) return null;
    const n = Number(id.replace(/\D/g, ''));
    if (workingIds.has(n)) return 'working';
    if (queuedIds.has(n)) return 'queued';
    if (shootWorking) return 'working';
    if (shootQueued) return 'queued';
    return null;
  };
  return { of, queued: queuedIds.size };
}

// The demo: the cull is done, Suggestions are partway through the Shoot.
function demo(): AiStatus {
  const ids = PHOTOS.map((p) => Number(p.id.replace(/\D/g, '')));
  const at = Math.floor(ids.length * 0.4);
  return status({ id: 1, kind: 'suggest', args: { photo_id: ids[at] } }, ids.slice(at + 1).map((photo_id, i) => ({ id: i + 2, kind: 'suggest', args: { photo_id } })));
}

const none: AiStatus = { of: () => null, queued: 0 };

/** The AI state of every Photo in the open Shoot, refreshed every few seconds. */
export function useAiStatus(): AiStatus {
  // The running job comes with the Queue chip's poll; only the queued list is its own.
  const q = usePoll<{ running: JobLite | null }>(LIVE ? 'progress' : null, {}, 5000);
  const jobs = usePoll<JobLite[]>(LIVE ? 'list_jobs' : null, { status: 'queued', limit: 1000 }, 5000);
  return useMemo(() => (!LIVE ? demo() : q && jobs ? status(q.running, jobs) : none), [q, jobs]);
}
