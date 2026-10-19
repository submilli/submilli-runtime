-------------------------- MODULE DurableExecution --------------------------
EXTENDS Naturals, Sequences, FiniteSets

\* A bounded protocol model, not the interpreter or a SQLite implementation.
\* One session makes admission order observable. Runs are numbered on admission.
CONSTANTS MaxRuns, MaxSteps, MaxCrashes, FuelLimit, Lifecycle, Compatible,
          LegacyKey, Idempotent, Fault, Witness, NoWrite

ASSUME /\ MaxRuns \in Nat \ {0} /\ MaxSteps \in Nat \ {0}
       /\ MaxCrashes \in Nat /\ FuelLimit \in Nat
       /\ Lifecycle \in BOOLEAN /\ Compatible \in BOOLEAN /\ LegacyKey \in BOOLEAN
       /\ Idempotent \in BOOLEAN
       /\ Fault \in {"none", "early-ack", "expire-key", "skip-replay",
                    "stale-write", "new-key", "consume-uncommitted"}

Runs == 1..MaxRuns
Keys == 1..MaxRuns
Values == {1, 2}
Terminal == {"completed", "cancelled"}
OutcomeKinds == {"observation", "outcome"}
ChargedKinds == OutcomeKinds
EffectKey(r, step) == <<r, step, 1>>
IsObservation(step) == step % 2 = 1

VARIABLES db, process, flight, finished, provider, monitor, witnessed
vars == <<db, process, flight, finished, provider, monitor, witnessed>>

EmptyLocal == [active |-> FALSE, generation |-> 0, step |-> 1,
               seen |-> <<>>, replayThrough |-> 0, pending |-> NoWrite,
               call |-> "none", reply |-> 0]
EmptyRun == [state |-> "absent", key |-> 0, code |-> 0, generation |-> 0,
             history |-> <<>>, next |-> 1, spent |-> 0, response |-> <<>>,
             retained |-> FALSE, pinned |-> FALSE]

Init ==
    /\ db = [runs |-> [r \in Runs |->
                         IF LegacyKey /\ r = 1
                         THEN [EmptyRun EXCEPT !.state = "indeterminate",
                                !.key = 1, !.code = 1, !.pinned = TRUE]
                         ELSE EmptyRun],
             keys |-> [k \in Keys |-> IF LegacyKey /\ k = 1 THEN 1 ELSE 0],
             admitted |-> IF LegacyKey THEN 1 ELSE 0,
             lastResult |-> 0, closed |-> FALSE]
    /\ process = [online |-> TRUE, generation |-> 1,
                  local |-> [r \in Runs |-> EmptyLocal]]
    /\ flight = {}
    /\ finished = {}
    /\ provider = {}
    /\ monitor = [repeatedDispatch |-> FALSE,
                   mutations |-> [r \in Runs |-> [s \in 1..MaxSteps |-> 0]],
                   badIdentity |-> FALSE, earlyConsume |-> FALSE,
                   earlyAck |-> FALSE, staleWrite |-> FALSE,
                   outOfOrder |-> FALSE, badClientResponse |-> FALSE]
    /\ witnessed = FALSE

Saw(name, condition) == witnessed \/ (Witness = name /\ condition)
Events(r, step, kinds) ==
    {db.runs[r].history[i] : i \in
        {j \in 1..Len(db.runs[r].history) :
            db.runs[r].history[j].step = step /\
            db.runs[r].history[j].kind \in kinds}}
HasOutcome(r, step) == Events(r, step, OutcomeKinds) # {}
Outcome(r, step) == CHOOSE e \in Events(r, step, OutcomeKinds) : TRUE
PreviousOutcomes(r, step) ==
    [i \in 1..(step - 1) |-> Outcome(r, i).value]
Arguments(r) ==
    <<db.runs[r].code, process.local[r].step, process.local[r].seen>>
ExpectedArguments(r, step) ==
    <<db.runs[r].code, step, PreviousOutcomes(r, step)>>
