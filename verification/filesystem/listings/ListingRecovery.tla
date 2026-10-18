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
             cached |-> FALSE, localCursor |-> 0, state |-> "ReadAt",
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
             /\ s.state = "ReadAt"
             /\ LET offset == IF Fault = "trust-cache" /\ s.cached THEN s.localCursor
                              ELSE IF s.last = s.pc
                              THEN IF Fault = "ignore-effect" THEN s.after ELSE s.before
                              ELSE s.after
                    source == IF Fault = "fresh-enumeration" THEN s.directory ELSE s.snapshot
                    value == IF Fault = "sort-after-offset" THEN LateSortedItem(source, offset)
                             ELSE Item(source, offset)
                IN s' = [s EXCEPT !.cached = TRUE, !.start = offset,
                                  !.localCursor = IF offset < 3 THEN offset + 1 ELSE offset,
                                  !.result = value, !.state = "SaveProgress",
                                  !.seen = @ \cup
                                    (IF s.crashes > 0 /\ s.last = s.pc THEN {"retried-pending"} ELSE {}) \cup
                                    (IF s.crashes > 0 /\ Len(s.history) = 2 THEN {"continued-after-skip"} ELSE {}) \cup
                                    (IF s.directory # s.snapshot THEN {"used-frozen-snapshot"} ELSE {})]
SaveProgress == /\ s.up /\ s.state = "SaveProgress"
                /\ s' = [s EXCEPT !.state = "Completed",
                                  !.last = IF Fault = "no-checkpoint" THEN s.last ELSE s.pc,
                                  !.before = IF Fault = "no-checkpoint" THEN s.before ELSE s.start,
                                  !.after = IF Fault = "no-checkpoint" THEN s.after ELSE s.localCursor]
CommitNext == /\ s.up /\ s.pc = Len(s.history) + 1
              /\ (s.state = "Completed" \/ (Fault = "history-first" /\ s.state = "SaveProgress"))
              /\ s' = [s EXCEPT !.history = Append(@, s.result), !.state = "HistoryCommitted"]
Deliver == /\ s.up /\ s.state = "HistoryCommitted"
           /\ s' = [s EXCEPT !.state = "ReadAt", !.pc = @ + 1, !.result = "none"]
Replay == /\ s.up /\ s.state = "ReadAt" /\ s.pc <= Len(s.history)
          /\ s' = [s EXCEPT !.pc = @ + 1]
CancelNext == /\ s.up /\ s.state \in {"SaveProgress", "Completed"} /\ s.cancels < MaxCancels
              /\ s' = [s EXCEPT !.cancels = @ + 1, !.state = "ReadAt", !.result = "none",
                                !.seen = @ \cup {IF s.state = "SaveProgress" THEN "cancel-before" ELSE "cancel-after"}]
Finalize == /\ s.up /\ s.cached /\ s.state # "SaveProgress" /\ ~s.finalized
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
                           !.localCursor = 0, !.state = "ReadAt", !.result = "none",
                           !.seen = @ \cup
                             (IF s.snapshot # "none" /\ ~s.openHistory THEN {"snapshot-gap"} ELSE {}) \cup
                             (IF s.last > Len(s.history) THEN {"progress-gap"} ELSE {})]
Restart == /\ ~s.up /\ s' = [s EXCEPT !.up = TRUE]
OpenDirectoryState == IF s.snapshot = "none" THEN "Initial" ELSE "Completed"
OpenDirectoryLoop == CASE OpenDirectoryState = "Initial" -> Capture \/ CommitSnapshot
                         [] OTHER -> FALSE
(* NextListingState holds a volatile item, while SQLite retains last/before/after.
   A retry decodes ReadAt and obtains the same item from the frozen snapshot. *)
NextListingLoop == CASE s.state = "ReadAt" -> NextEntry
                       [] s.state = "SaveProgress" -> SaveProgress
                       [] OTHER -> FALSE
FrameworkStep == CommitOpen \/ CommitNext \/ Deliver \/ Replay
Next == OpenDirectoryLoop \/ NextListingLoop \/ FrameworkStep \/ ChangeDirectory
        \/ CancelNext \/ Finalize \/ Release \/ DropCache \/ Collect \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Capture) /\ WF_vars(CommitSnapshot) /\ WF_vars(CommitOpen)
            /\ WF_vars(NextEntry) /\ WF_vars(SaveProgress) /\ WF_vars(CommitNext)
            /\ WF_vars(Deliver) /\ WF_vars(Replay) /\ WF_vars(Release)
            /\ WF_vars(DropCache) /\ WF_vars(Collect) /\ WF_vars(Restart)
TypeOK == /\ s.pc \in 1..5 /\ Len(s.history) <= 4 /\ s.last \in 0..4
          /\ s.before \in 0..3 /\ s.after \in 0..3 /\ s.localCursor \in 0..3
          /\ s.crashes \in 0..MaxCrashes /\ s.cancels \in 0..MaxCancels
          /\ s.state \in {"ReadAt", "SaveProgress", "Completed", "HistoryCommitted"}
SnapshotBeforeOpen == s.openHistory => s.snapshot # "none"
Retained == ~s.released => s.row
ProgressBeforeHistory == Len(s.history) <= s.last
LogicalProgress == s.last > 0 => s.before = Before(s.last) /\ s.after = After(s.last)
ReadPositionCorrect == s.state # "ReadAt" => s.start = Before(s.pc)
FrozenOrder == s.state # "ReadAt" => s.result = Item(s.snapshot, Before(s.pc))
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
