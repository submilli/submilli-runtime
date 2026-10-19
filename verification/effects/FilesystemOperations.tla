------------------------- MODULE FilesystemOperations -------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Fault, Witness
VARIABLES rows, first, prepared, completed, online, crashes, acknowledged, lost, witnessed
vars == <<rows, first, prepared, completed, online, crashes, acknowledged, lost, witnessed>>
Slots == 1..2
Kinds == {"filesystem-v1", "other-v1"}
(* State is ONE encoded enum. NoState represents SQL NULL, decoded as Initial.
   prepared/completed are ghost history for invariants, not extra SQL columns. *)
NoState == [variant |-> "Initial", value |-> 0]
(* 3 represents a Prepared marker with no fields; 1 and 2 represent saved
   retry facts such as append offsets. The variant distinguishes the marker
   from Initial even when there is no operation-specific payload. *)
PreparedValues == 1..3
Prepared(v) == [variant |-> "Prepared", value |-> v]
Completed(v) == [variant |-> "Completed", value |-> v]
Rejected == [variant |-> "Rejected", value |-> 1]
States == {NoState, Rejected} \cup {Prepared(v) : v \in PreparedValues}
                              \cup {Completed(v) : v \in 1..2}
(* Completed and Rejected abstract the Rust Completed(Ok/Err) variants. *)
Status(state) == CASE state.variant = "Initial" -> "preparing"
                  [] state.variant = "Prepared" -> "pending"
                  [] state.variant = "Completed" -> "done"
                  [] OTHER -> "rejected"
Empty == [status |-> "absent", kind |-> "none", request |-> 0, state |-> NoState]
Pending == {s \in Slots : rows[s].status \in {"preparing", "pending"}}
Terminal(s) == rows[s].state.variant \in {"Completed", "Rejected"}
Saw(w, condition) == witnessed \/ (Witness = w /\ condition)
Init ==
    /\ rows = [s \in Slots |-> Empty]
    /\ first = [s \in Slots |-> [kind |-> "none", request |-> 0]]
    /\ prepared = [s \in Slots |-> 0]
    /\ completed = [s \in Slots |-> 0]
    /\ online = TRUE /\ crashes = 0 /\ acknowledged = {} /\ lost = {}
    /\ witnessed = FALSE

Admit(s, kind, request) ==
    /\ online /\ rows[s].status = "absent"
    /\ (Pending = {} \/ Fault = "overlap")
    /\ rows' = [rows EXCEPT ![s] = [Empty EXCEPT !.status = "preparing", !.kind = kind, !.request = request]]
    /\ first' = [first EXCEPT ![s] = [kind |-> kind, request |-> request]]
    /\ UNCHANGED <<prepared, completed, online, crashes, acknowledged, lost, witnessed>>

