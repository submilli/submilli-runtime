------------------------- MODULE ResourceRecovery -------------------------
EXTENDS Naturals, Sequences, TLC
CONSTANTS MaxCrashes, Fault, Witness
ASSUME Fault \in {"none", "unsynced-pin", "restore-path", "restore-zero",
                   "fresh-replay", "early-collection"}
VARIABLE s
vars == <<s>>
Init == s = [up |-> TRUE, crashes |-> 0, pin |-> FALSE, diskPin |-> FALSE,
             opened |-> FALSE, path |-> "original", bytes |-> <<"a", "b">>,
             recorded |-> FALSE, cursor |-> 0, observed |-> "none",
             native |-> "none", offset |-> 0, candidate |-> "none",
             appended |-> FALSE, replaced |-> FALSE, replay |-> "none",
             terminal |-> FALSE, drained |-> FALSE, seen |-> {}]
Pin == /\ s.up /\ ~s.opened /\ ~s.pin
       /\ s' = [s EXCEPT !.pin = TRUE]
SyncPin == /\ s.up /\ s.pin /\ ~s.diskPin
           /\ s' = [s EXCEPT !.diskPin = TRUE]
RecordOpen == /\ s.up /\ ~s.opened /\ s.pin
              /\ (s.diskPin \/ Fault = "unsynced-pin")
              /\ s' = [s EXCEPT !.opened = TRUE, !.native = "original"]
Read == /\ s.up /\ s.opened /\ ~s.recorded /\ s.native = "original"
        /\ s.offset = 0 /\ s.candidate = "none"
        /\ s' = [s EXCEPT !.candidate = s.bytes[1], !.offset = 1]
RecordRead == /\ s.up /\ ~s.recorded /\ s.candidate # "none"
              /\ s' = [s EXCEPT !.recorded = TRUE, !.cursor = s.offset,
                                !.observed = s.candidate, !.candidate = "none"]
AppendOriginal == /\ s.up /\ s.recorded /\ ~s.appended
          /\ s' = [s EXCEPT !.bytes = Append(@, "c"), !.appended = TRUE]
Replace == /\ s.up /\ s.appended /\ ~s.replaced
           /\ s' = [s EXCEPT !.path = "newer", !.replaced = TRUE]
Crash == /\ s.up /\ s.crashes < MaxCrashes
         /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1,
                           !.pin = s.diskPin, !.native = "none", !.offset = 0,
                           !.candidate = "none", !.seen = @ \cup
                             (IF s.replaced THEN {"crashed-after-replace"} ELSE {})]
Restart == /\ ~s.up /\ s' = [s EXCEPT !.up = TRUE]
Restore == /\ s.up /\ s.opened /\ ~s.terminal /\ s.native = "none"
           /\ s.diskPin
           /\ s' = [s EXCEPT !.native = IF Fault = "restore-path" THEN s.path ELSE "original",
                             !.offset = IF Fault = "restore-zero" THEN 0 ELSE s.cursor,
                             !.seen = @ \cup (IF "crashed-after-replace" \in s.seen THEN {"restored"} ELSE {})]
ReplayRead == /\ s.up /\ s.recorded /\ s.replaced /\ ~s.terminal
              /\ s' = [s EXCEPT !.replay = IF Fault = "fresh-replay" THEN "newer-byte" ELSE s.observed,
                                !.seen = @ \cup (IF "restored" \in s.seen THEN {"replayed"} ELSE {})]
Terminal == /\ s.up /\ s.replaced /\ s.replay # "none"
            /\ s' = [s EXCEPT !.terminal = TRUE]
Drain == /\ s.up /\ s.terminal /\ ~s.drained
         /\ s' = [s EXCEPT !.native = "none", !.drained = TRUE]
Collect == /\ s.up /\ s.diskPin
           /\ ((s.terminal /\ s.drained) \/ Fault = "early-collection")
           /\ s' = [s EXCEPT !.pin = FALSE, !.diskPin = FALSE]
Next == Pin \/ SyncPin \/ RecordOpen \/ Read \/ RecordRead \/ AppendOriginal \/ Replace
        \/ Crash \/ Restart \/ Restore \/ ReplayRead \/ Terminal \/ Drain \/ Collect
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Pin) /\ WF_vars(SyncPin) /\ WF_vars(RecordOpen)
             /\ WF_vars(Read) /\ WF_vars(RecordRead) /\ WF_vars(AppendOriginal)
             /\ WF_vars(Replace) /\ WF_vars(Restart) /\ WF_vars(Restore)
             /\ WF_vars(ReplayRead) /\ WF_vars(Terminal) /\ WF_vars(Drain)
             /\ WF_vars(Collect)
TypeOK == /\ s.cursor \in 0..1 /\ s.offset \in 0..1
          /\ s.native \in {"none", "original", "newer"}
          /\ s.crashes \in 0..MaxCrashes
Retained == s.opened /\ ~(s.terminal /\ s.drained) => s.diskPin
SameResource == s.native # "none" => s.native = "original"
SameCursor == s.recorded /\ s.native # "none" => s.offset = s.cursor
RecordedRead == s.replay # "none" => s.replay = s.observed
Progress == <>(s.terminal /\ s.drained /\ ~s.diskPin)
NeverWitness == CASE Witness = "restore" -> ~({"crashed-after-replace", "restored", "replayed"} \subseteq s.seen)
                   [] OTHER -> ~(s.terminal /\ s.drained /\ ~s.diskPin)
=============================================================================
