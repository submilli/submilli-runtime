--------------------------- MODULE WriterRecovery ---------------------------
EXTENDS Naturals, Sequences, TLC
CONSTANTS MaxCrashes, Fault, Witness
ASSUME Fault \in {"none", "fresh-offset", "early-receipt", "unchecked-input", "repeat-close"}
VARIABLE s
vars == <<s>>
(* Prior execution outputs reconstruct these bytes. Fingerprints abstract a
   collision-free digest; SQL contains fingerprints and outcomes, not bytes.
   The staged file is the writer's output-in-progress, not an input archive. *)
Payload == <<1, 2>>
Fingerprint(byte) == IF byte = 1 THEN "hash-one" ELSE IF byte = 2 THEN "hash-two" ELSE "wrong"
Init == s = [up |-> TRUE, crashes |-> 0, receipts |-> <<>>, cache |-> <<>>, disk |-> <<>>,
             cursor |-> 0, input |-> 0, written |-> FALSE, synced |-> FALSE,
             closed |-> FALSE, target |-> <<>>, replaced |-> FALSE, returned |-> FALSE,
             badInput |-> FALSE, seen |-> {}]
Supply == /\ s.up /\ s.cursor < 2 /\ s.input = 0
          /\ s' = [s EXCEPT !.input = Payload[s.cursor + 1]]
WrongReplay == /\ s.up /\ s.cursor < Len(s.receipts) /\ s.input = 0
               /\ s' = [s EXCEPT !.input = 9]
ReplayChunk == /\ s.up /\ s.cursor < Len(s.receipts) /\ s.input # 0
               /\ (Fingerprint(s.input) = s.receipts[s.cursor + 1] \/ Fault = "unchecked-input")
               /\ s' = [s EXCEPT !.cursor = @ + 1, !.input = 0,
                                 !.badInput = @ \/ Fingerprint(s.input) # s.receipts[s.cursor + 1]]
Reject == /\ s.up /\ s.cursor < Len(s.receipts) /\ s.input # 0
          /\ Fingerprint(s.input) # s.receipts[s.cursor + 1]
          /\ s' = [s EXCEPT !.input = 0, !.seen = @ \cup {"mismatch-refused"}]
Stage == /\ s.up /\ ~s.closed /\ s.cursor = Len(s.receipts) /\ s.cursor < 2
         /\ s.input = Payload[s.cursor + 1] /\ ~s.written /\ Len(s.cache) < 4
         /\ s' = [s EXCEPT !.cache = Append(IF Fault = "fresh-offset" THEN s.cache ELSE SubSeq(s.cache, 1, s.cursor), s.input),
                           !.written = TRUE, !.synced = FALSE]
Sync == /\ s.up /\ s.written /\ ~s.synced
        /\ s' = [s EXCEPT !.disk = s.cache, !.synced = TRUE]
Receipt == /\ s.up /\ s.written /\ (s.synced \/ Fault = "early-receipt")
           /\ s' = [s EXCEPT !.receipts = Append(@, Fingerprint(s.input)), !.cursor = @ + 1,
                             !.input = 0, !.written = FALSE, !.synced = FALSE]
(* Publication abstracts the separately checked namespace protocol: only its
   synced result may back close. Process death before its receipt is idempotent. *)
Close == /\ s.up /\ s.cursor = 2 /\ Len(s.receipts) = 2 /\ ~s.closed
         /\ s.disk = Payload
         /\ s' = [s EXCEPT !.target = Payload, !.closed = TRUE]
Reuse == /\ s.up /\ s.closed /\ ~s.replaced
         /\ s' = [s EXCEPT !.target = <<9>>, !.replaced = TRUE]
ReplayClose == /\ s.up /\ s.cursor = 2 /\ s.closed /\ s.replaced
               /\ s' = [s EXCEPT !.target = IF Fault = "repeat-close" THEN Payload ELSE s.target,
                                 !.returned = TRUE]
Crash == /\ s.up /\ s.crashes < MaxCrashes
         /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1, !.cache = s.disk,
                           !.cursor = 0, !.input = 0, !.written = FALSE, !.synced = FALSE,
                           !.seen = @ \cup (IF Len(s.disk) > Len(s.receipts) THEN {"unacknowledged-tail"} ELSE {})]
Restart == /\ ~s.up /\ s' = [s EXCEPT !.up = TRUE]
Next == Supply \/ WrongReplay \/ ReplayChunk \/ Reject \/ Stage \/ Sync \/ Receipt \/ Close \/ Reuse \/ ReplayClose \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars
(* Correct replay is eventually supplied. Repeated incompatible attempts do not
   establish progress; they must remain refused without changing the output. *)
LiveNext == Supply \/ ReplayChunk \/ Stage \/ Sync \/ Receipt \/ Close \/ Reuse \/ ReplayClose \/ Crash \/ Restart
LiveSpec == Init /\ [][LiveNext]_vars /\ WF_vars(Supply) /\ WF_vars(ReplayChunk)
            /\ WF_vars(Stage) /\ WF_vars(Sync) /\ WF_vars(Receipt) /\ WF_vars(Close)
            /\ WF_vars(Reuse) /\ WF_vars(ReplayClose) /\ WF_vars(Restart)
TypeOK == /\ s.cursor \in 0..2 /\ Len(s.receipts) <= 2 /\ Len(s.cache) <= 4
          /\ s.crashes \in 0..MaxCrashes /\ s.input \in {0,1,2,9}
DurableChunks == \A i \in 1..Len(s.receipts) : i <= Len(s.disk) /\ Fingerprint(s.disk[i]) = s.receipts[i]
SameBytes == ~s.badInput
NoDuplicateTail == Len(s.disk) <= Len(s.receipts) + 1 /\ Len(s.disk) <= 2
NoRepublish == s.replaced => s.target = <<9>>
Progress == <>s.returned
NeverWitness == CASE Witness = "tail" -> ~("unacknowledged-tail" \in s.seen /\ s.returned)
                   [] OTHER -> "mismatch-refused" \notin s.seen
=============================================================================