Entry(kind, r, value) ==
    [kind |-> kind, step |-> process.local[r].step,
     seq |-> db.runs[r].next, args |-> Arguments(r), value |-> value,
     key |-> EffectKey(r, process.local[r].step)]
Request(r) ==
    [run |-> r, step |-> process.local[r].step,
     generation |-> process.generation,
     key |-> IF Fault = "new-key"
             THEN <<r, process.local[r].step, process.generation>>
             ELSE EffectKey(r, process.local[r].step),
     args |-> Arguments(r)]
ProviderRecords(key) == {p \in provider : p.key = key}
InFlight(r) == \E q \in flight : q.run = r
EarlierDrained(r) ==
    \A q \in 1..(r - 1) :
        db.runs[q].state \in Terminal /\
        ~process.local[q].active /\ ~InFlight(q)
Current(r) ==
    process.online /\ process.local[r].active /\
    db.runs[r].state = "running" /\
    process.local[r].generation = db.runs[r].generation
CanPrepare(r) ==
    Current(r) /\ process.local[r].pending = NoWrite /\
    process.local[r].step <= MaxSteps
Charged(r) ==
    Cardinality({i \in 1..Len(db.runs[r].history) :
                    db.runs[r].history[i].kind \in ChargedKinds})
RecordedThrough(r) ==
    Cardinality({i \in 1..Len(db.runs[r].history) :
                    db.runs[r].history[i].kind \in OutcomeKinds})

Admit(key, code) ==
    LET r == db.admitted + 1 IN
    /\ process.online /\ ~db.closed
    /\ db.keys[key] = 0 /\ r \in Runs
    /\ db' = [db EXCEPT !.keys[key] = r, !.admitted = r,
               !.runs[r] = [EmptyRun EXCEPT !.state = "accepted",
                            !.key = key, !.code = code, !.pinned = TRUE]]
    /\ UNCHANGED <<process, flight, finished, provider, monitor, witnessed>>

ClientResponse(r, code) ==
    [run |-> r, code |-> code,
          kind |-> IF db.runs[r].code # code THEN "conflict"
                   ELSE IF db.runs[r].state = "indeterminate" THEN "indeterminate"
                   ELSE IF db.runs[r].state = "cancelled" THEN "cancelled"
                   ELSE IF db.runs[r].state = "blocked" THEN "blocked"
                   ELSE IF db.runs[r].state # "completed" THEN "attach"
                   ELSE IF db.runs[r].retained THEN "result" ELSE "expired",
          response |-> IF db.runs[r].code = code /\
                          db.runs[r].state = "completed" /\ db.runs[r].retained
                       THEN db.runs[r].response ELSE <<>>]

ResponseMatches(c) ==
    /\ c.run \in 1..db.admitted
    /\ IF c.kind = "conflict"
       THEN c.code # db.runs[c.run].code /\ c.response = <<>>
       ELSE c.code = db.runs[c.run].code
    /\ IF c.kind = "result"
       THEN db.runs[c.run].state = "completed" /\ db.runs[c.run].retained /\
            c.response = PreviousOutcomes(c.run, MaxSteps + 1)
       ELSE c.response = <<>>
    /\ CASE c.kind = "expired" ->
                 db.runs[c.run].state = "completed" /\ ~db.runs[c.run].retained
         [] c.kind = "attach" -> db.runs[c.run].state \in {"accepted", "running"}
         [] c.kind \in {"cancelled", "blocked", "indeterminate"} ->
                 db.runs[c.run].state = c.kind
         [] OTHER -> c.kind \in {"conflict", "result"}

