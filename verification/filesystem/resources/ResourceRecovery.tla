------------------------- MODULE ResourceRecovery -------------------------
EXTENDS Naturals, Sequences, TLC
CONSTANTS MaxCrashes, MaxCancels, Fault, Witness
ASSUME Fault \in {"none", "no-checkpoint", "ignore-effect", "prefetched-offset",
                  "history-first", "finalizer-deletes", "mutable-pending",
                  "restore-path", "advance-early", "trust-native"}
VARIABLE s
vars == <<s>>
(* Three framework-assigned read IDs: two bytes and EOF. Open has already
   committed reader ID R. No native handle or read result is in the reader row. *)
Reads == 1..3
Before(i) == IF i = 1 THEN 0 ELSE IF i = 2 THEN 1 ELSE 2
After(i) == IF i < 3 THEN i ELSE 2
Value(offset, changed) == IF offset >= 2 THEN "EOF"
                         ELSE IF changed THEN "changed"
                         ELSE IF offset = 0 THEN "a" ELSE "b"
Expected(i) == Value(Before(i), FALSE)
Init == s = [up |-> TRUE, crashes |-> 0, cancels |-> 0, pc |-> 1, history |-> <<>>,
             row |-> TRUE, last |-> 0, before |-> 0, after |-> 0,
             native |-> FALSE, nativeFile |-> 0, localCursor |-> 0,
             physical |-> 0, phase |-> "idle", start |-> 0, result |-> "none",
             file |-> 1, changed |-> FALSE, blocked |-> FALSE,
             finalized |-> FALSE, released |-> FALSE, seen |-> {}]
Read == /\ s.up /\ ~s.released /\ ~s.blocked /\ s.row /\ s.pc \in Reads
        /\ s.phase = "idle"
        /\ (s.pc = Len(s.history) + 1 \/ Fault = "advance-early")
        /\ (s.file = 1 \/ Fault = "restore-path")
        /\ LET offset == IF Fault = "trust-native" /\ s.native THEN s.localCursor
                         ELSE IF s.last = s.pc
                         THEN IF Fault = "ignore-effect" THEN s.after ELSE s.before
                         ELSE s.after
               next == IF offset < 2 THEN offset + 1 ELSE offset
           IN s' = [s EXCEPT !.native = TRUE, !.nativeFile = s.file,
                             !.start = offset, !.localCursor = next,
                             !.physical = 2, !.result = Value(offset, s.changed),
                             !.phase = "read",
                             !.seen = @ \cup
                               (IF s.crashes > 0 /\ s.last = s.pc THEN {"retried-pending"} ELSE {}) \cup
                               (IF s.crashes > 0 /\ s.pc = 3 /\ Len(s.history) = 2 THEN {"continued-after-replay"} ELSE {})]
(* One SQLite commit stores ID + before/after progress, but no result bytes. *)
SaveProgress == /\ s.up /\ s.phase = "read"
                /\ s' = [s EXCEPT !.phase = "ready",
                                  !.last = IF Fault = "no-checkpoint" THEN s.last ELSE s.pc,
                                  !.before = IF Fault = "no-checkpoint" THEN s.before ELSE s.start,
                                  !.after = IF Fault = "no-checkpoint" THEN s.after
                                            ELSE IF Fault = "prefetched-offset" THEN s.physical ELSE s.localCursor]
(* Native work has drained, but its result/acknowledgement was abandoned.
   The next call retries the same effect with the still-open native handle. *)
AbortRead == /\ s.up /\ s.phase \in {"read", "ready"} /\ s.cancels < MaxCancels
             /\ s' = [s EXCEPT !.cancels = @ + 1, !.phase = "idle", !.result = "none",
                               !.seen = @ \cup
                                 (IF s.phase = "read" /\ s.last # s.pc THEN {"cancel-before-progress"} ELSE {}) \cup
                                 (IF s.phase = "ready" THEN {"cancel-after-progress"} ELSE {})]
(* Separate framework transaction: this is deliberately NOT SaveProgress. *)
CommitEffect == /\ s.up /\ s.pc = Len(s.history) + 1 /\ s.pc \in Reads
                /\ (s.phase = "ready" \/ (Fault = "history-first" /\ s.phase = "read"))
                /\ s' = [s EXCEPT !.history = Append(@, s.result), !.phase = "committed"]
Deliver == /\ s.up /\ s.phase = "committed"
           /\ s' = [s EXCEPT !.pc = @ + 1, !.phase = "idle", !.result = "none"]
Replay == /\ s.up /\ s.phase = "idle" /\ s.pc <= Len(s.history)
          /\ s' = [s EXCEPT !.pc = @ + 1, !.seen = @ \cup {"skipped-recorded-read"}]
AdvanceEarly == /\ Fault = "advance-early" /\ s.up /\ s.phase = "ready" /\ s.pc < 3
                /\ s' = [s EXCEPT !.pc = @ + 1, !.phase = "idle", !.result = "none"]
