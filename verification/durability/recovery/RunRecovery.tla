----------------------------- MODULE RunRecovery -----------------------------
EXTENDS Naturals, Sequences, FiniteSets

\* Scheduler/lifecycle composition. DurableExecution models native dispatch and
\* immutable outcomes; filesystem models establish retry-safe adapter progress.
CONSTANTS Capacity, Fault
Runs == 1..3
Sessions == 1..2
Session(r) == IF r <= 2 THEN 1 ELSE 2
Terminal == {"completed", "cancelled"}
VARIABLES state, attempt, owner, boot, online, live, history, progress, consumed,
          closed, pinned, resumes, badLaunch, badResume, badOrder, witness
vars == <<state, attempt, owner, boot, online, live, history, progress, consumed,
          closed, pinned, resumes, badLaunch, badResume, badOrder, witness>>

Init ==
    /\ state = [r \in Runs |-> "absent"]
    /\ attempt = [r \in Runs |-> 0]
    /\ owner = [r \in Runs |-> 0]
    /\ boot = 1 /\ online = TRUE /\ live = {}
    /\ history = [r \in Runs |-> 0]
    /\ progress = [r \in Runs |-> 0]
    /\ consumed = [r \in Runs |-> 0]
    /\ closed = {} /\ pinned = {}
    /\ resumes = [r \in Runs |-> 0]
    /\ badLaunch = FALSE /\ badResume = FALSE /\ badOrder = FALSE
    /\ witness = FALSE

EarlierDone(r) == \A q \in Runs :
    (q < r /\ Session(q) = Session(r)) =>
        (state[q] \in Terminal /\ q \notin live)

Admit(r) ==
    /\ online /\ Session(r) \notin closed /\ state[r] = "absent"
    /\ state' = [state EXCEPT ![r] = "queued"]
    /\ pinned' = pinned \cup {r}
    /\ UNCHANGED <<attempt, owner, boot, online, live, history, progress,
                   consumed, closed, resumes, badLaunch, badResume, badOrder, witness>>

Claim(r) ==
    /\ online /\ Session(r) \notin closed /\ r \notin live
    /\ Cardinality(live) < Capacity /\ attempt[r] < 2
    /\ (EarlierDone(r) \/ Fault = "overtake")
    /\ (state[r] = "queued" \/ (state[r] = "running" /\ owner[r] < boot))
    /\ state' = [state EXCEPT ![r] = "running"]
    /\ attempt' = [attempt EXCEPT ![r] = @ + 1]
    /\ owner' = [owner EXCEPT ![r] = boot]
    /\ consumed' = [consumed EXCEPT ![r] = 0]
    /\ badOrder' = (badOrder \/ ~EarlierDone(r))
    /\ UNCHANGED <<boot, online, live, history, progress, closed, pinned,
                   resumes, badLaunch, badResume, witness>>

Launch(r) ==
    /\ online /\ state[r] = "running" /\ owner[r] = boot
    /\ Cardinality(live) < Capacity
    /\ (r \notin live \/ Fault = "duplicate-launch")
    /\ live' = live \cup {r}
    /\ badLaunch' = (badLaunch \/ r \in live)
    /\ UNCHANGED <<state, attempt, owner, boot, online, history, progress,
                   consumed, closed, pinned, resumes, badResume, badOrder, witness>>

\* Adapter progress may precede the outer history commit. Retrying that effect
\* does not advance twice, and replayed outcomes do not invoke the adapter.
Execute(r) ==
    /\ online /\ r \in live /\ state[r] = "running"
    /\ consumed[r] = history[r] /\ history[r] < 2
    /\ progress' = [progress EXCEPT ![r] = history[r] + 1]
    /\ UNCHANGED <<state, attempt, owner, boot, online, live, history, consumed,
                   closed, pinned, resumes, badLaunch, badResume, badOrder, witness>>

CommitOutcome(r) ==
    /\ online /\ r \in live /\ state[r] = "running"
    /\ progress[r] = history[r] + 1
    /\ history' = [history EXCEPT ![r] = progress[r]]
    /\ UNCHANGED <<state, attempt, owner, boot, online, live, progress, consumed,
                   closed, pinned, resumes, badLaunch, badResume, badOrder, witness>>