Duplicate(key, code) ==
    LET r == db.keys[key] IN
    /\ process.online /\ ~db.closed /\ r # 0
    \* Latch validity at delivery instead of retaining irrelevant old responses
    \* in every subsequent state. This observer never controls the protocol.
    /\ monitor' = [monitor EXCEPT !.badClientResponse =
                     @ \/ ~ResponseMatches(ClientResponse(r, code))]
    /\ witnessed' = (
        Saw("conflicting-retry", db.runs[r].code # code) \/
        Saw("expired-retry", db.runs[r].code = code /\
            db.runs[r].state = "completed" /\ ~db.runs[r].retained) \/
        Saw("legacy-refused", db.runs[r].state = "indeterminate"))
    /\ UNCHANGED <<db, process, flight, finished, provider>>

Claim(r) ==
    /\ process.online /\ ~db.closed /\ EarlierDrained(r)
    /\ db.runs[r].state \in {"accepted", "running"}
    /\ db.runs[r].generation < process.generation
    /\ db' = [db EXCEPT !.runs[r].state = "running",
               !.runs[r].generation = process.generation]
    /\ UNCHANGED <<process, flight, finished, provider, monitor, witnessed>>

Launch(r) ==
    /\ process.online /\ ~process.local[r].active
    /\ db.runs[r].state = "running"
    /\ db.runs[r].generation = process.generation
    /\ db' = IF Compatible THEN db
             ELSE [db EXCEPT !.runs[r].state = "blocked"]
    /\ process' = IF Compatible
                  THEN [process EXCEPT !.local[r] =
                       [EmptyLocal EXCEPT !.active = TRUE,
                        !.generation = process.generation,
                        !.replayThrough = RecordedThrough(r)]]
                  ELSE process
    /\ monitor' = [monitor EXCEPT
                    !.outOfOrder = @ \/ ~EarlierDrained(r)]
    /\ UNCHANGED <<flight, finished, provider, witnessed>>

Prepare(r, kind, value) ==
    /\ CanPrepare(r)
    /\ ~HasOutcome(r, process.local[r].step)
    /\ CASE kind = "observation" ->
                IsObservation(process.local[r].step) /\
                db.runs[r].spent < FuelLimit
         [] kind = "outcome" ->
                process.local[r].call = "reply" /\
                value = process.local[r].reply
    /\ process' = [process EXCEPT !.local[r].pending =
                    [generation |-> process.local[r].generation,
                     event |-> Entry(kind, r, value)]]
    /\ UNCHANGED <<db, flight, finished, provider, monitor, witnessed>>

PrepareTerminal(r) ==
    /\ Current(r) /\ process.local[r].pending = NoWrite
    /\ process.local[r].step = MaxSteps + 1
    /\ process' = [process EXCEPT !.local[r].pending =
                    [generation |-> process.local[r].generation,
                     event |-> Entry("terminal", r, process.local[r].seen)]]
    /\ UNCHANGED <<db, flight, finished, provider, monitor, witnessed>>

\* Preparation is separate: cancellation may fence this write before it commits.
\* Commit does not acknowledge itself. The caller must reread or lose its memory.
Commit(r) ==
    LET p == process.local[r].pending IN
    /\ process.online /\ p # NoWrite
    /\ p.event.seq = db.runs[r].next
    /\ db.runs[r].state # "completed"
    /\ (Fault = "stale-write" \/
        (p.generation = db.runs[r].generation /\
         db.runs[r].state = "running"))
    /\ db' = [db EXCEPT
         !.runs[r].history = IF p.event.kind = "terminal" THEN @ ELSE Append(@, p.event),
         !.runs[r].next = IF p.event.kind = "terminal" THEN @ ELSE @ + 1,
         !.runs[r].spent = @ + IF p.event.kind \in ChargedKinds THEN 1 ELSE 0,
         !.runs[r].state = IF p.event.kind = "terminal" THEN "completed" ELSE @,
         !.runs[r].response = IF p.event.kind = "terminal" THEN p.event.value ELSE @,
         !.runs[r].retained = IF p.event.kind = "terminal" THEN TRUE ELSE @,
         !.lastResult = IF p.event.kind = "terminal" THEN r ELSE @]
    /\ monitor' = [monitor EXCEPT !.staleWrite = @ \/
         p.generation # db.runs[r].generation \/ db.runs[r].state # "running"]
    /\ UNCHANGED <<process, flight, finished, provider, witnessed>>

