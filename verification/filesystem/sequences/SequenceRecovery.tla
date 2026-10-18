------------------------- MODULE SequenceRecovery -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS MaxCrashes, Fault, Witness
ASSUME Fault \in {"none", "skip-receipt", "repeat-prefix", "early-cleanup"}
VARIABLE s
vars == <<s>>
\* The program calls JOURNALED operations 1 and 2 in order; there is no plan.
\* Plain byte writes have no local receipts; see writes/WriteRecovery.tla.
Steps == 1..2
Init == s = [up |-> TRUE, crashes |-> 0, cursor |-> 1,
             intent |-> 0, applied |-> {}, persisted |-> {}, receipts |-> {},
             refused |-> {}, outer |-> FALSE, journalRetained |-> TRUE,
             damaged |-> FALSE, seen |-> {}]
Prepare == /\ s.up /\ s.cursor \in Steps /\ s.intent = 0
           /\ s.cursor \notin (s.receipts \cup s.refused)
           /\ s' = [s EXCEPT !.intent = s.cursor]
\* Exclusive-owner primitive retry is supplied by the primitive models.
Apply == /\ s.up /\ s.intent = s.cursor /\ s.intent \in Steps /\ s.journalRetained
         /\ s' = [s EXCEPT !.applied = @ \cup {s.intent}]
Sync == /\ s.up /\ s.intent \in s.applied
        /\ s' = [s EXCEPT !.persisted = @ \cup {s.intent}]
Receipt == /\ s.up /\ s.intent \in Steps
           /\ (s.intent \in s.persisted \/ Fault = "skip-receipt")
           /\ s' = [s EXCEPT !.receipts = @ \cup {s.intent}, !.intent = 0]
RefuseSecond == /\ s.up /\ s.cursor = 2 /\ s.intent = 0
                /\ 2 \notin (s.receipts \cup s.refused)
                /\ s' = [s EXCEPT !.refused = {2}]
Return == /\ s.up /\ s.cursor \in (s.receipts \cup s.refused)
          /\ s' = [s EXCEPT !.cursor = @ + 1,
                            !.damaged = s.damaged \/ (Fault = "repeat-prefix" /\ s.cursor \in s.receipts /\ s.crashes > 0),
                            !.seen = @ \cup (IF s.crashes > 0 THEN {"replay"} ELSE {})]
\* A future caller may persist its own completion separately. This is outside
\* the filesystem; a crash before Finish must still reuse filesystem receipts.
Finish == /\ s.up /\ s.cursor = 3 /\ ~s.outer
          /\ s' = [s EXCEPT !.outer = TRUE]
Crash == /\ s.up /\ s.crashes < MaxCrashes
         /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1, !.cursor = 1,
                           !.applied = s.persisted,
                           !.seen = @ \cup
                             (IF s.receipts = {1} THEN {"prefix-crash"} ELSE {}) \cup
                             (IF s.receipts = Steps /\ ~s.outer THEN {"completion-gap"} ELSE {}) \cup
                             (IF s.refused = {2} THEN {"failure-crash"} ELSE {})]
Restart == /\ ~s.up /\ s' = [s EXCEPT !.up = TRUE]
\* Journal lifetime is a caller precondition, not a cleanup service here.
BadCleanup == /\ Fault = "early-cleanup" /\ s.journalRetained
              /\ s' = [s EXCEPT !.journalRetained = FALSE]
(* The program retries prior completed calls before reaching a later pending
   call. Its cursor is not a filesystem plan. Sync/receipt can be in flight for
   that later identity while the caller replays the completed prefix. *)
DecodeState == CASE s.cursor \in (s.receipts \cup s.refused) -> "Completed"
                 [] s.intent = s.cursor /\ s.intent \in Steps -> "Prepared"
                 [] OTHER -> "Initial"
CallLoop == CASE DecodeState = "Initial" -> Prepare \/ RefuseSecond
                [] DecodeState = "Prepared" -> Apply
                [] DecodeState = "Completed" -> Return
                [] OTHER -> FALSE
Next == CallLoop \/ Sync \/ Receipt \/ Finish \/ Crash \/ Restart \/ BadCleanup
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Prepare) /\ WF_vars(Apply) /\ WF_vars(Sync)
             /\ WF_vars(Receipt) /\ WF_vars(Return) /\ WF_vars(Finish)
             /\ WF_vars(Restart)
TypeOK == /\ s.intent \in 0..2 /\ s.cursor \in 1..3
          /\ s.receipts \subseteq Steps /\ s.persisted \subseteq Steps
          /\ s.applied \subseteq Steps /\ s.refused \subseteq {2}
          /\ s.crashes \in 0..MaxCrashes
DurablePrefix == /\ s.receipts \subseteq s.persisted
                 /\ (2 \in (s.receipts \cup s.refused) => 1 \in s.receipts)
                 /\ s.receipts \cap s.refused = {}
JournalRetained == s.journalRetained
Completed == s.outer => s.receipts \cup s.refused = Steps
NoRepeat == ~s.damaged
Progress == <>s.outer
NeverWitness == CASE Witness = "prefix" -> ~("prefix-crash" \in s.seen /\ "replay" \in s.seen /\ s.outer)
                   [] Witness = "gap" -> ~("completion-gap" \in s.seen /\ "replay" \in s.seen /\ s.outer)
                   [] OTHER -> ~("failure-crash" \in s.seen /\ "replay" \in s.seen /\ s.outer)
=============================================================================
