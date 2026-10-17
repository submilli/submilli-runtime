------------------------- MODULE ListingRecovery -------------------------
EXTENDS Naturals, Sequences, TLC
CONSTANTS MaxCrashes, MaxCancels, Fault, Witness
ASSUME Fault \in {"none", "open-before-snapshot", "no-checkpoint", "ignore-effect",
                  "trust-cache", "fresh-enumeration", "sort-after-offset",
                  "history-first", "finalizer-deletes"}
VARIABLE s
vars == <<s>>
(* Raw pathname order differs from the component-wise order exposed by next. *)
Raw(which) == IF which = "old" THEN <<"a", "a-", "a/z">> ELSE <<"b", "b-", "b/z">>
Ordered(which) == IF which = "old" THEN <<"a", "a/z", "a-">> ELSE <<"b", "b/z", "b-">>
Before(i) == IF i <= 3 THEN i - 1 ELSE 3
After(i) == IF i <= 3 THEN i ELSE 3
Item(which, offset) == IF offset < 3 THEN Ordered(which)[offset + 1] ELSE "EOF"
(* Sorting a suffix of raw pathname order is NOT seeking in the full ordering. *)
LateSortedItem(which, offset) == IF offset = 0 THEN Item(which, 0)
                               ELSE IF offset = 1 THEN Raw(which)[3]
                               ELSE IF offset = 2 THEN Raw(which)[3] ELSE "EOF"
Init == s = [up |-> TRUE, crashes |-> 0, cancels |-> 0,
             row |-> TRUE, directory |-> "old", capture |-> "none", snapshot |-> "none",
             openHistory |-> FALSE, pc |-> 1, history |-> <<>>,
             last |-> 0, before |-> 0, after |-> 0,
             cached |-> FALSE, localCursor |-> 0, phase |-> "idle",
             start |-> 0, result |-> "none", finalized |-> FALSE,
             released |-> FALSE, seen |-> {}]
Capture == /\ s.up /\ s.row /\ s.snapshot = "none" /\ s.capture = "none"
           /\ s' = [s EXCEPT !.capture = s.directory]
CommitSnapshot == /\ s.up /\ s.snapshot = "none" /\ s.capture # "none"
                  /\ s' = [s EXCEPT !.snapshot = s.capture, !.capture = "none"]
CommitOpen == /\ s.up /\ ~s.openHistory
              /\ (s.snapshot # "none" \/ Fault = "open-before-snapshot")
              /\ s' = [s EXCEPT !.openHistory = TRUE]
ChangeDirectory == /\ s.up /\ s.snapshot # "none" /\ s.directory = "old"
                   /\ s' = [s EXCEPT !.directory = "new"]
NextEntry == /\ s.up /\ s.row /\ s.openHistory /\ ~s.released
             /\ s.snapshot # "none" /\ s.pc \in 1..4 /\ s.pc = Len(s.history) + 1
             /\ s.phase = "idle"
             /\ LET offset == IF Fault = "trust-cache" /\ s.cached THEN s.localCursor
                              ELSE IF s.last = s.pc
                              THEN IF Fault = "ignore-effect" THEN s.after ELSE s.before
                              ELSE s.after
                    source == IF Fault = "fresh-enumeration" THEN s.directory ELSE s.snapshot
                    value == IF Fault = "sort-after-offset" THEN LateSortedItem(source, offset)
                             ELSE Item(source, offset)
                IN s' = [s EXCEPT !.cached = TRUE, !.start = offset,
                                  !.localCursor = IF offset < 3 THEN offset + 1 ELSE offset,
                                  !.result = value, !.phase = "read",
                                  !.seen = @ \cup
                                    (IF s.crashes > 0 /\ s.last = s.pc THEN {"retried-pending"} ELSE {}) \cup
                                    (IF s.crashes > 0 /\ Len(s.history) = 2 THEN {"continued-after-skip"} ELSE {}) \cup
                                    (IF s.directory # s.snapshot THEN {"used-frozen-snapshot"} ELSE {})]
SaveProgress == /\ s.up /\ s.phase = "read"
                /\ s' = [s EXCEPT !.phase = "ready",
                                  !.last = IF Fault = "no-checkpoint" THEN s.last ELSE s.pc,
                                  !.before = IF Fault = "no-checkpoint" THEN s.before ELSE s.start,
                                  !.after = IF Fault = "no-checkpoint" THEN s.after ELSE s.localCursor]
CommitNext == /\ s.up /\ s.pc = Len(s.history) + 1
              /\ (s.phase = "ready" \/ (Fault = "history-first" /\ s.phase = "read"))
              /\ s' = [s EXCEPT !.history = Append(@, s.result), !.phase = "committed"]
Deliver == /\ s.up /\ s.phase = "committed"
           /\ s' = [s EXCEPT !.phase = "idle", !.pc = @ + 1, !.result = "none"]
Replay == /\ s.up /\ s.phase = "idle" /\ s.pc <= Len(s.history)
          /\ s' = [s EXCEPT !.pc = @ + 1]
CancelNext == /\ s.up /\ s.phase \in {"read", "ready"} /\ s.cancels < MaxCancels
              /\ s' = [s EXCEPT !.cancels = @ + 1, !.phase = "idle", !.result = "none",
                                !.seen = @ \cup {IF s.phase = "read" THEN "cancel-before" ELSE "cancel-after"}]
