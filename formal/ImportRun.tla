---------------------------- MODULE ImportRun ----------------------------
(***************************************************************************)
(* A resumable Import that copies originals into the Library              *)
(* (originals/<Shoot>/), killed at any step and resumed into the same     *)
(* Shoot and copy folder (library/import.rs, tools/library.rs resume_run). *)
(* Checked with TLC by scripts/formal; skill .claude/skills/import-resume. *)
(*                                                                         *)
(* Each action is one step in the code:                                    *)
(*   Seen     try_process: the file's hash is in `seen` (rows already     *)
(*            committed at the start, plus this run's): a Duplicate        *)
(*   Adopt    copy_verified: a file in the copy folder already holds this  *)
(*            content (a copy renamed in before the kill): reuse it        *)
(*   TmpCopy  copy_verified: write + sync_all + verify the hidden tmp      *)
(*   Rename   copy_verified: unique_name + rename(tmp, dest)               *)
(*   Insert   the main loop: INSERT the Photo row into the open            *)
(*            transaction (not on disk until the next Commit)              *)
(*   Commit   the main loop: commit every FLUSH_EVERY (500 ms), and at end *)
(*   Crash    the process dies: the open transaction is lost              *)
(*   Resume   the next start: clear_tmp(copy folder), seen = committed     *)
(*            hashes, every file of the card walked again                  *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    N,           \* files on the card (distinct contents)
    S,           \* names a file may get in the copy folder: name, name-2, ... name-S
    MaxCrashes

Files == 1..N
Slots == 1..S

VARIABLES
    copy,      \* copy[f][s]: the copy folder holds file f's content at name(-s)
    tmp,       \* tmp[f]: its hidden .argus-tmp is there ("none", "partial", "full")
    row,       \* row[f]: "none", "pending" (in the open transaction) or "saved" (committed), with its slot
    slotOf,    \* the slot each row points at
    up, pc, i, crashes, finished

vars == <<copy, tmp, row, slotOf, up, pc, i, crashes, finished>>

Init ==
    /\ copy = [f \in Files |-> [s \in Slots |-> FALSE]]
    /\ tmp = [f \in Files |-> "none"]
    /\ row = [f \in Files |-> "none"]
    /\ slotOf = [f \in Files |-> 0]
    /\ up = TRUE /\ pc = "next" /\ i = 1 /\ crashes = 0 /\ finished = FALSE

Taken(f, s) == copy[f][s]             \* only file f's names can clash: same camera name
Free(f) == CHOOSE s \in Slots : ~Taken(f, s) /\ \A t \in Slots : t < s => Taken(f, t)
Copied(f) == {s \in Slots : copy[f][s]}

(* Walk to file i: skip it when its hash is known, adopt a copy already there, or copy it. *)
Next1 ==
    /\ up /\ pc = "next" /\ ~finished
    /\ IF i > N THEN pc' = "end" /\ UNCHANGED <<copy, tmp, row, slotOf, i>>
       ELSE IF row[i] # "none" THEN i' = i + 1 /\ UNCHANGED <<copy, tmp, row, slotOf, pc>>
       ELSE IF Copied(i) # {}
       THEN /\ slotOf' = [slotOf EXCEPT ![i] = CHOOSE s \in Copied(i) : TRUE]
            /\ pc' = "insert"
            /\ UNCHANGED <<copy, tmp, row, i>>
       ELSE pc' = "tmp" /\ UNCHANGED <<copy, tmp, row, slotOf, i>>
    /\ UNCHANGED <<up, crashes, finished>>

TmpCopy ==
    /\ up /\ pc = "tmp"
    /\ tmp' = [tmp EXCEPT ![i] = "full"]
    /\ pc' = "rename"
    /\ UNCHANGED <<copy, row, slotOf, up, i, crashes, finished>>

Rename ==
    /\ up /\ pc = "rename"
    /\ IF \E s \in Slots : ~Taken(i, s)
       THEN /\ copy' = [copy EXCEPT ![i][Free(i)] = TRUE]
            /\ slotOf' = [slotOf EXCEPT ![i] = Free(i)]
            /\ pc' = "insert"
       ELSE pc' = "overflow" /\ UNCHANGED <<copy, slotOf>>
    /\ tmp' = [tmp EXCEPT ![i] = "none"]
    /\ UNCHANGED <<row, up, i, crashes, finished>>

Insert ==
    /\ up /\ pc = "insert"
    /\ row' = [row EXCEPT ![i] = "pending"]
    /\ pc' = "next" /\ i' = i + 1
    /\ UNCHANGED <<copy, tmp, slotOf, up, crashes, finished>>

(* The periodic commit (FLUSH_EVERY), any time between steps. *)
Commit ==
    /\ up /\ \E f \in Files : row[f] = "pending"
    /\ row' = [f \in Files |-> IF row[f] = "pending" THEN "saved" ELSE row[f]]
    /\ UNCHANGED <<copy, tmp, slotOf, up, pc, i, crashes, finished>>

End ==
    /\ up /\ pc = "end"
    /\ row' = [f \in Files |-> IF row[f] = "pending" THEN "saved" ELSE row[f]]
    /\ finished' = TRUE /\ pc' = "idle"
    /\ UNCHANGED <<copy, tmp, slotOf, up, i, crashes>>

Crash ==
    /\ up /\ ~finished /\ crashes < MaxCrashes
    /\ up' = FALSE /\ crashes' = crashes + 1 /\ pc' = "down"
    /\ row' = [f \in Files |-> IF row[f] = "pending" THEN "none" ELSE row[f]]
    /\ UNCHANGED <<copy, tmp, slotOf, i, finished>>

Resume ==
    /\ ~up /\ ~finished
    /\ up' = TRUE /\ pc' = "next" /\ i' = 1
    /\ tmp' = [f \in Files |-> "none"]
    /\ UNCHANGED <<copy, row, slotOf, crashes, finished>>

Done == finished /\ UNCHANGED vars

Next == Next1 \/ TmpCopy \/ Rename \/ Insert \/ Commit \/ End \/ Crash \/ Resume \/ Done

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
TypeOK == row \in [Files -> {"none", "pending", "saved"}]

(* Each original is copied into the Library once: no DSC1.ARW beside DSC1-2.ARW. *)
OneCopyEach == \A f \in Files : Cardinality(Copied(f)) <= 1

(* A saved Photo points at a copy that is there. *)
RowHasCopy == \A f \in Files : row[f] = "saved" => copy[f][slotOf[f]]

(* A finished Import brought in every file. *)
AllImportedWhenDone == finished => \A f \in Files : row[f] = "saved"

(* A finished Import leaves no copy the Library doesn't know (wasted disk). *)
NoStrayCopyWhenDone == finished => \A f \in Files, s \in Slots : copy[f][s] => slotOf[f] = s

(* A finished Import leaves no temporary file. *)
NoTmpWhenDone == finished => \A f \in Files : tmp[f] = "none"

NamesSuffice == pc # "overflow"
=============================================================================
