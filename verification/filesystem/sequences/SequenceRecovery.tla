------------------------- MODULE SequenceRecovery -------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
CONSTANTS MaxCrashes, Fault, Witness, Completion
ASSUME Fault \in {"none", "fresh-plan", "skip-receipt", "repeat-prefix", "early-cleanup"}
VARIABLE s
vars == <<s>>
Steps == 1..2
Init == s = [up |-> TRUE, crashes |-> 0, plan |-> <<>>, original |-> <<1,2>>,
             intent |-> 0, applied |-> {}, persisted |-> {}, receipts |-> {},
             result |-> "pending", pinned |-> TRUE, reused |-> FALSE,
             damaged |-> FALSE, seen |-> {}]
Admit == /\ s.up /\ s.plan = <<>>
         /\ s' = [s EXCEPT !.plan = s.original]
Prepare(i) == /\ s.up /\ s.plan # <<>> /\ s.result = "pending"
              /\ s.intent = 0 /\ i \notin s.receipts
              /\ \A j \in Steps : j < i => j \in s.receipts
              /\ s' = [s EXCEPT !.intent = i]
Apply == /\ s.up /\ s.intent \in Steps /\ s.pinned
         /\ s' = [s EXCEPT !.applied = @ \cup {s.intent}]
Sync == /\ s.up /\ s.intent \in s.applied
        /\ s' = [s EXCEPT !.persisted = @ \cup {s.intent}]
Receipt == /\ s.up /\ s.intent \in Steps
           /\ (s.intent \in s.persisted \/ Fault = "skip-receipt")
           /\ s' = [s EXCEPT !.receipts = @ \cup {s.intent}, !.intent = 0,
                             !.seen = @ \cup {"step-receipt"}]
Finish == /\ s.up /\ s.receipts = Steps /\ s.result = "pending"
          /\ s' = [s EXCEPT !.result = "done"]
FailSecond == /\ Completion = "prefix" /\ s.up /\ s.receipts = {1} /\ s.intent = 0 /\ s.result = "pending"
              /\ s' = [s EXCEPT !.result = "failed"]
Reuse == /\ s.up /\ s.result # "pending" /\ ~s.reused
         /\ s' = [s EXCEPT !.reused = TRUE]
Replay == /\ s.up /\ s.reused
          /\ s' = [s EXCEPT !.damaged = Fault = "repeat-prefix",
                            !.seen = @ \cup {"replay"}]
Crash == /\ s.up /\ s.crashes < MaxCrashes
         /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1,
                           !.applied = s.persisted,
                           !.seen = @ \cup (IF s.receipts = {1} /\ s.result = "pending" THEN {"prefix-crash"} ELSE {})]
Restart == /\ ~s.up
           /\ s' = [s EXCEPT !.up = TRUE,
                             !.plan = IF Fault = "fresh-plan" /\ s.plan # <<>> THEN <<2,1>> ELSE s.plan,
                             !.seen = @ \cup (IF "prefix-crash" \in s.seen THEN {"prefix-restarted"} ELSE {})]
Collect == /\ s.up /\ s.pinned
           /\ ((s.result # "pending" /\ "replay" \in s.seen) \/ Fault = "early-cleanup")
           /\ s' = [s EXCEPT !.pinned = FALSE]
Next == Admit \/ (\E i \in Steps: Prepare(i)) \/ Apply \/ Sync \/ Receipt
        \/ Finish \/ FailSecond \/ Reuse \/ Replay \/ Crash \/ Restart \/ Collect
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Admit) /\ (\A i \in Steps: WF_vars(Prepare(i)))
             /\ WF_vars(Apply) /\ WF_vars(Sync) /\ WF_vars(Receipt)
             /\ WF_vars(Finish) /\ WF_vars(Reuse) /\ WF_vars(Replay)
             /\ WF_vars(Restart) /\ WF_vars(Collect)
TypeOK == /\ s.intent \in 0..2 /\ s.receipts \subseteq Steps /\ s.persisted \subseteq Steps
          /\ s.applied \subseteq Steps /\ s.crashes \in 0..MaxCrashes
ImmutablePlan == s.plan \in {<<>>, s.original}
DurablePrefix == s.receipts \subseteq s.persisted /\ (2 \in s.receipts => 1 \in s.receipts)
Evidence == s.result = "pending" => s.pinned
Completed == s.result = "done" => s.receipts = Steps
NoRepeat == ~s.damaged
Progress == <>(s.result # "pending" /\ ~s.pinned)
NeverWitness == CASE Witness = "prefix" -> ~("prefix-restarted" \in s.seen /\ s.result = "done")
                   [] OTHER -> ~(s.result = "failed" /\ "replay" \in s.seen)
=============================================================================
