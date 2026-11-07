---
name: queue
description: TLA+ model of the background Queue shared by every Argus process on a Library (app Worker, argus work/serve/mcp): enqueue with dedupe, claim, settle, cancel, retry, requeue after a quit or crash. Use before and after changing library/jobs.rs, jobs/run.rs settle, or jobs/worker.rs, and to read a broken rule from `scripts/formal`.
---

# Queue, model-checked

`formal/Queue.tla` models the `jobs` table with one or two Argus processes working it (the app's Worker beside `argus work`, `serve` or `mcp`), two threads queueing work, processes starting, crashing and quitting, and jobs that succeed, fail, fail across a sleep, are cancelled or retried. TLC tries every interleaving. Each action is one SQLite statement or transaction; the header maps them to code. Created 2026-11-06 under the routine in `docs/formal.md`.

## Run it

```sh
scripts/formal                               # every model; ~3 s
scripts/formal --trace Queue-p2 NoDoubleRun  # shortest path to a broken rule
scripts/formal --mutations                   # every rule catches its bug
scripts/formal --stale                       # covered code changed since the sync commit
```

## The rules

Configs: `Queue-p1` (the app alone: 3 jobs, jobs 1 and 2 share a dedupe key, 2 starts and 1 crash) and `Queue-p2` (the app and a second process: 2 jobs sharing a key, 1 start and 1 crash each).

| Rule | In words | Harm if broken |
| --- | --- | --- |
| `NoDoubleRun` | No job runs in two processes at once | Two culls of one Shoot, two exports, two backups racing |
| `RunnerSeesRunning` | While a process runs a job, the Queue shows it running (or cancelled) | Work mode lies; a second process takes it |
| `OneActivePerKey` | At most one queued or running job per dedupe key | The same work queued and run twice |
| `CancelSticks` | A job the person cancelled stays cancelled | A cancel that doesn't stick |

## Findings, fixed 2026-11-06

Both reproduced by tests in `engine/tests/resume.rs` and kept as mutations in `formal/Queue.mutations`.

- **Two processes, one job.** Every process starting on a Library ran `requeue_stale_jobs`, which put *every* running job back to queued, including one the app was running. `argus work` started beside the open app would then run it a second time. Fix: `claim_with` records the process (`jobs.pid`, schema v15) and `requeue_stale_jobs` takes only jobs whose process is gone. `Rounds::tick` also calls it every few minutes, so a killed `argus work` leaves no job stuck as running.
- **The same work queued twice.** `enqueue_job` read for a job with the dedupe key, then inserted: two threads could both see none. Fix: one `INSERT ... WHERE NOT EXISTS`.

## When you change the covered code

1. `scripts/formal --stale`, then read the diff against the action map in `formal/Queue.tla`'s header. A step is one SQL statement or one transaction; two statements are two steps.
2. Mirror a change in the model, run `scripts/formal`. A newly broken rule: `--trace`; a path the code can take is a bug in the change, a path it can't is a modeling error (never weaken the rule).
3. New rule: add a mutation that breaks it; `--mutations` must say DETECTED for all.
4. Move `sync:` below once the model describes HEAD.

## What it does not cover

Priorities, holds and which queued job is picked (the model lets any be claimed); progress and `interrupted` labels; reorder; pid reuse after a reboot (a dead job's pid taken by an unrelated process keeps it running until that process ends); liveness (that every queued job eventually runs). Bounds: 2 processes, 3 jobs, 2 starts, 1 crash.

## Coverage record

model: Queue
sync: a395db30f6e0386949e645bd15d7e720ac532528

- covers: engine/src/library/jobs.rs
- covers: engine/src/jobs/run.rs
- covers: engine/src/jobs/worker.rs

Functions: `Library::{enqueue_job, claim_with, finish_job, cancel_job, retry_failed_jobs, requeue_stale_jobs, interrupt_job}`, `jobs::settle`, `Rounds::tick`, `Worker::start`.
