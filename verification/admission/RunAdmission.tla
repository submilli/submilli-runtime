----------------------------- MODULE RunAdmission -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS MaxRuns, Fault, Witness
Runs == 1..MaxRuns
Keys == 0..1 \* Zero means no caller-provided key.
States == {"absent", "prepared", "running", "completed", "indeterminate", "aborted", "cancelled"}
VARIABLES db, pending, known, dispatched, startup, closed, stale, witnessed
vars == <<db, pending, known, dispatched, startup, closed, stale, witnessed>>
Empty == [state |-> "absent", key |-> 0, code |-> 0, generation |-> 0,
          revision |-> 0, order |-> 0, admitted |-> FALSE, retained |-> FALSE]
Init == /\ db = [r \in Runs |-> Empty]
        /\ pending = [r \in Runs |-> [staged |-> FALSE, key |-> 0, code |-> 0]]
        /\ known = [r \in Runs |-> "none"]
        /\ dispatched = [r \in Runs |-> 0]
        /\ startup = 1 /\ closed = FALSE /\ stale = FALSE /\ witnessed = FALSE
Used(k) == k # 0 /\ \E r \in Runs : db[r].key = k /\ db[r].state # "absent"
NextOrder == Cardinality({r \in Runs : db[r].state # "absent"}) + 1
Stage(r,k,c) ==
    /\ ~closed /\ db[r].state = "absent" /\ ~pending[r].staged
    /\ pending' = [pending EXCEPT ![r] = [staged |-> TRUE, key |-> k, code |-> c]]
    /\ UNCHANGED <<db,known,dispatched,startup,closed,stale,witnessed>>
CommitPreparation(r) ==
    /\ ~closed /\ pending[r].staged /\ db[r].state = "absent"
    /\ (Fault = "duplicate-key" \/ ~Used(pending[r].key))
    /\ db' = [db EXCEPT ![r] = [Empty EXCEPT !.state = "prepared",
        !.key = pending[r].key, !.code = pending[r].code,
        !.generation = startup, !.order = NextOrder]]
    /\ UNCHANGED <<pending,known,dispatched,startup,closed,stale,witnessed>>
Observe(r) ==
    /\ db[r].state # "absent"
    /\ known' = [known EXCEPT ![r] = db[r].state]
    /\ witnessed' = (witnessed \/ (Witness = "lost-ack" /\ pending[r].staged))
    /\ UNCHANGED <<db,pending,dispatched,startup,closed,stale>>
Admit(r) ==
    /\ ~closed /\ known[r] = "prepared" /\ db[r].state = "prepared"
    /\ db[r].generation = startup
    /\ db' = [db EXCEPT ![r].state = "running", ![r].admitted = TRUE,
                       ![r].revision = @ + 1]
    /\ UNCHANGED <<pending,known,dispatched,startup,closed,stale,witnessed>>
Dispatch(r) ==
    \* A launch permit may already be in memory when closure fences the row.
    \* Lifecycle draining is not part of this compatibility increment.
    /\ known[r] = "running" /\ dispatched[r] = 0
    /\ db[r].generation = startup
    /\ dispatched' = [dispatched EXCEPT ![r] = @ + 1]
    /\ witnessed' = (witnessed \/ (Witness = "unkeyed" /\ db[r].key = 0)
                     \/ (Witness = "late-dispatch" /\ closed /\ db[r].state = "cancelled"))
    /\ UNCHANGED <<db,pending,known,startup,closed,stale>>
Release(r) ==
    /\ db[r].generation = startup /\ dispatched[r] = 0
    /\ (db[r].state = "prepared" \/ (Fault = "release-admitted" /\ db[r].state = "running"))
    /\ db' = [db EXCEPT ![r].state = "aborted", ![r].key = 0, ![r].revision = @ + 1]
    /\ UNCHANGED <<pending,known,dispatched,startup,closed,stale,witnessed>>
Complete(r) ==
    /\ dispatched[r] = 1 /\ known[r] = "running"
    /\ (Fault = "stale-write" \/ (db[r].state = "running" /\ db[r].generation = startup))
    /\ db[r].state # "completed"
    /\ stale' = (stale \/ db[r].state # "running" \/ db[r].generation # startup)
    /\ db' = [db EXCEPT ![r].state = "completed", ![r].retained = TRUE, ![r].revision = @ + 1]
    /\ UNCHANGED <<pending,known,dispatched,startup,closed,witnessed>>
Expire(r) ==
    /\ db[r].state = "completed" /\ db[r].retained
    /\ db' = [db EXCEPT ![r].retained = FALSE, ![r].revision = @ + 1]
    /\ UNCHANGED <<pending,known,dispatched,startup,closed,stale,witnessed>>
Close ==
    /\ ~closed /\ closed' = TRUE
    /\ db' = [r \in Runs |-> IF db[r].state \in {"prepared","running"}
          THEN [db[r] EXCEPT !.state = "cancelled", !.revision = @ + 1] ELSE db[r]]
    /\ UNCHANGED <<pending,known,dispatched,startup,stale,witnessed>>
Crash ==
    /\ startup = 1 /\ startup' = 2
    /\ pending' = [r \in Runs |-> [staged |-> FALSE, key |-> 0, code |-> 0]] /\ known' = [r \in Runs |-> "none"]
    /\ witnessed' = (witnessed \/ (Witness = "interrupted" /\ \E r \in Runs : db[r].state = "running"))
    /\ UNCHANGED <<db,dispatched,closed,stale>>
Recover(r) ==
    /\ db[r].generation < startup /\ db[r].state \in {"prepared","running"}
    /\ db' = [db EXCEPT ![r].state = "indeterminate", ![r].revision = @ + 1]
    /\ UNCHANGED <<pending,known,dispatched,startup,closed,stale,witnessed>>
Next == (\E r \in Runs, k \in Keys, c \in 1..2 : Stage(r,k,c))
     \/ (\E r \in Runs : CommitPreparation(r) \/ Observe(r) \/ Admit(r) \/ Dispatch(r)
          \/ Release(r) \/ Complete(r) \/ Expire(r) \/ Recover(r)) \/ Close \/ Crash
TypeOK == /\ \A r \in Runs : db[r].state \in States /\ dispatched[r] \in 0..1
          /\ startup \in 1..2
AdmissionUnique == \A a,b \in Runs :
    (db[a].key # 0 /\ db[a].key = db[b].key /\ db[a].state # "absent" /\ db[b].state # "absent") => a=b
KeyReserved == \A r \in Runs : db[r].admitted => db[r].state # "aborted"
CommitBarrier == \A r \in Runs : dispatched[r] = 1 => db[r].admitted
OrderUnique == \A a,b \in Runs : (db[a].order # 0 /\ db[a].order = db[b].order) => a=b
NoStaleWrites == ~stale
NeverWitness == ~witnessed
Spec == Init /\ [][Next]_vars
=============================================================================
