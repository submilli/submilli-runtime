-------------------------- MODULE UpdateRecovery --------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
CONSTANTS MaxCrashes, Fault, Witness, Kind

(* Composition of native preparation, retained replacement, filesystem receipt,
   and independent execution history. File/directory sync and SQL commits are
   separate actions. The sole owner drains cancelled work before retry. *)
Original == <<1, 2>>
Later == <<9>>
Transform(bytes) == CASE Kind = "insert" -> Append(bytes, 3)
                     [] Kind \in {"edit", "patch"} ->
                          IF bytes = Original THEN <<4, 2>> ELSE bytes
                     [] OTHER -> bytes
Result(bytes) == <<bytes, Transform(bytes), Kind>>
Expected == Transform(Original)
ExpectedResult == Result(Original)
Changes == Kind \in {"insert", "edit", "patch"}

VARIABLE s
vars == <<s>>
Init == s = [phase |-> "start", up |-> TRUE, crashes |-> 0,
             target |-> Original, diskTarget |-> Original,
             candidate |-> <<>>, diskCandidate |-> <<>>, named |-> FALSE,
             prepared |-> FALSE, receipt |-> FALSE, history |-> FALSE,
             computed |-> <<>>, captured |-> <<>>, recorded |-> <<>>,
             delivered |-> FALSE, later |-> FALSE, worker |-> FALSE,
             request |-> "original", mismatch |-> FALSE, seen |-> {}]

Begin ==
    /\ s.up /\ s.phase = "start" /\ ~s.worker
    /\ s' = [s EXCEPT !.phase =
          IF s.history /\ Fault # "reapply-completed" THEN "deliver"
          ELSE IF s.request # "original" /\ s.prepared /\ Fault # "mismatch"
               THEN "failed"
          ELSE IF s.receipt /\ Fault # "reapply-completed" THEN "history"
          ELSE IF s.prepared /\ Fault # "recompute" /\ Fault # "reapply-completed"
               THEN "publish" ELSE "compute",
          !.mismatch = s.mismatch \/ (s.request # "original" /\ s.prepared /\ Fault = "mismatch")]

Compute ==
    /\ s.up /\ s.phase = "compute"
    /\ s' = [s EXCEPT !.computed = Result(s.target),
          !.candidate = Transform(s.target),
          !.phase = IF Changes THEN "stage" ELSE "history"]

Stage ==
    /\ s.up /\ s.phase = "stage"
    /\ s' = [s EXCEPT !.worker = TRUE, !.phase = "file-sync"]

SyncFile ==
    /\ s.worker /\ s.phase = "file-sync"
    /\ s' = [s EXCEPT !.diskCandidate =
          IF Fault = "skip-file-sync" THEN <<>> ELSE s.candidate,
          !.phase = "directory-sync"]

SyncPrivateDirectory ==
    /\ s.worker /\ s.phase = "directory-sync"
    /\ s' = [s EXCEPT !.named = Fault # "skip-private-sync",
          !.worker = FALSE, !.phase = "capture"]

Capture ==
    /\ s.up /\ s.phase = "capture"
    /\ s' = [s EXCEPT !.prepared = TRUE,
          !.captured = IF Fault = "lose-result" THEN <<>> ELSE s.computed,
          !.phase = "publish", !.seen = @ \cup {"capture-commit"}]

Publish ==
    /\ s.up /\ s.phase = "publish"
    /\ s' = IF ~s.named /\ Fault # "skip-private-sync"
             THEN [s EXCEPT !.phase = "failed"]
             ELSE [s EXCEPT !.target = s.candidate, !.worker = TRUE,
                   !.phase = "public-sync", !.seen = @ \cup {"published"}]

SyncPublicDirectory ==
    /\ s.worker /\ s.phase = "public-sync"
    /\ s' = [s EXCEPT !.diskTarget =
          IF Fault = "skip-public-sync" THEN s.diskTarget ELSE s.target,
          !.worker = FALSE, !.phase = "receipt"]

Receipt ==
    /\ s.up /\ s.phase = "receipt"
    /\ s' = [s EXCEPT !.receipt = TRUE, !.phase = "history",
          !.seen = @ \cup {"receipt-commit"}]

History ==
    /\ s.up /\ s.phase = "history"
    /\ s' = [s EXCEPT !.history = TRUE,
          !.recorded = IF Changes THEN s.captured ELSE s.computed,
          !.phase = "deliver"]

Deliver ==
    /\ s.up
    /\ (s.phase = "deliver" \/ (Fault = "early-delivery" /\ s.phase = "history"))
    /\ s' = [s EXCEPT !.delivered = TRUE, !.phase = "done"]

LaterEffect ==
    /\ s.up /\ s.delivered /\ ~s.later
    /\ s' = [s EXCEPT !.target = Later, !.diskTarget = Later, !.later = TRUE]

(* A commit can outlive its acknowledgement: Crash is enabled immediately
   after Capture/Receipt/History, before the next action observes that state. *)
Crash ==
    /\ s.up /\ s.crashes < MaxCrashes
    /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1, !.phase = "offline",
          !.worker = FALSE, !.target = s.diskTarget,
          !.candidate = IF s.named THEN s.diskCandidate ELSE <<>>,
          !.computed = <<>>,
          !.seen = @ \cup
            (IF s.prepared /\ ~s.receipt /\ "published" \in s.seen THEN {"publication-gap"} ELSE {})
            \cup (IF s.receipt /\ ~s.history THEN {"history-gap"} ELSE {})]

Cancel ==
    /\ s.up /\ s.worker /\ s.crashes < MaxCrashes
    /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1,
          !.seen = @ \cup {"cancelled-worker"}]

Restart ==
    /\ ~s.up /\ ~s.worker
    /\ s' = [s EXCEPT !.up = TRUE, !.phase = "start", !.computed = <<>>,
          !.seen = @ \cup {"restart"}]

DifferentRequest ==
    /\ Fault = "mismatch" /\ ~s.up /\ s.prepared
    /\ s' = [s EXCEPT !.request = "different"]

Step == Begin \/ Compute \/ Stage \/ SyncFile \/ SyncPrivateDirectory \/ Capture
        \/ Publish \/ SyncPublicDirectory \/ Receipt \/ History \/ Deliver
Next == Step \/ Crash \/ Cancel \/ Restart \/ LaterEffect \/ DifferentRequest
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Step) /\ WF_vars(Restart)
Progress == <>(s.delivered)

CaptureDurable == s.prepared => s.named /\ s.diskCandidate = Expected /\ s.captured = ExpectedResult
CompletionDurable == s.receipt /\ ~s.later => s.diskTarget = Expected
OutcomeStable == s.history => s.recorded = ExpectedResult
ExposureOrdered == s.delivered => s.history /\ (~Changes \/ s.receipt)
LaterPreserved == s.later => s.target = Later
IdentityChecked == ~s.mismatch
AtomicPublication == s.target \in {Original, Expected, Later}
NeverWitness == CASE Witness = "publication-gap" ->
                      ~({"publication-gap", "restart"} \subseteq s.seen /\ s.delivered)
                  [] Witness = "history-gap" ->
                      ~({"history-gap", "restart"} \subseteq s.seen /\ s.delivered)
                  [] Witness = "cancelled-worker" ->
                      ~({"cancelled-worker", "restart"} \subseteq s.seen /\ s.delivered)
                  [] OTHER -> TRUE
=============================================================================