Finalize == /\ s.up /\ s.cached /\ s.phase # "read" /\ ~s.finalized
            /\ s' = [s EXCEPT !.cached = FALSE, !.finalized = TRUE,
                              !.row = IF Fault = "finalizer-deletes" THEN FALSE ELSE s.row,
                              !.seen = @ \cup {"finalized"}]
Release == /\ s.up /\ s.pc = 5 /\ Len(s.history) = 4 /\ ~s.released
           /\ s' = [s EXCEPT !.released = TRUE]
DropCache == /\ s.up /\ s.released /\ s.cached
             /\ s' = [s EXCEPT !.cached = FALSE]
Collect == /\ s.up /\ s.released /\ ~s.cached /\ s.row
           /\ s' = [s EXCEPT !.row = FALSE]
Crash == /\ s.up /\ s.crashes < MaxCrashes
         /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1,
                           !.pc = 1, !.capture = "none", !.cached = FALSE,
                           !.localCursor = 0, !.phase = "idle", !.result = "none",
                           !.seen = @ \cup
                             (IF s.snapshot # "none" /\ ~s.openHistory THEN {"snapshot-gap"} ELSE {}) \cup
                             (IF s.last > Len(s.history) THEN {"progress-gap"} ELSE {})]
Restart == /\ ~s.up /\ s' = [s EXCEPT !.up = TRUE]
Next == Capture \/ CommitSnapshot \/ CommitOpen \/ ChangeDirectory \/ NextEntry
        \/ SaveProgress \/ CommitNext \/ Deliver \/ Replay \/ CancelNext \/ Finalize
        \/ Release \/ DropCache \/ Collect \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Capture) /\ WF_vars(CommitSnapshot) /\ WF_vars(CommitOpen)
            /\ WF_vars(NextEntry) /\ WF_vars(SaveProgress) /\ WF_vars(CommitNext)
            /\ WF_vars(Deliver) /\ WF_vars(Replay) /\ WF_vars(Release)
            /\ WF_vars(DropCache) /\ WF_vars(Collect) /\ WF_vars(Restart)
TypeOK == /\ s.pc \in 1..5 /\ Len(s.history) <= 4 /\ s.last \in 0..4
          /\ s.before \in 0..3 /\ s.after \in 0..3 /\ s.localCursor \in 0..3
          /\ s.crashes \in 0..MaxCrashes /\ s.cancels \in 0..MaxCancels
          /\ s.phase \in {"idle", "read", "ready", "committed"}
SnapshotBeforeOpen == s.openHistory => s.snapshot # "none"
Retained == ~s.released => s.row
ProgressBeforeHistory == Len(s.history) <= s.last
LogicalProgress == s.last > 0 => s.before = Before(s.last) /\ s.after = After(s.last)
ReadPositionCorrect == s.phase # "idle" => s.start = Before(s.pc)
FrozenOrder == s.phase # "idle" => s.result = Item(s.snapshot, Before(s.pc))
HistoryCorrect == \A i \in 1..Len(s.history): s.history[i] = Item(s.snapshot, Before(i))
Progress == <>(s.released /\ ~s.cached /\ ~s.row)
NeverWitness == CASE Witness = "open" -> ~("snapshot-gap" \in s.seen /\ s.released)
                   [] Witness = "gap" -> ~({"progress-gap", "retried-pending"} \subseteq s.seen /\ s.released)
                   [] Witness = "skip" -> ~("continued-after-skip" \in s.seen /\ s.released)
                   [] Witness = "changed" -> ~("used-frozen-snapshot" \in s.seen /\ s.released)
                   [] Witness = "cancel-before" -> ~("cancel-before" \in s.seen /\ s.released)
                   [] Witness = "cancel-after" -> ~("cancel-after" \in s.seen /\ s.released)
                   [] OTHER -> ~("finalized" \in s.seen /\ s.released /\ ~s.row)
=============================================================================