Conflict(s, kind, request) ==
    /\ online /\ rows[s].status # "absent" /\ (rows[s].kind # kind \/ rows[s].request # request)
    /\ rows' = CASE Fault = "identity" -> [rows EXCEPT ![s].kind = kind]
                    [] Fault = "request" -> [rows EXCEPT ![s].request = request]
                    [] OTHER -> rows
    /\ witnessed' = (Saw("conflict", rows[s].kind # kind) \/
                       (Witness = "request-conflict" /\ rows[s].kind = kind /\ rows[s].request # request))
    /\ UNCHANGED <<first, prepared, completed, online, crashes, acknowledged, lost>>

Prepare(s, payload) ==
    /\ online /\ (rows[s].status = "preparing" \/
                  (Fault = "payload" /\ rows[s].status = "pending"))
    /\ rows' = [rows EXCEPT ![s].status = "pending", ![s].state = Prepared(payload)]
    /\ prepared' = IF prepared[s] = 0 THEN [prepared EXCEPT ![s] = payload] ELSE prepared
    /\ UNCHANGED <<first, completed, online, crashes, acknowledged, lost, witnessed>>

(* Completion replaces prepared data, rather than adding an outcome column.
   Native persistence barriers are checked in the filesystem models. *)
Complete(s, value) ==
    /\ online /\ (rows[s].status = "pending" \/
                  (Fault = "outcome" /\ rows[s].status = "done") \/
                  (Fault = "early-completion" /\ rows[s].status = "preparing"))
    /\ rows' = [rows EXCEPT ![s].status = "done", ![s].state = Completed(value)]
    /\ completed' = IF completed[s] = 0 THEN [completed EXCEPT ![s] = value] ELSE completed
    /\ witnessed' = Saw("empty-prepared", prepared[s] = 3)
    /\ UNCHANGED <<first, prepared, online, crashes, acknowledged, lost>>

Reject(s) ==
    /\ online /\ (rows[s].status = "preparing" \/
                  (Fault = "reject-pending" /\ rows[s].status = "pending"))
    /\ rows' = [rows EXCEPT ![s].status = "rejected", ![s].state = Rejected]
    /\ completed' = [completed EXCEPT ![s] = 1]
    /\ UNCHANGED <<first, prepared, online, crashes, acknowledged, lost, witnessed>>

Replay(s) ==
    /\ online /\ (Terminal(s) \/ (Fault = "early-return" /\ rows[s].status = "pending"))
    /\ s \notin acknowledged
    /\ acknowledged' = acknowledged \cup {s}
    /\ witnessed' = Saw("lost-ack", s \in lost)
    /\ UNCHANGED <<rows, first, prepared, completed, online, crashes, lost>>

Crash ==
    /\ online /\ crashes = 0
    /\ \E s \in Slots : Terminal(s) /\ s \notin acknowledged
    /\ online' = FALSE /\ crashes' = 1
    /\ lost' = {s \in Slots : Terminal(s) /\ s \notin acknowledged}
    /\ UNCHANGED <<rows, first, prepared, completed, acknowledged, witnessed>>
Restart ==
    /\ ~online /\ online' = TRUE
    /\ UNCHANGED <<rows, first, prepared, completed, crashes, acknowledged, lost, witnessed>>

ForgetState(s) ==
    /\ online /\ Fault = "forget-state" /\ rows[s].state # NoState
    /\ rows' = [rows EXCEPT ![s].state = NoState, ![s].status = "preparing"]
    /\ UNCHANGED <<first, prepared, completed, online, crashes, acknowledged, lost, witnessed>>
WrongIndex(s) ==
    /\ online /\ Fault = "wrong-index" /\ rows[s].status = "pending"
    /\ rows' = [rows EXCEPT ![s].status = "done"]
    /\ UNCHANGED <<first, prepared, completed, online, crashes, acknowledged, lost, witnessed>>

DecodeState(s) == IF rows[s].kind = "none" THEN "Absent" ELSE rows[s].state.variant
OperationLoop(s) ==
    CASE DecodeState(s) = "Absent" -> (\E k \in Kinds, q \in 1..2 : Admit(s, k, q))
      [] DecodeState(s) = "Initial" -> (\E v \in PreparedValues : Prepare(s, v)) \/ Reject(s)
      [] DecodeState(s) = "Prepared" -> (\E v \in 1..2 : Complete(s, v))
      [] DecodeState(s) \in {"Completed", "Rejected"} -> Replay(s)
      [] OTHER -> FALSE
FaultyLoop(s) == (Fault = "payload" /\ (\E v \in PreparedValues : Prepare(s, v)))
                \/ (Fault \in {"outcome", "early-completion"} /\ (\E v \in 1..2 : Complete(s, v)))
                \/ (Fault = "reject-pending" /\ Reject(s))
                \/ (Fault = "early-return" /\ Replay(s))
                \/ ForgetState(s) \/ WrongIndex(s)
Next == (\E s \in Slots : OperationLoop(s) \/ FaultyLoop(s)
           \/ (\E k \in Kinds, q \in 1..2 : Conflict(s, k, q)))
        \/ Crash \/ Restart

TypeOK ==
    /\ rows \in [Slots -> [status : {"absent", "preparing", "pending", "done", "rejected"},
                          kind : Kinds \cup {"none"}, request : 0..2, state : States]]
    /\ first \in [Slots -> [kind : Kinds \cup {"none"}, request : 0..2]]
    /\ prepared \in [Slots -> 0..3] /\ completed \in [Slots -> 0..2]
    /\ online \in BOOLEAN /\ crashes \in 0..1 /\ acknowledged \subseteq Slots /\ lost \subseteq Slots /\ witnessed \in BOOLEAN
ImmutableIdentity ==
    \A s \in Slots : /\ rows[s].kind = first[s].kind
                     /\ rows[s].request = first[s].request
ImmutablePayload == \A s \in Slots : rows[s].state.variant = "Prepared" => rows[s].state.value = prepared[s]
ImmutableOutcome == \A s \in Slots : Terminal(s) => rows[s].state.value = completed[s]
ProgressNeverResets == \A s \in Slots :
    /\ prepared[s] # 0 => rows[s].state.variant # "Initial"
    /\ completed[s] # 0 => Terminal(s)
IndexMatchesState == \A s \in Slots : rows[s].kind # "none" => rows[s].status = Status(rows[s].state)
PrepareBeforeComplete == \A s \in Slots : rows[s].state.variant = "Completed" => prepared[s] # 0
OnePending == Cardinality(Pending) <= 1
RefuseBeforePrepare == \A s \in Slots : rows[s].state.variant = "Rejected" => prepared[s] = 0
ReceiptBeforeReturn == \A s \in acknowledged : Terminal(s)
NeverWitness == ~witnessed
Spec == Init /\ [][Next]_vars
=============================================================================
