// Prepare, Run it all and Finish (engine/src/prepare.rs, GLOSSARY): the heavy work for a new
// Shoot done up front so the person can walk away (Greg, 2026-11-02). Work mode (⌘5) shows it
// in full; the Status bar shows a line. The demo has a Shoot partway through its Prepare.

import { useEffect, useRef } from "react";
import { call } from "./backend";
import { usePoll } from "./poll";
import { LIVE } from "./live";
import { toastFailure } from "@/lib/utils";

export interface Stage {
  label: string;
  kinds: string[];
  done: number;
  failed: number;
  total: number;
  running: boolean;
  eta_s: number | null;
  /** Seconds the finished jobs took. */
  seconds: number;
  /** A per-Shoot job's own progress (previews, Analyzers), in Photos. */
  progress: { done: number; total: number } | null;
  /** Left for Finish (Keywords, Backup). */
  later: boolean;
  state: "none" | "later" | "running" | "done";
}

export interface Prepare {
  shoot_ids: number[];
  shoot_names: string[];
  photos: number;
  keepers: number;
  since: string;
  state: "running" | "battery" | "paused" | "done";
  gentle: boolean;
  eta_s: number | null;
  stages: Stage[];
  finished_at?: string;
}

export interface PrepareStatus {
  prepare: Prepare | null;
  last_done: Prepare | null;
  /** What Run it all would run: jobs by kind, and the seconds it would take. */
  held: { jobs: Record<string, number>; eta_s: number };
}

const st = (
  label: string,
  kinds: string[],
  done: number,
  total: number,
  eta_s: number | null,
  seconds: number,
  later = false,
): Stage => ({
  label,
  kinds,
  done,
  failed: 0,
  total,
  running: done < total,
  eta_s,
  seconds,
  progress: null,
  later,
  state: later
    ? "later"
    : total === 0
      ? "none"
      : done < total
        ? "running"
        : "done",
});

const DEMO: PrepareStatus = {
  prepare: {
    shoot_ids: [1],
    shoot_names: ["2026-10-12_Tahoe"],
    photos: 214,
    keepers: 96,
    since: new Date(Date.now() - 14 * 60_000).toISOString(),
    state: "running",
    gentle: false,
    eta_s: 38 * 60,
    stages: [
      st("Previews", ["render_previews"], 1, 1, null, 180),
      st("Faces, focus, level", ["analyze"], 1, 1, null, 240),
      st("Stacks and AI flags", ["check_shoot", "cull_shoot"], 1, 1, null, 8),
      st(
        "Edits for likely keepers",
        ["suggest_shoot", "suggest"],
        33,
        96,
        24 * 60,
        33 * 26,
      ),
      st("Denoise noisy keepers", ["denoise"], 4, 41, 14 * 60, 4 * 34),
      st("Keywords", ["tag_photos"], 0, 0, null, 0, true),
      st("Backup", ["backup"], 0, 0, null, 0, true),
    ],
  },
  last_done: null,
  held: { jobs: { suggest: 84, tag_photos: 2, denoise: 12 }, eta_s: 85 * 60 },
};

export function usePrepare(): PrepareStatus | null {
  const p = usePoll<PrepareStatus>(LIVE ? "prepare_status" : null, {}, 3000);
  return LIVE ? (p ?? null) : DEMO;
}

const act = (command: string, args: Record<string, unknown>, what: string) =>
  LIVE
    ? call(command, args).then(() => undefined, toastFailure(what))
    : Promise.resolve();

/** Run it all: every held job now at Full, keeping the Mac awake, until the Queue is empty. */
export const runAll = () =>
  act("set_queue", { run_now: true }, "Couldn’t start Run it all");
/** Cull now: Prepare carries on at the Compute level so the app stays quick. */
export const cullNow = (gentle = true) =>
  act("prepare_shoot", { gentle }, "Couldn’t change Prepare");
export const stopPrepare = () =>
  act("prepare_shoot", { stop: true }, "Couldn’t stop Prepare");
export const runOnBattery = () =>
  act("set_queue", { run_anyway: true }, "Couldn’t run on battery");
export const finishShoot = (shootId: number) =>
  LIVE
    ? call<{ backups: unknown[]; tag_job: number }>("finish_shoot", {
        shoot_id: shootId,
      })
    : Promise.resolve({ backups: [], tag_job: 0 });

/** "1:05 PM", `s` seconds from now. */
export const clockIn = (s: number) =>
  new Date(Date.now() + s * 1000).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });

/** How many jobs Run it all would run. */
export const heldCount = (p: PrepareStatus | null) =>
  Object.values(p?.held.jobs ?? {}).reduce((a, b) => a + b, 0);

/** Say so once when a Prepare finishes: a toast, and a macOS notification when the window isn't in front. */
export function useReadyNotice(
  p: PrepareStatus | null,
  say: (text: string) => void,
) {
  const seen = useRef<string | null | undefined>(undefined);
  const at = p?.last_done?.finished_at ?? null;
  useEffect(() => {
    if (!p) return;
    // The first answer is what was already done before this window looked.
    if (seen.current === undefined) {
      seen.current = at;
      return;
    }
    if (!at || at === seen.current) return;
    seen.current = at;
    const d = p.last_done!;
    const text = `${d.shoot_names.join(", ")} is ready: ${d.photos} Photos, edits for ${d.keepers}`;
    say(text);
    try {
      if (document.hidden && "Notification" in window) {
        if (Notification.permission === "granted")
          new Notification("Argus", { body: text });
        else if (Notification.permission !== "denied")
          void Notification.requestPermission();
      }
    } catch {
      // No notifications here (a browser without them): the toast says it.
    }
  }, [p, at, say]);
}
