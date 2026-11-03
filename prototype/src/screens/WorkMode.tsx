// Work mode (⌘5; Greg 2026-11-02, mocked on the One-sitting Shoot page): the Queue in full.
// It opens on Prepare after an Import: when the new Shoot will be ready, each stage with a
// bar and ETA, where the time goes, what the Mac is doing, and the jobs, grouped by kind.
// Run it all (⇧⌘R) is always here while anything is held; Finish lives in Export.

import { useEffect, useState } from "react";
import { useHotkeys } from "react-hotkeys-hook";
import {
  BatteryMedium,
  Check,
  Coffee,
  Pause,
  Play,
  Thermometer,
  Zap,
} from "lucide-react";
import { go } from "../router";
import { call } from "../data/backend";
import { LIVE } from "../data/live";
import { whileShown } from "../data/poll";
import {
  clockIn,
  cullNow,
  heldCount,
  runAll,
  runOnBattery,
  stopPrepare,
  usePrepare,
  type Prepare,
  type Stage,
} from "../data/prepare";
import { COMPUTE_LABEL, dur, useQueueOrDemo } from "../ui/Queue";
import { ai, gb, pct, Spark, useMeter } from "../ui/Meter";
import { HeadLead, Hints, ModeSwitch } from "../ui/ViewBar";
import { Kbd, Seg, ESC_OUTSIDE_DIALOGS } from "../ui/kit";
import { eta } from "../ui/Progress";
import { Button } from "@/components/ui/button";
import { StatusFoot } from "../ui/StatusBar";
import { RunningRow, useRunning } from "../ui/Running";
import { toastFailure } from "@/lib/utils";

/** What a job kind is, for the grouped list. */
const KIND: Record<string, string> = {
  suggest: "Suggestions",
  suggest_shoot: "Picking likely keepers",
  denoise: "Denoise",
  render_previews: "Previews",
  analyze: "Faces, focus, level",
  check_shoot: "AI flags",
  cull_shoot: "AI cull",
  tag_photos: "Keywords",
  backup: "Backup",
  match_hero: "Apply to the rest",
  pre_edit_shoot: "Shoot match",
  rank_presets: "Presets for you",
  learn_edits: "Learning your edits",
  download_model: "Model download",
  resume_run: "Finishing interrupted work",
};

/** Why a job or run stopped part way and waits to carry on (engine `interrupted`, run `note`). */
const INTERRUPTED: Record<string, string> = {
  asleep: "Paused while asleep",
  quit: "Paused when Argus quit",
  card: "Paused: card not connected",
};

interface Job {
  id: number;
  kind: string;
  label: string;
  priority: number;
  done: number;
  total: number;
  /** Queued again part way: `asleep` or `quit`. It carries on from where it got to. */
  interrupted?: string | null;
}

/** An Export or Import not finished (engine `list_runs`): paused ones wait to carry on. */
interface Run {
  id: number;
  kind: string;
  label: string;
  status: string;
  note: string | null;
  done: number;
  total: number;
}

function useRuns(): Run[] {
  const [runs, setRuns] = useState<Run[]>([]);
  useEffect(() => {
    if (!LIVE) return;
    const load = () => void call<Run[]>("list_runs").then(setRuns, () => {});
    load();
    return whileShown(load, 5000);
  }, []);
  return runs;
}

function useQueuedJobs(): Job[] {
  const [jobs, setJobs] = useState<Job[]>([]);
  useEffect(() => {
    if (!LIVE) {
      setJobs([
        { id: 99, kind: "suggest", label: "Suggestions for DSC08005", priority: 10, done: 0, total: 0, interrupted: "asleep" },
        ...Array.from({ length: 63 }, (_, i) => ({
          id: 100 + i,
          kind: "suggest",
          label: "",
          priority: 10,
          done: 0,
          total: 0,
        })),
        ...Array.from({ length: 37 }, (_, i) => ({
          id: 200 + i,
          kind: "denoise",
          label: "",
          priority: 2,
          done: 0,
          total: 0,
        })),
      ]);
      return;
    }
    const load = () =>
      void call<Job[]>("list_jobs", { status: "queued", limit: 1000 }).then(
        setJobs,
        () => {},
      );
    load();
    return whileShown(load, 3000);
  }, []);
  return jobs;
}