\* Before a commit this models an aborted transaction; afterwards, a successful
\* commit with an acknowledgement that was lost or reconciled by rereading.
ForgetWrite(r) ==
    /\ process.online /\ process.local[r].pending # NoWrite
    /\ process' = [process EXCEPT !.local[r].pending = NoWrite]
    /\ UNCHANGED <<db, flight, finished, provider, monitor, witnessed>>

Dispatch(r) ==
    LET step == process.local[r].step
        q == Request(r)
    IN
    /\ Current(r) /\ step <= MaxSteps
    /\ ~IsObservation(step) /\ ~HasOutcome(r, step)
    /\ db.runs[r].spent < FuelLimit
    /\ process.local[r].pending = NoWrite /\ process.local[r].call = "none"
    \* Within a startup, retry only after completion and retirement of the reply.
    \* Requests from an older startup can still be in flight at the same time.
    /\ q \notin flight /\ q \notin {f.request : f \in finished}
    /\ flight' = flight \cup {q}
    /\ process' = [process EXCEPT !.local[r].call = "waiting"]
    /\ monitor' = [monitor EXCEPT
         !.repeatedDispatch = @ \/ HasOutcome(r, step),
         !.badIdentity = @ \/ q.key # EffectKey(r, step) \/
                             q.args # ExpectedArguments(r, step)]
    /\ witnessed' = Saw("retry-in-flight",
                         \E old \in flight : old.run = r /\ old.step = step /\
                                              old.generation # q.generation)
    /\ UNCHANGED <<db, finished, provider>>

\* The provider progresses independently, including while the server is down.
\* The framework has no idempotency policy. The adapter may deduplicate;
\* otherwise a repeated request may mutate again, even with the same key.
\* The saturating observer records whether duplication has occurred.
RemoteComplete(q, value) ==
    /\ q \in flight
    /\ flight' = flight \ {q}
    /\ finished' = finished \cup {[request |-> q, value |->
         IF Idempotent /\ ProviderRecords(q.key) # {}
         THEN (CHOOSE p \in ProviderRecords(q.key) : TRUE).value ELSE value]}
    /\ provider' =
        IF ~Idempotent \/ ProviderRecords(q.key) = {}
        THEN provider \cup {[key |-> q.key, run |-> q.run, step |-> q.step,
                            args |-> q.args, value |-> value]}
        ELSE provider
    /\ monitor' = [monitor EXCEPT !.mutations[q.run][q.step] =
         IF (Idempotent /\ ProviderRecords(q.key) # {}) \/ @ = 2 THEN @ ELSE @ + 1]
    /\ witnessed' = Saw("duplicate-mutation", ~Idempotent /\
                          monitor.mutations[q.run][q.step] = 1)
    /\ UNCHANGED <<db, process>>

Receive(r) ==
    LET q == Request(r)
        replies == {f \in finished : f.request = q}
    IN
    /\ Current(r) /\ process.local[r].call = "waiting"
    /\ replies # {}
    /\ process' = [process EXCEPT
         !.local[r].call = "reply",
         !.local[r].reply = (CHOOSE f \in replies : TRUE).value]
    /\ finished' = finished \ replies
    /\ UNCHANGED <<db, flight, provider, monitor, witnessed>>

LoseReply(r) ==
    /\ Current(r) /\ process.local[r].call = "waiting"
    /\ Request(r) \in {f.request : f \in finished}
    /\ process' = [process EXCEPT !.local[r].call = "none"]
    /\ finished' = {f \in finished : f.request # Request(r)}
    /\ UNCHANGED <<db, flight, provider, monitor, witnessed>>

