------------------------------- MODULE Queue -------------------------------
(***************************************************************************)
(* The background Queue (Library jobs table) shared by every Argus process *)
(* that works a Library: the app's Worker, `argus work`, `argus serve` and *)
(* `argus mcp` each go round it (jobs/worker.rs). Processes start, crash   *)
(* or quit; jobs fail, fail across a sleep, are cancelled and retried.     *)
(* Checked with TLC by scripts/formal; skill .claude/skills/queue-model.   *)
(*                                                                         *)
(* Each action is one SQLite statement or transaction in the code:         *)
(*   Start     Worker::start / argus work: Library::requeue_stale_jobs,    *)
(*             one UPDATE of the running jobs whose process is gone        *)
(*   Reap      Rounds::tick every few minutes: requeue_stale_jobs again    *)
(*   Crash     a process dies or quits; the job it ran stays as it was     *)
(*   Enqueue   Library::enqueue_job: one INSERT ... WHERE NOT EXISTS a     *)
(*             queued or running job with the same dedupe_key              *)
(*   Claim     Library::claim_with: one transaction, SELECT the next       *)
(*             queued job, UPDATE it running (pid = this process) WHERE    *)
(*             status = 'queued'                                           *)
(*   Succeed   jobs/run.rs settle -> finish_job(Ok) WHERE running          *)
(*   Fail      settle -> finish_job(Err) WHERE running                     *)
(*   Asleep    settle -> interrupt_job(asleep) WHERE running               *)
(*   Cancel    Library::cancel_job: queued or running -> cancelled         *)
(*   Retry     Library::retry_failed_jobs (newest failed of each key with  *)
(*             nothing queued or running; the other failed -> cancelled)   *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    P,           \* Argus processes on one Library: 1 the app, 2 `argus work` (or serve, mcp)
    C,           \* threads that queue jobs (a command, the Worker)
    J,           \* job rows that may be queued; jobs 1 and 2 share a dedupe key
    MaxStarts,   \* starts per process (the first included)
    MaxCrashes   \* crashes per process

Procs == 1..P
Callers == 1..C
Jobs == 1..J
Key == [j \in Jobs |-> IF j <= 2 THEN 1 ELSE j]

Status == {"none", "queued", "running", "done", "failed", "cancelled", "deduped"}
Active == {"queued", "running"}

VARIABLES
    status,     \* jobs.status; "none" = not queued yet, "deduped" = enqueue returned another
    up,         \* up[p]: process p is alive
    work,       \* work[p]: the job p is executing, 0 = none
    ins,        \* ins[c]: the job caller c checked and is about to insert, 0 = none
    starts,
    crashes,
    cancelled,  \* jobs the person cancelled (history)
    ran,        \* ran[j]: how many processes have executed j at once at most (history)
    owner       \* owner[j]: the process that claimed j last

vars == <<status, up, work, ins, starts, crashes, cancelled, ran, owner>>

Init ==
    /\ status = [j \in Jobs |-> "none"]
    /\ up = [p \in Procs |-> FALSE]
    /\ work = [p \in Procs |-> 0]
    /\ ins = [c \in Callers |-> 0]
    /\ starts = [p \in Procs |-> 0]
    /\ crashes = [p \in Procs |-> 0]
    /\ cancelled = {}
    /\ ran = [j \in Jobs |-> 0]
    /\ owner = [j \in Jobs |-> 0]

Runners(j) == {p \in Procs : work[p] = j}

(* requeue_stale_jobs: a running job whose process is gone goes back to   *)
(* queued (process p's own earlier life included: a restart has a new pid). *)
Stale(p, j) == status[j] = "running" /\ (owner[j] = p \/ ~up[owner[j]])

Requeue(p) == [j \in Jobs |-> IF Stale(p, j) THEN "queued" ELSE status[j]]

Start(p) ==
    /\ ~up[p] /\ starts[p] < MaxStarts
    /\ up' = [up EXCEPT ![p] = TRUE]
    /\ starts' = [starts EXCEPT ![p] = @ + 1]
    /\ status' = Requeue(p)
    /\ UNCHANGED <<work, ins, crashes, cancelled, ran, owner>>

(* The same, from a live process's rounds: a crashed `argus work` leaves no job *)
(* stuck as running while the app goes on.                                      *)
Reap(p) ==
    /\ up[p] /\ \E j \in Jobs : status[j] = "running" /\ ~up[owner[j]]
    /\ status' = [j \in Jobs |-> IF status[j] = "running" /\ ~up[owner[j]] THEN "queued" ELSE status[j]]
    /\ UNCHANGED <<up, work, ins, starts, crashes, cancelled, ran, owner>>

Crash(p) ==
    /\ up[p] /\ crashes[p] < MaxCrashes
    /\ up' = [up EXCEPT ![p] = FALSE]
    /\ work' = [work EXCEPT ![p] = 0]
    /\ crashes' = [crashes EXCEPT ![p] = @ + 1]
    /\ UNCHANGED <<status, ins, starts, cancelled, ran, owner>>

(* enqueue_job: the dedupe check and the insert in one statement. *)
Check(c, j) ==
    /\ ins[c] = 0 /\ status[j] = "none" /\ \A d \in Callers : ins[d] # j
    /\ IF \E k \in Jobs : k # j /\ Key[k] = Key[j] /\ status[k] \in Active
       THEN status' = [status EXCEPT ![j] = "deduped"] /\ UNCHANGED ins
       ELSE status' = [status EXCEPT ![j] = "queued"] /\ UNCHANGED ins
    /\ UNCHANGED <<up, work, starts, crashes, cancelled, ran, owner>>

(* Unused while the check and insert are one statement: kept so a mutation can *)
(* split them again (enqueue_job's old SELECT, then INSERT).                  *)
Insert(c) ==
    /\ ins[c] # 0
    /\ status' = [status EXCEPT ![ins[c]] = "queued"]
    /\ ins' = [ins EXCEPT ![c] = 0]
    /\ UNCHANGED <<up, work, starts, crashes, cancelled, ran, owner>>

(* claim_with: the read and the conditional write in one transaction. *)
Claim(p, j) ==
    /\ up[p] /\ work[p] = 0 /\ status[j] = "queued"
    /\ status' = [status EXCEPT ![j] = "running"]
    /\ work' = [work EXCEPT ![p] = j]
    /\ ran' = [ran EXCEPT ![j] = IF Cardinality(Runners(j)) + 1 > @ THEN Cardinality(Runners(j)) + 1 ELSE @]
    /\ owner' = [owner EXCEPT ![j] = p]
    /\ UNCHANGED <<up, ins, starts, crashes, cancelled>>

(* settle: the job's work ended one way or another; the row changes only while running. *)
Settle(p, to) ==
    /\ up[p] /\ work[p] # 0
    /\ status' = [status EXCEPT ![work[p]] = IF @ = "running" THEN to ELSE @]
    /\ work' = [work EXCEPT ![p] = 0]
    /\ UNCHANGED <<up, ins, starts, crashes, cancelled, ran, owner>>

Succeed(p) == Settle(p, "done")
Fail(p) == Settle(p, "failed")
Asleep(p) == Settle(p, "queued")

Cancel(j) ==
    /\ status[j] \in Active
    /\ status' = [status EXCEPT ![j] = "cancelled"]
    /\ cancelled' = cancelled \cup {j}
    /\ UNCHANGED <<up, work, ins, starts, crashes, ran, owner>>

(* retry_failed_jobs: two UPDATEs, back to back; one step is enough here because *)
(* the second only touches rows the first left failed.                          *)
Retry ==
    /\ \E j \in Jobs : status[j] = "failed"
    /\ LET again(j) == /\ status[j] = "failed"
                       /\ \A k \in Jobs : Key[k] = Key[j] /\ status[k] = "failed" => k <= j
                       /\ ~\E k \in Jobs : Key[k] = Key[j] /\ status[k] \in Active
       IN status' = [j \in Jobs |-> IF again(j) THEN "queued"
                                    ELSE IF status[j] = "failed" THEN "cancelled" ELSE status[j]]
    /\ UNCHANGED <<up, work, ins, starts, crashes, cancelled, ran, owner>>

Next ==
    \/ \E p \in Procs : Start(p) \/ Reap(p) \/ Crash(p) \/ Succeed(p) \/ Fail(p) \/ Asleep(p)
    \/ \E p \in Procs, j \in Jobs : Claim(p, j)
    \/ \E c \in Callers, j \in Jobs : Check(c, j)
    \/ \E c \in Callers : Insert(c)
    \/ \E j \in Jobs : Cancel(j)
    \/ Retry
    \/ UNCHANGED vars

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
TypeOK ==
    /\ status \in [Jobs -> Status]
    /\ work \in [Procs -> {0} \cup Jobs]

(* No job runs in two processes at once (two culls of one Shoot, two exports). *)
NoDoubleRun == \A j \in Jobs : ran[j] <= 1

(* While a process runs a job, the Queue shows it running (or cancelled, to stop it). *)
RunnerSeesRunning == \A p \in Procs : work[p] # 0 => status[work[p]] \in {"running", "cancelled"}

(* At most one queued or running job per dedupe key. *)
OneActivePerKey == \A j, k \in Jobs :
    j # k /\ Key[j] = Key[k] /\ status[j] \in Active => status[k] \notin Active

(* A job the person cancelled stays cancelled. *)
CancelSticks == \A j \in cancelled : status[j] = "cancelled"

=============================================================================