/** The Shoot's stages: a bar, the count and the time left (or taken). */
function StageRow({ s }: { s: Stage }) {
  const unit =
    s.progress ?? (s.total ? { done: s.done, total: s.total } : null);
  const frac =
    s.state === "done" ? 1 : unit && unit.total ? unit.done / unit.total : 0;
  const right =
    s.state === "later"
      ? "at Finish"
      : s.state === "done"
        ? s.seconds
          ? dur(s.seconds)
          : "done"
        : s.state === "none"
          ? "–"
          : (eta(s.eta_s) ?? "");
  return (
    <div className={`wm-stage ${s.state}`} data-testid="work-stage">
      <span className="wm-stage-name trunc">{s.label}</span>
      <span className={`job-bar ${s.state === "done" ? "done" : ""}`}>
        <span style={{ width: `${Math.round(frac * 100)}%` }} />
      </span>
      <span className="mono faint wm-num">
        {unit && s.state !== "later" ? `${unit.done}/${unit.total}` : ""}
      </span>
      <span className="mono faint wm-num">{right}</span>
    </div>
  );
}

/** Where the time goes: each stage's spent part solid, what's left faint, on one scale. */
function TimeBars({ p }: { p: Prepare }) {
  const rows = p.stages.filter((s) => !s.later && s.state !== "none");
  const max = Math.max(60, ...rows.map((s) => s.seconds + (s.eta_s ?? 0)));
  return (
    <div className="wm-time" data-testid="work-time">
      {rows.map((s) => (
        <div key={s.label} className="wm-time-row">
          <span className="trunc faint">{s.label}</span>
          <span className="wm-time-track">
            <span
              className="spent"
              style={{ width: `${(s.seconds / max) * 100}%` }}
            />
            <span
              className="left"
              style={{ width: `${((s.eta_s ?? 0) / max) * 100}%` }}
            />
          </span>
          <span className="mono faint wm-num">
            {dur(s.seconds + (s.eta_s ?? 0))}
          </span>
        </div>
      ))}
    </div>
  );
}

/** CPU, GPU and memory over the last minutes, and the pace. */
function MacCard() {
  const m = useMeter(true);
  if (!m) return <p className="faint">Reading the Mac…</p>;
  const all = (k: "cpu_pct" | "gpu_pct" | "mem_bytes") =>
    m.history.map((h) => {
      const a = ai(h);
      const argus = h.argus;
      return k === "mem_bytes"
        ? a.mem_bytes + (argus?.mem_bytes ?? 0)
        : (a[k] ?? 0) + (argus?.[k] ?? 0);
    });
  const now = {
    cpu: all("cpu_pct").at(-1) ?? 0,
    gpu: all("gpu_pct").at(-1) ?? 0,
    mem: all("mem_bytes").at(-1) ?? 0,
  };
  const rows: [string, number[], number, string][] = [
    ["CPU", all("cpu_pct"), 400, pct(now.cpu)],
    ...(m.energy
      ? ([["GPU", all("gpu_pct"), 100, pct(now.gpu)]] as [
          string,
          number[],
          number,
          string,
        ][])
      : []),
    [
      "Memory",
      all("mem_bytes"),
      m.mem_total_bytes,
      `${gb(now.mem)} of ${gb(m.mem_total_bytes)}`,
    ],
  ];
  return (
    <div className="wm-sparks">
      {rows.map(([label, values, max, v]) => (
        <div key={label} className="wm-spark">
          <span className="faint">{label}</span>
          <Spark
            values={values}
            max={Math.max(max, ...values)}
            w={160}
            h={22}
          />
          <span className="mono wm-num">{v}</span>
        </div>
      ))}
    </div>
  );
}