Consume(r) ==
    LET step == process.local[r].step
        value == IF HasOutcome(r, step) THEN Outcome(r, step).value
                 ELSE process.local[r].pending.event.value
        replayed == IF Fault = "skip-replay" /\
                       step <= process.local[r].replayThrough
                    THEN CHOOSE other \in Values : other # value
                    ELSE value
    IN
    /\ Current(r) /\ step <= MaxSteps
    /\ ((HasOutcome(r, step) /\ process.local[r].pending = NoWrite) \/
        (Fault = "consume-uncommitted" /\ process.local[r].pending # NoWrite /\
         process.local[r].pending.event.kind \in OutcomeKinds))
    /\ process' = [process EXCEPT
         !.local[r].seen = Append(@, replayed), !.local[r].step = @ + 1,
         !.local[r].call = "none", !.local[r].reply = 0]
    /\ monitor' = [monitor EXCEPT !.earlyConsume = @ \/ ~HasOutcome(r, step)]
    /\ witnessed' = Saw("replayed", step <= process.local[r].replayThrough)
    /\ UNCHANGED <<db, flight, finished, provider>>

BlockForFuel(r) ==
    /\ Current(r) /\ process.local[r].step <= MaxSteps
    /\ process.local[r].pending = NoWrite
    /\ ~HasOutcome(r, process.local[r].step)
    /\ db.runs[r].spent = FuelLimit
    /\ db' = [db EXCEPT !.runs[r].state = "blocked"]
    /\ process' = [process EXCEPT !.local[r] = EmptyLocal]
    /\ UNCHANGED <<flight, finished, provider, monitor, witnessed>>

Ack(r) ==
    /\ process.online
    /\ ((db.runs[r].state = "completed" /\ db.runs[r].retained) \/
        (Fault = "early-ack" /\ process.local[r].step = MaxSteps + 1))
    /\ monitor' = [monitor EXCEPT !.earlyAck = @ \/
                     db.runs[r].state # "completed" \/
                     db.runs[r].response # PreviousOutcomes(r, MaxSteps + 1)]
    /\ UNCHANGED <<db, process, flight, finished, provider, witnessed>>

Cancel(r) ==
    /\ Lifecycle /\ process.online
    /\ db.runs[r].state \in {"accepted", "running", "blocked"}
    \* A cancellation fence is durable; an old prepared write keeps its sequence.
    /\ db' = [db EXCEPT !.runs[r].state = "cancelled",
               !.runs[r].generation = @ + 1]
    /\ witnessed' = Saw("cancel-pending", process.local[r].pending # NoWrite)
    /\ UNCHANGED <<process, flight, finished, provider, monitor>>

Drain(r) ==
    /\ process.online /\ db.runs[r].state \in Terminal
    /\ process.local[r].active /\ ~InFlight(r)
    /\ process' = [process EXCEPT !.local[r] = EmptyLocal]
    /\ UNCHANGED <<db, flight, finished, provider, monitor, witnessed>>

Release(r) ==
    /\ Lifecycle /\ process.online /\ db.runs[r].state \in Terminal
    /\ db.runs[r].pinned /\ ~process.local[r].active /\ ~InFlight(r)
    /\ db' = [db EXCEPT !.runs[r].pinned = FALSE]
    /\ UNCHANGED <<process, flight, finished, provider, monitor, witnessed>>

ExpireResponse(r) ==
    /\ Lifecycle /\ process.online
    /\ db.runs[r].state = "completed" /\ db.runs[r].retained
    /\ db' = [db EXCEPT !.runs[r].retained = FALSE,
         !.keys[db.runs[r].key] =
             IF Fault = "expire-key" THEN 0 ELSE @]
    /\ UNCHANGED <<process, flight, finished, provider, monitor, witnessed>>

ExpireSession ==
    /\ Lifecycle /\ process.online /\ ~db.closed
    /\ \A r \in Runs : db.runs[r].state = "absent" \/
          (db.runs[r].state \in Terminal /\
           ~process.local[r].active /\ ~InFlight(r))
    /\ db' = [db EXCEPT !.closed = TRUE]
    /\ UNCHANGED <<process, flight, finished, provider, monitor, witnessed>>

