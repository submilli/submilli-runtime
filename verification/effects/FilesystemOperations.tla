------------------------- MODULE FilesystemOperations -------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Fault, Witness
VARIABLES rows, first, prepared, completed, online, crashes, acknowledged, lost, witnessed
vars == <<rows, first, prepared, completed, online, crashes, acknowledged, lost, witnessed>>
Slots == 1..2
Kinds == {"filesystem-v1", "other-v1"}
Empty == [state |-> "absent", kind |-> "none", request |-> 0, payload |-> 0, outcome |-> 0]
Pending == {s \in Slots : rows[s].state \in {"preparing", "pending"}}
Terminal(s) == rows[s].state \in {"done", "rejected"}
Saw(w, condition) == witnessed \/ (Witness = w /\ condition)
Init ==
    /\ rows = [s \in Slots |-> Empty]
    /\ first = [s \in Slots |-> [kind |-> "none", request |-> 0]]
    /\ prepared = [s \in Slots |-> 0]
    /\ completed = [s \in Slots |-> 0]
    /\ online = TRUE /\ crashes = 0 /\ acknowledged = {} /\ lost = {}
    /\ witnessed = FALSE

\* Admission commits the immutable operation/version/request identity.
Admit(s, kind, request) ==
    /\ online /\ rows[s].state = "absent"
    /\ (Pending = {} \/ Fault = "overlap")
    /\ rows' = [rows EXCEPT ![s] = [Empty EXCEPT !.state = "preparing", !.kind = kind, !.request = request]]
    /\ first' = [first EXCEPT ![s] = [kind |-> kind, request |-> request]]
    /\ UNCHANGED <<prepared, completed, online, crashes, acknowledged, lost, witnessed>>

\* A conflicting adapter must not reinterpret an existing slot.
Conflict(s, kind, request) ==
    /\ online /\ rows[s].state # "absent" /\ (rows[s].kind # kind \/ rows[s].request # request)
    /\ rows' = CASE Fault = "identity" -> [rows EXCEPT ![s].kind = kind]
                    [] Fault = "request" -> [rows EXCEPT ![s].request = request]
                    [] OTHER -> rows
    /\ witnessed' = (Saw("conflict", rows[s].kind # kind) \/
                       (Witness = "request-conflict" /\ rows[s].kind = kind /\ rows[s].request # request))
    /\ UNCHANGED <<first, prepared, completed, online, crashes, acknowledged, lost>>

Prepare(s, payload) ==
    /\ online /\ (rows[s].state = "preparing" \/
                  (Fault = "payload" /\ rows[s].state = "pending"))
    /\ rows' = [rows EXCEPT ![s].state = "pending", ![s].payload = payload]
    /\ prepared' = IF prepared[s] = 0 THEN [prepared EXCEPT ![s] = payload] ELSE prepared
    /\ UNCHANGED <<first, completed, online, crashes, acknowledged, lost, witnessed>>

\* The adapter supplies persistence barriers before Complete. The filesystem
\* models verify those barriers; this model verifies only the SQL lifecycle.
Complete(s, value) ==
    /\ online /\ (rows[s].state = "pending" \/
                  (Fault = "outcome" /\ rows[s].state = "done"))
    /\ rows' = [rows EXCEPT ![s].state = "done", ![s].outcome = value]
    /\ completed' = IF completed[s] = 0 THEN [completed EXCEPT ![s] = value] ELSE completed
    /\ UNCHANGED <<first, prepared, online, crashes, acknowledged, lost, witnessed>>

Reject(s) ==
    /\ online /\ (rows[s].state = "preparing" \/
                  (Fault = "reject-pending" /\ rows[s].state = "pending"))
    /\ rows' = [rows EXCEPT ![s].state = "rejected", ![s].outcome = 1]
    /\ completed' = [completed EXCEPT ![s] = 1]
    /\ UNCHANGED <<first, prepared, online, crashes, acknowledged, lost, witnessed>>

Replay(s) ==
    /\ online /\ (Terminal(s) \/ (Fault = "early-return" /\ rows[s].state = "pending"))
    /\ s \notin acknowledged
    /\ acknowledged' = acknowledged \cup {s}
    /\ witnessed' = Saw("lost-ack", s \in lost)
    /\ UNCHANGED <<rows, first, prepared, completed, online, crashes, lost>>

Crash ==
    /\ online /\ crashes = 0
    \* Force the witness to exercise a committed but unacknowledged outcome.
    /\ \E s \in Slots : Terminal(s) /\ s \notin acknowledged
    /\ online' = FALSE /\ crashes' = 1
    /\ lost' = {s \in Slots : Terminal(s) /\ s \notin acknowledged}
    /\ UNCHANGED <<rows, first, prepared, completed, acknowledged, witnessed>>
Restart ==
    /\ ~online /\ online' = TRUE
    /\ UNCHANGED <<rows, first, prepared, completed, crashes, acknowledged, lost, witnessed>>
Next == (\E s \in Slots :
           (\E k \in Kinds, q \in 1..2 : Admit(s, k, q) \/ Conflict(s, k, q)) \/
           (\E v \in 1..2 : Prepare(s, v) \/ Complete(s, v)) \/ Reject(s) \/ Replay(s))
        \/ Crash \/ Restart
TypeOK ==
    /\ rows \in [Slots -> [state : {"absent", "preparing", "pending", "done", "rejected"},
                          kind : Kinds \cup {"none"}, request : 0..2, payload : 0..2, outcome : 0..2]]
    /\ first \in [Slots -> [kind : Kinds \cup {"none"}, request : 0..2]]
    /\ prepared \in [Slots -> 0..2] /\ completed \in [Slots -> 0..2]
    /\ online \in BOOLEAN /\ crashes \in 0..1 /\ acknowledged \subseteq Slots /\ lost \subseteq Slots /\ witnessed \in BOOLEAN
ImmutableIdentity ==
    \A s \in Slots :
        /\ rows[s].kind = first[s].kind
        /\ rows[s].request = first[s].request
ImmutablePayload == \A s \in Slots : rows[s].payload = prepared[s]
ImmutableOutcome == \A s \in Slots : rows[s].outcome = completed[s]
OnePending == Cardinality(Pending) <= 1
RefuseBeforePrepare == \A s \in Slots : rows[s].state = "rejected" => rows[s].payload = 0
ReceiptBeforeReturn == \A s \in acknowledged : Terminal(s)
NeverWitness == ~witnessed
Spec == Init /\ [][Next]_vars
=============================================================================