export function WorkScreen() {
  const s = usePrepare();
  const q = useQueueOrDemo();
  const jobs = useQueuedJobs();
  const runs = useRuns();
  // Import, Export and AI work run next to the Queue's job: all of it shows here (Greg, 2026-11-02).
  const running = useRunning(q);
  const p = s?.prepare ?? null;
  const held = heldCount(s);
  const [, tick] = useState(0);
  // The ready time moves with the clock.
  useEffect(() => whileShown(() => tick((n) => n + 1), 30_000), []);
  const set = (args: Record<string, unknown>) =>
    LIVE
      ? call("set_queue", args).then(
          () => undefined,
          toastFailure("Couldn’t change Background work"),
        )
      : Promise.resolve();

  useHotkeys(
    "space",
    () => q && void set({ paused: !q.paused }),
    { preventDefault: true },
    [q],
  );
  useHotkeys(
    "enter",
    () => p && (void cullNow(true), go({ name: "moments" })),
    { preventDefault: true },
    [p],
  );
  useHotkeys(["escape"], () => go({ name: "cull" }), ESC_OUTSIDE_DIALOGS);

  const groups = new Map<string, number>();
  for (const j of jobs) if (!j.interrupted) groups.set(j.kind, (groups.get(j.kind) ?? 0) + 1);
  // Work a closed lid, a quit or an ejected card stopped part way, each on its own row.
  const paused = [
    ...jobs.filter((j) => j.interrupted).map((j) => ({ key: `job-${j.id}`, label: j.label, why: INTERRUPTED[j.interrupted!] ?? "Paused" })),
    ...runs.filter((r) => r.status === "paused" && r.note === "card").map((r) => ({ key: `run-${r.id}`, label: r.label, why: INTERRUPTED.card })),
  ];
  const done = p && p.state === "done";
  const last = !p ? (s?.last_done ?? null) : null;
  const battery = p?.state === "battery" || (!!q?.battery_paused && held > 0);

  return (
    <div
      className="panel main view"
      style={{
        flexDirection: "column",
        borderRadius: 0,
        border: "none",
        background: "var(--bg)",
      }}
      data-testid="work-screen"
    >
      <header className="panel-head view-head">
        <HeadLead>
          <span className="view-file">
            {p ? "Preparing" : "Background work"}
          </span>
          <span className="mono faint view-meta trunc">
            {p
              ? p.shoot_names.join(" · ")
              : running.length
                ? running.map((w) => w.label).join(" · ")
                : `${q?.queued ?? 0} waiting`}
          </span>
        </HeadLead>
        <ModeSwitch mode="work" />
        <span className="grow" />
        {q && (
          <Seg
            label="Compute"
            quiet
            value={q.compute}
            onChange={(v) => void set({ compute: v })}
            items={(["gentle", "balanced", "full"] as const).map((c) => ({
              value: c,
              label: COMPUTE_LABEL[c],
            }))}
          />
        )}
        {q && (
          <Button
            size="sm"
            onClick={() => void set({ paused: !q.paused })}
            data-testid="work-pause"
          >
            {q.paused ? <Play size={12} /> : <Pause size={12} />}{" "}
            {q.paused ? "Resume" : "Pause"}
          </Button>
        )}
        {p && (
          <Button
            size="sm"
            variant="default"
            onClick={() => (void cullNow(true), go({ name: "moments" }))}
            data-testid="work-cull-now"
          >
            Cull now <Kbd>↵</Kbd>
          </Button>
        )}
      </header>

      <div className="wm-body">
        <section className="wm-top" data-testid="work-top">
          <div className="wm-ready">
            {p ? (
              <>
                <span className="wm-eyebrow">{p.shoot_names.join(" · ")}</span>
                <span className="wm-big" data-testid="work-ready">
                  {done
                    ? "Ready"
                    : p.state === "battery"
                      ? "Waiting for the charger"
                      : p.state === "paused"
                        ? "Paused"
                        : p.eta_s != null
                          ? `Ready at ${clockIn(p.eta_s)}`
                          : "Starting"}
                </span>
                <span className="mono faint">
                  {[
                    p.eta_s != null && !done ? `${dur(p.eta_s)} left` : null,
                    `${p.photos} Photos`,
                    p.keepers ? `${p.keepers} likely keepers` : null,
                    p.gentle
                      ? "at your Compute level while you cull"
                      : "at Full",
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
                <PrepareBar p={p} />
              </>
            ) : last ? (
              <>
                <span className="wm-eyebrow">
                  {last.shoot_names.join(" · ")}
                </span>
                <span className="wm-big done" data-testid="work-ready">
                  <Check size={14} /> Ready
                </span>
                <span className="mono faint">
                  {last.photos} Photos · edits for {last.keepers} · finished{" "}
                  {last.finished_at
                    ? new Date(last.finished_at).toLocaleTimeString([], {
                        hour: "numeric",
                        minute: "2-digit",
                      })
                    : ""}
                </span>
              </>
            ) : (
              <>
                <span className="wm-big">Nothing preparing</span>
                <span className="faint">
                  Import a card and Argus prepares it here: previews, AI flags,
                  then edits and denoise for every likely keeper.
                </span>
              </>
            )}
          </div>
          <div className="wm-pills">
            {q?.on_battery ? (
              <span className="wm-pill warn">
                <BatteryMedium size={11} /> On battery
              </span>
            ) : (
              <span className="wm-pill ok">
                <Zap size={11} /> Plugged in
              </span>
            )}
            {(p && p.state === "running") || q?.run_now ? (
              <span
                className="wm-pill warn"
                title="A closed lid still sleeps a MacBook that drives no display"
              >
                <Coffee size={11} /> Keeping the Mac awake · lid open
              </span>
            ) : null}
            {q?.thermal_paused && (
              <span className="wm-pill warn">
                <Thermometer size={11} /> Hot: waiting to cool
              </span>
            )}
          </div>
        </section>

        {battery && (
          <div className="wm-callout" data-testid="work-battery">
            <BatteryMedium size={12} />
            <span className="grow">
              On battery the heavy work waits for the charger, and starts by
              itself when you plug in. Previews and AI flags are done, so you
              can cull now either way.
            </span>
            <Button
              size="sm"
              onClick={() => void runOnBattery()}
              data-testid="work-run-on-battery"
            >
              Run on battery
            </Button>
          </div>
        )}

        {held > 0 && !q?.run_now && (
          <div className="wm-runall" data-testid="work-run-all">
            <div className="grow">
              <div className="wm-h">All the hard work, now</div>
              <span className="faint">
                {Object.entries(s!.held.jobs)
                  .map(([k, n]) => `${n} ${KIND[k] ?? k}`)
                  .join(", ")}{" "}
                waiting for idle time or overnight. About {dur(s!.held.eta_s)}{" "}
                at Full, keeping the Mac awake.
              </span>
            </div>
            <Button
              size="sm"
              variant="default"
              onClick={() => void runAll()}
              data-testid="work-run-all-button"
            >
              Run it all <Kbd>⇧⌘R</Kbd>
            </Button>
          </div>
        )}
        {q?.run_now && (
          <div className="wm-callout" data-testid="work-running-all">
            <Zap size={12} />
            <span className="grow">
              Running it all at Full until the queue is empty
              {q.eta_s ? `, about ${dur(q.eta_s)}` : ""}.
            </span>
            <Button size="sm" onClick={() => void set({ run_now: false })}>
              Stop
            </Button>
          </div>
        )}

        <div className="wm-grid">
          {p && (
            <section className="wm-card" data-testid="work-stages">
              <div className="wm-card-head">
                <span className="wm-h">Stages</span>
                <span className="grow" />
                <button
                  className="import-link"
                  onClick={() => void stopPrepare()}
                  data-testid="work-stop-prepare"
                >
                  Stop preparing
                </button>
              </div>
              {p.stages.map((st) => (
                <StageRow key={st.label} s={st} />
              ))}
            </section>
          )}
          {p && (
            <section className="wm-card">
              <div className="wm-card-head">
                <span className="wm-h">Where the time goes</span>
                <span className="grow" />
                <span className="mono faint">spent · left</span>
              </div>
              <TimeBars p={p} />
            </section>
          )}
          <section className="wm-card">
            <div className="wm-card-head">
              <span className="wm-h">Mac</span>
              <span className="grow" />
              <span className="mono faint">last 10 min · AI and Argus</span>
            </div>
            <MacCard />
          </section>
          <section className="wm-card" data-testid="work-jobs">
            <div className="wm-card-head">
              <span className="wm-h">Running</span>
            </div>
            {running.map((w) => (
              <RunningRow key={w.key} w={w} />
            ))}
            {!running.length && (
              <span className="faint">
                {q?.paused ? "Paused" : "Nothing running"}
              </span>
            )}
            <div className="wm-card-head">
              <span className="wm-h">Waiting</span>
              <span className="grow" />
              <span className="mono faint">{jobs.length}</span>
            </div>
            {paused.map((j) => (
              <div key={j.key} className="wm-job wm-paused" data-testid="work-interrupted" title={`${j.why}: carries on from where it got to`}>
                <span className="wm-dot" />
                <span className="trunc">{j.label}</span>
                <span className="faint wm-num">{j.why}</span>
              </div>
            ))}
            {[...groups.entries()]
              .sort((a, b) => b[1] - a[1])
              .map(([kind, n]) => (
                <div key={kind} className="wm-job" data-testid="work-group">
                  <span className="wm-dot" />
                  <span className="trunc">
                    {KIND[kind] ?? kind} · {n}
                  </span>
                </div>
              ))}
            {!groups.size && !paused.length && <span className="faint">Nothing waiting</span>}
          </section>
        </div>
      </div>

      <StatusFoot className="view-foot">
        <span>
          {p
            ? p.gentle
              ? "Culling while it prepares"
              : "Walk away: Argus keeps going"
            : "Background work"}
        </span>
        <span className="grow" />
        <Hints
          items={[
            ["Space", q?.paused ? "Resume" : "Pause"],
            ...(p ? ([["↵", "Cull now"]] as [string, string][]) : []),
            ["⇧⌘R", "Run it all"],
            ["Esc", "Library"],
          ]}
        />
      </StatusFoot>
    </div>
  );
}

/** One bar for the whole Prepare: finished jobs over all of them. */
function PrepareBar({ p }: { p: Prepare }) {
  const now = p.stages.filter((s) => !s.later);
  const total = now.reduce((a, s) => a + s.seconds + (s.eta_s ?? 0), 0);
  const spent = now.reduce((a, s) => a + s.seconds, 0);
  const frac = p.state === "done" ? 1 : total ? spent / total : 0;
  return (
    <span className="job-bar wm-mainbar" data-testid="work-bar">
      <span style={{ width: `${Math.round(frac * 100)}%` }} />
    </span>
  );
}
