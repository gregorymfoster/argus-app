---------------------------- MODULE ExportRun ----------------------------
(***************************************************************************)
(* A resumable Export run (engine::library::Run), with Argus killed at     *)
(* any step: closing the lid, a quit or a crash. Checked with TLC by       *)
(* scripts/formal; the project skill .claude/skills/export-resume says     *)
(* how to read and update it.                                              *)
(*                                                                         *)
(* Each action is one indivisible step in the code:                        *)
(*   Top      tools/exports.rs export_photos: run_item + finished() check, *)
(*            finish_run_item for a file already whole (resume skip)       *)
(*   Reopen   export.rs claim_again: reuse the recorded name, opening each *)
(*            missing file (create(true) for a claimed row, create_new for *)
(*            an intent row; the same in a one-process model)              *)
(*   Intent   (slot.intent)(&path) = Library::intend_run_item: the name    *)
(*            about to be tried is recorded before create_new              *)
(*   Claim    export.rs claim: OpenOptions::create_new on one name         *)
(*   Undo     export.rs claim: remove the names this try claimed, n + 1    *)
(*   Record   (slot.claimed)(&paths[0]) = Library::claim_run_item          *)
(*   TmpOpen  export.rs put: File::create(tmp_of(path))                    *)
(*   TmpFull  export.rs put: write + sync_all                              *)
(*   Rename   export.rs put: std::fs::rename(tmp, path)                    *)
(*   Finish   tools/exports.rs: Library::finish_run_item                   *)
(*   End      jobs/resume.rs resume_export / end_run(id, false)            *)
(*   Stop     jobs/resume.rs: job cancelled -> Library::end_run(id, true)  *)
(*            -> export::clear_unfinished on each claimed, unfinished name *)
(*   Crash    the process dies (lid, quit, crash): local state is lost     *)
(*   Restart  jobs/resume.rs queue_orphaned_runs -> resume_export, which   *)
(*            calls export_photos for every Photo of the run, in order     *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    N,          \* Photos in the run
    E,          \* files per Photo: 1 for JPEG/HEIC/TIFF, 2 for Original (+ .xmp)
    S,          \* names tried per Photo: stem, stem-2, ... stem-S
    MaxCrashes, \* how many times Argus dies during the run
    Foreign     \* TRUE: a file the person already had sits at Photo 1's name

Photos == 1..N
Exts   == 1..E
Slots  == 1..S

(* What a name on disk holds. "empty" is a claimed name not written yet;   *)
(* "partial" a file half written; "foreign" a file Argus did not write    *)
(* (the person's own).                                                     *)
FileStates == {"absent", "empty", "partial", "whole", "foreign"}
TmpStates  == {"absent", "partial", "full"}

VARIABLES
    file,     \* file[p][s][e]: the name stem(-s).ext of Photo p
    tmp,      \* tmp[p][s][e]: its .name.argus-tmp
    item,     \* the run_items row of Photo p: slot 0 = no row; st none, intent, claimed, done
    run,      \* runs.status
    up,       \* whether the Argus process is alive
    pc,       \* where the process is in export_photos
    i,        \* the Photo it is on (resume_export's loop)
    slot,     \* the name claim / claim_again is on
    e,        \* the extension claim / put is on
    claimed,  \* the extensions this claim try created (claim's `claimed`)
    crashes

vars == <<file, tmp, item, run, up, pc, i, slot, e, claimed, crashes>>

Finished(f) == f # "absent" /\ f # "empty"   \* export::finished: len > 0

Init ==
    /\ file = [p \in Photos |-> [s \in Slots |-> [x \in Exts |->
                 IF Foreign /\ p = 1 /\ s = 1 /\ x = 1 THEN "foreign" ELSE "absent"]]]
    /\ tmp = [p \in Photos |-> [s \in Slots |-> [x \in Exts |-> "absent"]]]
    /\ item = [p \in Photos |-> [slot |-> 0, st |-> "none"]]
    /\ run = "running"
    /\ up = TRUE
    /\ pc = "top"
    /\ i = 1
    /\ slot = 0
    /\ e = 0
    /\ claimed = {}
    /\ crashes = 0

Local == <<pc, i, slot, e, claimed>>

(* export_photos for Photo i: skip it when its row is done, or claimed and *)
(* its files are whole; reuse a claimed name whatever its files hold (the  *)
(* row proves the name is this run's) and an intended name while its files *)
(* are missing or empty; otherwise claim a free name.                      *)
Top ==
    /\ up /\ run = "running" /\ pc = "top"
    /\ IF i > N
       THEN /\ pc' = "end"
            /\ UNCHANGED <<file, tmp, item, run, up, i, slot, e, claimed, crashes>>
       ELSE LET it    == item[i]
                whole == it.st = "claimed" /\ \A x \in Exts : Finished(file[i][it.slot][x])
                fits  == \/ it.st = "claimed"
                         \/ it.st = "intent" /\ \A x \in Exts : ~Finished(file[i][it.slot][x])
            IN IF it.st = "done" \/ whole
               THEN /\ item' = [item EXCEPT ![i].st = "done"]
                    /\ i' = i + 1
                    /\ UNCHANGED <<file, tmp, run, up, pc, slot, e, claimed, crashes>>
               ELSE IF fits
               THEN /\ pc' = "reopen" /\ slot' = it.slot /\ e' = 1
                    /\ UNCHANGED <<file, tmp, item, run, up, i, claimed, crashes>>
               ELSE /\ pc' = "intent" /\ slot' = 1 /\ e' = 1 /\ claimed' = {}
                    /\ UNCHANGED <<file, tmp, item, run, up, i, crashes>>

(* claim_again's reuse: open each missing file, keep what is there. *)
Reopen ==
    /\ up /\ pc = "reopen"
    /\ file' = [file EXCEPT ![i][slot][e] = IF @ = "absent" THEN "empty" ELSE @]
    /\ IF e = E THEN pc' = "record" /\ e' = 0 ELSE e' = e + 1 /\ UNCHANGED pc
    /\ UNCHANGED <<tmp, item, run, up, i, slot, claimed, crashes>>

(* claim: record the name about to be tried, before creating anything. *)
Intent ==
    /\ up /\ pc = "intent"
    /\ item' = [item EXCEPT ![i] = [slot |-> slot, st |-> "intent"]]
    /\ pc' = "claim"
    /\ UNCHANGED <<file, tmp, run, up, i, slot, e, claimed, crashes>>

(* claim: create_new on stem(-slot).ext; AlreadyExists moves to Undo. *)
Claim ==
    /\ up /\ pc = "claim"
    /\ IF file[i][slot][e] = "absent"
       THEN /\ file' = [file EXCEPT ![i][slot][e] = "empty"]
            /\ claimed' = claimed \cup {e}
            /\ IF e = E THEN pc' = "record" /\ e' = 0 ELSE e' = e + 1 /\ UNCHANGED pc
       ELSE /\ pc' = "undo"
            /\ UNCHANGED <<file, claimed, e>>
    /\ UNCHANGED <<tmp, item, run, up, i, slot, crashes>>

(* claim: remove what this try created, then try the next name. *)
Undo ==
    /\ up /\ pc = "undo"
    /\ file' = [file EXCEPT ![i][slot] = [x \in Exts |-> IF x \in claimed THEN "absent" ELSE @[x]]]
    /\ claimed' = {}
    /\ IF slot = S THEN pc' = "overflow" /\ UNCHANGED <<slot, e>>
                   ELSE pc' = "intent" /\ slot' = slot + 1 /\ e' = 1
    /\ UNCHANGED <<tmp, item, run, up, i, crashes>>

(* Library::claim_run_item: the row now says the name is claimed. *)
Record ==
    /\ up /\ pc = "record"
    /\ item' = [item EXCEPT ![i] = [slot |-> slot, st |-> "claimed"]]
    /\ pc' = "tmp" /\ e' = 1
    /\ UNCHANGED <<file, tmp, run, up, i, slot, claimed, crashes>>

(* put: File::create(tmp) truncates any temporary file left before. *)
TmpOpen ==
    /\ up /\ pc = "tmp"
    /\ tmp' = [tmp EXCEPT ![i][slot][e] = "partial"]
    /\ pc' = "write"
    /\ UNCHANGED file
    /\ UNCHANGED <<item, run, up, i, slot, e, claimed, crashes>>

(* put: write_all + sync_all. *)
TmpFull ==
    /\ up /\ pc = "write"
    /\ tmp' = [tmp EXCEPT ![i][slot][e] = "full"]
    /\ pc' = "rename"
    /\ UNCHANGED <<file, item, run, up, i, slot, e, claimed, crashes>>

(* put: rename(tmp, path), atomic on APFS. *)
Rename ==
    /\ up /\ pc = "rename"
    /\ file' = [file EXCEPT ![i][slot][e] = "whole"]
    /\ tmp' = [tmp EXCEPT ![i][slot][e] = "absent"]
    /\ IF e = E THEN pc' = "finish" /\ e' = 0 ELSE pc' = "tmp" /\ e' = e + 1
    /\ UNCHANGED <<item, run, up, i, slot, claimed, crashes>>

(* Library::finish_run_item. *)
Finish ==
    /\ up /\ pc = "finish"
    /\ item' = [item EXCEPT ![i] = [slot |-> slot, st |-> "done"]]
    /\ pc' = "top" /\ i' = i + 1 /\ slot' = 0
    /\ UNCHANGED <<file, tmp, run, up, e, claimed, crashes>>

(* Library::end_run(id, false). *)
End ==
    /\ up /\ pc = "end"
    /\ run' = "done"
    /\ pc' = "idle"
    /\ UNCHANGED <<file, tmp, item, up, i, slot, e, claimed, crashes>>

(* The person stops the run (the Queue checks between Photos):            *)
(* end_run(id, true) clears each claimed, unfinished name and its tmp.    *)
Unfinished(p) == item[p].st \in {"intent", "claimed"}

Stop ==
    /\ up /\ run = "running" /\ pc = "top"
    /\ file' = [p \in Photos |-> [s \in Slots |-> [x \in Exts |->
                 IF Unfinished(p) /\ s = item[p].slot /\ file[p][s][x] = "empty"
                 THEN "absent" ELSE file[p][s][x]]]]
    /\ tmp' = [p \in Photos |-> [s \in Slots |-> [x \in Exts |->
                 IF Unfinished(p) /\ s = item[p].slot THEN "absent" ELSE tmp[p][s][x]]]]
    /\ run' = "stopped"
    /\ pc' = "idle"
    /\ UNCHANGED <<item, up, i, slot, e, claimed, crashes>>

Crash ==
    /\ up /\ run = "running" /\ crashes < MaxCrashes
    /\ up' = FALSE
    /\ crashes' = crashes + 1
    /\ pc' = "down" /\ slot' = 0 /\ e' = 0 /\ claimed' = {}
    /\ UNCHANGED <<file, tmp, item, run, i>>

(* The next start: queue_orphaned_runs -> resume_export from the first Photo. *)
Restart ==
    /\ ~up /\ run = "running"
    /\ up' = TRUE
    /\ pc' = "top" /\ i' = 1
    /\ UNCHANGED <<file, tmp, item, run, slot, e, claimed, crashes>>

Ended == run \in {"done", "stopped"} /\ UNCHANGED vars

Next ==
    \/ Top \/ Reopen \/ Intent \/ Claim \/ Undo \/ Record
    \/ TmpOpen \/ TmpFull \/ Rename \/ Finish \/ End
    \/ Stop \/ Crash \/ Restart \/ Ended

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* The promises. Each one is a sentence a person would care about.         *)

TypeOK ==
    /\ file \in [Photos -> [Slots -> [Exts -> FileStates]]]
    /\ tmp \in [Photos -> [Slots -> [Exts -> TmpStates]]]
    /\ run \in {"running", "done", "stopped"}
    /\ crashes \in 0..MaxCrashes

(* The person's own file at an export's name is never written over. *)
ForeignKept == Foreign => file[1][1][1] = "foreign"

(* Each Photo is in the folder at most once: no DSC1.jpg and DSC1-2.jpg. *)
OneCopy == \A p \in Photos :
    Cardinality({s \in Slots : \E x \in Exts : file[p][s][x] = "whole"}) <= 1

(* No name ever holds a half-written file: a resumed or interrupted Export *)
(* finds each file whole under its name or not there yet.                  *)
NeverHalfWritten == \A p \in Photos, s \in Slots, x \in Exts : file[p][s][x] # "partial"

(* A Photo the run counts as done is whole on disk, every file of it. *)
DoneIsWhole == \A p \in Photos :
    item[p].st = "done" => \A x \in Exts : file[p][item[p].slot][x] = "whole"

(* A finished run exported every Photo. *)
AllExportedWhenDone == run = "done" => \A p \in Photos : item[p].st = "done"

(* A resumed Photo keeps the name an uninterrupted run gives it (never -2). *)
FirstFree(p) == IF Foreign /\ p = 1 THEN 2 ELSE 1
KeepsItsName == \A p \in Photos : item[p].st = "done" => item[p].slot = FirstFree(p)

NoTmp == \A p \in Photos, s \in Slots, x \in Exts : tmp[p][s][x] = "absent"
NoEmptyName == \A p \in Photos, s \in Slots, x \in Exts : file[p][s][x] # "empty"

(* A finished or stopped run leaves no temporary file behind. *)
NoTmpWhenDone == run = "done" => NoTmp
NoTmpWhenStopped == run = "stopped" => NoTmp

(* A finished or stopped run leaves no empty, claimed name behind. *)
NoEmptyNameWhenDone == run = "done" => NoEmptyName
NoEmptyNameWhenStopped == run = "stopped" => NoEmptyName

(* claim never ran out of the names the model gives it (S is big enough). *)
NamesSuffice == pc # "overflow"
=============================================================================