Crash ==
    /\ process.online /\ process.generation <= MaxCrashes
    /\ process' = [process EXCEPT !.online = FALSE,
                   !.local = [r \in Runs |-> EmptyLocal]]
    /\ witnessed' = (
        Saw("crash-after-dispatch", flight # {}) \/
        Saw("crash-after-remote", \E p \in provider :
            ~HasOutcome(p.run, p.step)) \/
        Saw("crash-after-outcome", \E r \in Runs :
            process.local[r].active /\
            ~IsObservation(process.local[r].step) /\
            HasOutcome(r, process.local[r].step)) \/
        Saw("crash-after-terminal", \E r \in Runs :
            db.runs[r].state = "completed" /\ process.local[r].active))
    /\ UNCHANGED <<db, flight, finished, provider, monitor>>

Restart ==
    /\ ~process.online
    /\ process' = [process EXCEPT !.online = TRUE, !.generation = @ + 1]
    /\ UNCHANGED <<db, flight, finished, provider, monitor, witnessed>>

Next ==
    \/ \E key \in Keys, code \in Values : Admit(key, code) \/ Duplicate(key, code)
    \/ \E r \in Runs :
         Claim(r) \/ Launch(r) \/ PrepareTerminal(r) \/ Commit(r) \/
         ForgetWrite(r) \/ Dispatch(r) \/ Receive(r) \/ LoseReply(r) \/
         Consume(r) \/ BlockForFuel(r) \/ Ack(r) \/ Cancel(r) \/ Drain(r) \/
         Release(r) \/ ExpireResponse(r) \/
         (\E kind \in OutcomeKinds, value \in Values :
             Prepare(r, kind, value))
    \/ \E q \in flight, value \in Values : RemoteComplete(q, value)
    \/ ExpireSession \/ Crash \/ Restart

TypeOK ==
    /\ db.admitted \in 0..MaxRuns /\ db.keys \in [Keys -> 0..MaxRuns]
    /\ process.generation \in 1..(MaxCrashes + 1)
    /\ process.online \in BOOLEAN /\ db.closed \in BOOLEAN
    /\ \A r \in Runs :
         /\ db.runs[r].state \in {"absent", "accepted", "running", "blocked",
                                  "completed", "cancelled", "indeterminate"}
         /\ db.runs[r].spent \in 0..FuelLimit
         /\ Len(db.runs[r].history) <= MaxSteps
         /\ process.local[r].step \in 1..(MaxSteps + 1)
         /\ process.local[r].replayThrough \in 0..MaxSteps
         /\ process.local[r].seen \in UNION {[1..n -> Values] : n \in 0..MaxSteps}

RequestSetsDisjoint == flight \cap {f.request : f \in finished} = {}
AdmissionUnique ==
    \A r, q \in 1..db.admitted : db.runs[r].key = db.runs[q].key => r = q
KeyReserved ==
    \A r \in 1..db.admitted : db.keys[db.runs[r].key] = r
ClientResponseMatches ==
    ~monitor.badClientResponse
HistoryConsistent ==
    \A r \in Runs :
      /\ db.runs[r].next = Len(db.runs[r].history) + 1
      /\ db.runs[r].spent = Charged(r)
      /\ \A i \in 1..Len(db.runs[r].history) :
           LET e == db.runs[r].history[i] IN
           /\ e.seq = i
           /\ e.args = ExpectedArguments(r, e.step)
           /\ e.key = EffectKey(r, e.step)
           /\ e.kind \in OutcomeKinds
           /\ e.step = i
EffectHistoryWellFormed ==
    \A r \in Runs, step \in 1..MaxSteps :
      /\ Cardinality(Events(r, step, OutcomeKinds)) <= 1
      /\ IF IsObservation(step)
         THEN Events(r, step, {"outcome"}) = {}
         ELSE Events(r, step, {"observation"}) = {}
      /\ ((HasOutcome(r, step) /\ ~IsObservation(step)) =>
           \E p \in provider :
                p.key = EffectKey(r, step) /\
                p.args = Outcome(r, step).args /\ p.value = Outcome(r, step).value)