MutatePending == /\ Fault = "mutable-pending" /\ s.up /\ s.phase = "ready"
                 /\ s.pc < 3 /\ ~s.changed
                 /\ s' = [s EXCEPT !.changed = TRUE]
ReplacePath == /\ s.up /\ s.phase = "idle" /\ ~s.released /\ s.file = 1
               /\ s' = [s EXCEPT !.file = 2]
RefuseChangedFile == /\ s.up /\ s.phase = "idle" /\ s.pc = Len(s.history) + 1
                     /\ s.pc \in Reads /\ s.file # 1 /\ ~s.blocked
                     /\ s' = [s EXCEPT !.blocked = TRUE, !.native = FALSE,
                                       !.seen = @ \cup {"changed-file-refused"}]
(* Finalization can also happen on attempt teardown, without durable release.
   It drains native work before close; SQL lifetime is a separate decision. *)
Finalize == /\ s.up /\ s.native /\ s.phase # "read" /\ ~s.finalized
            /\ s' = [s EXCEPT !.native = FALSE, !.finalized = TRUE,
                              !.row = IF Fault = "finalizer-deletes" THEN FALSE ELSE s.row,
                              !.seen = @ \cup
                                (IF s.last > Len(s.history) THEN {"finalized-pending"} ELSE {})]
Release == /\ s.up /\ s.pc = 4 /\ Len(s.history) = 3 /\ ~s.released
           /\ s' = [s EXCEPT !.released = TRUE]
CloseNative == /\ s.up /\ s.released /\ s.native
               /\ s' = [s EXCEPT !.native = FALSE]
Collect == /\ s.up /\ s.released /\ ~s.native /\ s.row
           /\ s' = [s EXCEPT !.row = FALSE]
Crash == /\ s.up /\ s.crashes < MaxCrashes
         /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1, !.pc = 1,
                           !.native = FALSE, !.nativeFile = 0, !.localCursor = 0,
                           !.physical = 0, !.phase = "idle", !.result = "none",
                           !.seen = @ \cup
                             (IF s.last > Len(s.history) THEN {"progress-gap-crash"} ELSE {}) \cup
                             (IF Len(s.history) = 2 THEN {"two-reads-crash"} ELSE {})]
Restart == /\ ~s.up /\ s' = [s EXCEPT !.up = TRUE]
Core == Read \/ SaveProgress \/ AbortRead \/ CommitEffect \/ Deliver \/ Replay \/ Finalize
        \/ Release \/ CloseNative \/ Collect \/ Crash \/ Restart
Next == Core \/ AdvanceEarly \/ MutatePending \/ ReplacePath \/ RefuseChangedFile
Spec == Init /\ [][Next]_vars
(* Liveness requires a stable file; replacement is a deliberate refusal case. *)
LiveSpec == Init /\ [][Core]_vars /\ WF_vars(Read) /\ WF_vars(SaveProgress)
            /\ WF_vars(CommitEffect) /\ WF_vars(Deliver) /\ WF_vars(Replay)
            /\ WF_vars(Release) /\ WF_vars(CloseNative) /\ WF_vars(Collect) /\ WF_vars(Restart)
TypeOK == /\ s.pc \in 1..4 /\ Len(s.history) <= 3 /\ s.last \in 0..3
          /\ s.before \in 0..2 /\ s.after \in 0..2 /\ s.localCursor \in 0..2
          /\ s.phase \in {"idle", "read", "ready", "committed"}
          /\ s.crashes \in 0..MaxCrashes /\ s.cancels \in 0..MaxCancels
ReaderRetained == ~s.released => s.row
ProgressBeforeHistory == Len(s.history) <= s.last
OneUncommittedRead == s.last <= Len(s.history) + 1
LogicalProgress == s.last > 0 => s.before = Before(s.last) /\ s.after = After(s.last)
ReadPositionCorrect == s.phase # "idle" => s.start = Before(s.pc)
ReadResultStable == s.phase # "idle" => s.result = Expected(s.pc)
HistoryCorrect == \A i \in 1..Len(s.history): s.history[i] = Expected(i)
SameFile == s.native => s.nativeFile = 1
Progress == <>(s.released /\ ~s.native /\ ~s.row)
NeverWitness == CASE Witness = "gap" -> ~({"progress-gap-crash", "retried-pending"} \subseteq s.seen /\ s.released)
                   [] Witness = "skip" -> ~({"two-reads-crash", "skipped-recorded-read", "continued-after-replay"} \subseteq s.seen /\ s.released)
                   [] Witness = "finalize" -> ~({"finalized-pending", "retried-pending"} \subseteq s.seen /\ s.released)
                   [] Witness = "cancel-before" -> ~("cancel-before-progress" \in s.seen /\ s.released)
                   [] Witness = "cancel-after" -> ~("cancel-after-progress" \in s.seen /\ s.released)
                   [] Witness = "changed" -> "changed-file-refused" \notin s.seen
                   [] OTHER -> ~(s.released /\ ~s.native /\ ~s.row)
=============================================================================