Consume(r) ==
    /\ online /\ r \in live /\ state[r] = "running"
    /\ consumed[r] < history[r]
    /\ consumed' = [consumed EXCEPT ![r] = @ + 1]
    /\ UNCHANGED <<state, attempt, owner, boot, online, live, history, progress,
                   closed, pinned, resumes, badLaunch, badResume, badOrder, witness>>

Complete(r) ==
    /\ online /\ r \in live /\ state[r] = "running" /\ consumed[r] = 2
    /\ state' = [state EXCEPT ![r] = "completed"]
    /\ witness' = (witness \/ (r = 3 /\ state[1] = "stuck"))
    /\ UNCHANGED <<attempt, owner, boot, online, live, history, progress, consumed,
                   closed, pinned, resumes, badLaunch, badResume, badOrder>>

Stick(r) ==
    /\ online /\ state[r] = "running"
    /\ state' = [state EXCEPT ![r] = "stuck"]
    /\ UNCHANGED <<attempt, owner, boot, online, live, history, progress, consumed,
                   closed, pinned, resumes, badLaunch, badResume, badOrder, witness>>

Drain(r) ==
    /\ online /\ r \in live /\ state[r] # "running"
    /\ live' = live \ {r}
    /\ UNCHANGED <<state, attempt, owner, boot, online, history, progress, consumed,
                   closed, pinned, resumes, badLaunch, badResume, badOrder, witness>>

Resume(r) ==
    /\ online /\ state[r] = "stuck" /\ r \notin live
    /\ Session(r) \notin closed /\ resumes[r] = 0
    /\ state' = [state EXCEPT ![r] = "queued"]
    /\ resumes' = [resumes EXCEPT ![r] = @ + 1]
    /\ UNCHANGED <<attempt, owner, boot, online, live, history, progress, consumed,
                   closed, pinned, badLaunch, badResume, badOrder, witness>>

Close(s) ==
    /\ online /\ s \notin closed
    /\ closed' = closed \cup {s}
    /\ state' = [r \in Runs |-> IF Session(r) = s /\
                  state[r] \in {"queued", "running", "stuck"}
                  THEN "cancelled" ELSE state[r]]
    /\ UNCHANGED <<attempt, owner, boot, online, live, history, progress, consumed,
                   pinned, resumes, badLaunch, badResume, badOrder, witness>>

Retire(r) ==
    /\ online /\ r \in pinned
    /\ ((state[r] \in Terminal /\ r \notin live) \/ Fault = "early-retire")
    /\ pinned' = pinned \ {r}
    /\ UNCHANGED <<state, attempt, owner, boot, online, live, history, progress,
                   consumed, closed, resumes, badLaunch, badResume, badOrder, witness>>

Crash ==
    /\ online /\ boot = 1
    /\ online' = FALSE /\ live' = {}
    /\ UNCHANGED <<state, attempt, owner, boot, history, progress, consumed,
                   closed, pinned, resumes, badLaunch, badResume, badOrder, witness>>
Restart ==
    /\ ~online /\ online' = TRUE /\ boot' = 2
    /\ state' = [r \in Runs |-> IF Fault = "auto-resume" /\ state[r] = "stuck"
                                THEN "queued" ELSE state[r]]
    /\ badResume' = (badResume \/ (Fault = "auto-resume" /\
                                     \E r \in Runs : state[r] = "stuck"))
    /\ UNCHANGED <<attempt, owner, live, history, progress, consumed, closed,
                   pinned, resumes, badLaunch, badOrder, witness>>

Next == (\E r \in Runs : Admit(r) \/ Claim(r) \/ Launch(r) \/ Execute(r) \/
          CommitOutcome(r) \/ Consume(r) \/ Complete(r) \/ Stick(r) \/ Drain(r) \/
          Resume(r) \/ Retire(r)) \/ (\E s \in Sessions : Close(s)) \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars
SingleLaunch == ~badLaunch /\ Cardinality(live) <= Capacity
ManualResumeOnly == ~badResume
SessionOrder == ~badOrder
ResourcesProtected == \A r \in Runs :
    (state[r] \in {"queued", "running", "stuck"} \/ r \in live) => r \in pinned
ProgressBounded == \A r \in Runs : consumed[r] <= history[r] /\
    history[r] <= progress[r] /\ progress[r] <= history[r] + 1 /\ progress[r] <= 2
TerminalCommitted == \A r \in Runs : state[r] = "completed" => history[r] = 2
NoWitness == ~witness
=============================================================================