ReplayMatches ==
    \A r \in Runs :
      /\ process.local[r].step = Len(process.local[r].seen) + 1
      /\ \A i \in 1..Len(process.local[r].seen) :
           HasOutcome(r, i) /\ process.local[r].seen[i] = Outcome(r, i).value
NoDuplicateMutation ==
    Idempotent => \A r \in Runs, s \in 1..MaxSteps :
        monitor.mutations[r][s] <= 1
ProviderArgumentsMatch ==
    \A p \in provider : p.args = ExpectedArguments(p.run, p.step)
CommitBarriers ==
    ~monitor.repeatedDispatch /\
    ~monitor.earlyConsume /\ ~monitor.earlyAck
StableEffectIdentity == ~monitor.badIdentity
NoStaleWrites == ~monitor.staleWrite
SessionOrder == ~monitor.outOfOrder
ResourcesProtected ==
    \A r \in Runs :
      (db.runs[r].state \notin Terminal \cup {"absent"} \/
       process.local[r].active \/ InFlight(r)) => db.runs[r].pinned
TerminalConsistent ==
    /\ \A r \in Runs : db.runs[r].state = "completed" =>
         /\ db.runs[r].response = PreviousOutcomes(r, MaxSteps + 1)
         /\ Len(db.runs[r].history) = MaxSteps
    /\ db.lastResult =
         IF {r \in Runs : db.runs[r].state = "completed"} = {} THEN 0
         ELSE CHOOSE r \in Runs :
             db.runs[r].state = "completed" /\
             \A q \in Runs : db.runs[q].state = "completed" => q <= r
LegacyNotRecovered ==
    LegacyKey => db.runs[1].state = "indeterminate" /\ db.runs[1].history = <<>>
HistoryPrefix ==
    \A r \in Runs :
      SubSeq(db'.runs[r].history, 1, Len(db.runs[r].history)) = db.runs[r].history
AppendOnly == [][HistoryPrefix]_vars
NeverWitness == ~witnessed

Spec == Init /\ [][Next]_vars

\* Strong fairness here means transactions/replies cannot fail forever.
\* There are finitely many crashes; recovery has no dependency on a client retry.
FairRun(r) ==
    /\ WF_vars(Claim(r)) /\ WF_vars(Launch(r))
    /\ SF_vars(\E k \in OutcomeKinds, v \in Values :
                   Prepare(r, k, v))
    /\ SF_vars(Commit(r)) /\ WF_vars(ForgetWrite(r))
    /\ SF_vars(Dispatch(r)) /\ SF_vars(Receive(r))
    /\ WF_vars(Consume(r)) /\ SF_vars(PrepareTerminal(r))
    /\ WF_vars(BlockForFuel(r)) /\ WF_vars(Drain(r)) /\ WF_vars(Release(r))
FairRemote ==
    \A r \in Runs, s \in 1..MaxSteps, g \in 1..(MaxCrashes + 1) :
       WF_vars(\E q \in flight, v \in Values :
           q.run = r /\ q.step = s /\ q.generation = g /\ RemoteComplete(q, v))
LiveSpec ==
    Spec /\ WF_vars(Restart) /\ FairRemote /\ (\A r \in Runs : FairRun(r))
Progress ==
    \A r \in Runs : (db.runs[r].state \in {"accepted", "running"})
        ~> (db.runs[r].state \in Terminal \cup {"blocked"})
CleanupProgress ==
    \A r \in Runs : (db.runs[r].state \in Terminal)
        ~> (~process.local[r].active /\ ~InFlight(r))
ReleaseProgress ==
    Lifecycle => \A r \in Runs : (db.runs[r].state \in Terminal)
        ~> ~db.runs[r].pinned
=============================================================================
