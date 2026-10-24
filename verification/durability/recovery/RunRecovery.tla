----------------------------- MODULE RunRecovery -----------------------------
EXTENDS Naturals, FiniteSets

\* Single-node restart recovery: how the supervisor in
\* crates/submilli-server/src/recovery.rs claims, launches, reconciles, resumes
\* and retires durable runs around process crashes and session closure.
\* DurableExecution models native dispatch and immutable outcomes; the
\* filesystem models establish retry-safe adapter progress. This model reduces
\* both to one counter each and checks their composition with the scheduler.
\* Invariants are predicates over the state variables; each negative variant
\* disables one rule (`redispatch` two). Four ghost variables and the witness
\* latch record facts for invariants and witnesses: whether the run's latest
\* claim was launched, runs that entered stuck and were not resumed, history
\* at cancellation, and the attempt that made adapter progress.

CONSTANTS Capacity, MaxBoot, MaxAttempt, Legacy, Fault, Witness

\* Capacity assumes sessions have disjoint writable named volumes. The Rust
\* scheduler additionally excludes overlapping writers until their tasks drain
\* and recovers persisted running writers before queued ones. Stuck runs release
\* that exclusion; safe resume after intervening writes needs advisory locking.

ASSUME /\ Capacity \in Nat \ {0} /\ MaxBoot \in Nat \ {0} /\ MaxAttempt \in Nat \ {0}
       /\ Legacy \in BOOLEAN
       /\ Fault \in {"none", "duplicate-claim", "redispatch", "stale-launch",
                    "stuck-while-live", "auto-resume", "overtake", "early-retire",
                    "stale-commit"}
       /\ Witness \in {"none", "reclaim-after-crash", "retry-commits-prior-progress",
                      "fenced-task-drains", "unrelated-session-progress",
                      "legacy-successor-completed"}

Runs == 1..3
Sessions == 1..2
Session(r) == IF r <= 2 THEN 1 ELSE 2
Steps == 2
Terminal == {"completed", "cancelled"}
Retained == {"queued", "running", "stuck"}
OrderingFinished == Terminal \cup {"indeterminate"}
Attempts == 1..(MaxAttempt + 1)

VARIABLES
    \* Committed in SQLite: lifecycle, attempt fence, owning startup, outer
    \* history, adapter progress, closed sessions and retained resources.
    state, attempt, owner, history, progress, closed, pinned,
    \* Volatile: the current startup, its live attempt tasks and each run's
    \* replay cursor. A crash erases these.
    boot, online, live, consumed,
    \* Bound and ghosts: explicit resumes per run, whether the run's latest
    \* claim was launched, runs stuck and not resumed, history at cancellation,
    \* the attempt that made adapter progress, and the witness latch.
    resumes, launched, unresumed, frozen, progressBy, witness
vars == <<state, attempt, owner, history, progress, closed, pinned,
          boot, online, live, consumed,
          resumes, launched, unresumed, frozen, progressBy, witness>>

Init ==
    /\ state = [r \in Runs |-> IF Legacy /\ r = 1 THEN "indeterminate" ELSE "absent"]
    /\ attempt = [r \in Runs |-> 0]
    /\ owner = [r \in Runs |-> 0]
    /\ history = [r \in Runs |-> 0]
    /\ progress = [r \in Runs |-> 0]
    /\ closed = {} /\ pinned = {}
    /\ boot = 1 /\ online = TRUE /\ live = {}
    /\ consumed = [r \in Runs |-> 0]
    /\ resumes = [r \in Runs |-> 0]
    /\ launched = [r \in Runs |-> FALSE] /\ unresumed = {}
    /\ frozen = [r \in Runs |-> 0]
    /\ progressBy = [r \in Runs |-> 0]
    /\ witness = FALSE

Live(r) == \E a \in Attempts : <<r, a>> \in live
Earlier(r) == {q \in Runs : q < r /\ Session(q) = Session(r)}
\* has_run_predecessor: an earlier run of the session that is queued, running,
\* or stuck blocks a claim. Indeterminate runs have ended without a recoverable
\* outcome. A predecessor whose task has not drained still blocks the session.
EarlierDone(r) == \A q \in Earlier(r) : state[q] \in OrderingFinished /\ ~Live(q)
ClosingCancels(s, r) == Session(r) = s /\ state[r] \in Retained
AutoResumed(r) == Fault = "auto-resume" /\ state[r] = "stuck"
Saw(name, condition) == witness \/ (Witness = name /\ condition)

\* recovery::admit commits the program and the queued run in admission order.
Admit(r) ==
    /\ online /\ Session(r) \notin closed /\ state[r] = "absent"
    /\ \A q \in Earlier(r) : state[q] # "absent"
    /\ state' = [state EXCEPT ![r] = "queued"]
    /\ pinned' = pinned \cup {r}
    /\ UNCHANGED <<attempt, owner, history, progress, closed, boot, online, live,
                   consumed, resumes, launched, unresumed, frozen, progressBy, witness>>

\* recovery::claim: one transaction checks the predecessor and the session,
\* then commits a new attempt owned by this startup. A run already running
\* under this startup is never another launch permit; one running under an
\* earlier startup was interrupted by a crash and is reclaimed.
Claim(r) ==
    /\ online /\ Session(r) \notin closed
    /\ Cardinality(live) < Capacity /\ attempt[r] < MaxAttempt
    /\ (EarlierDone(r) \/ Fault = "overtake")
    /\ \/ state[r] = "queued"
       \/ state[r] = "running" /\ owner[r] < boot
       \/ Fault \in {"duplicate-claim", "redispatch"} /\ state[r] = "running"
    /\ state' = [state EXCEPT ![r] = "running"]
    /\ attempt' = [attempt EXCEPT ![r] = @ + 1]
    /\ owner' = [owner EXCEPT ![r] = boot]
    /\ launched' = [launched EXCEPT ![r] = FALSE]
    /\ witness' = Saw("reclaim-after-crash", state[r] = "running" /\ history[r] > 0)
    /\ UNCHANGED <<history, progress, closed, pinned, boot, online, live, consumed,
                   resumes, unresumed, frozen, progressBy>>

\* The supervisor spawns the task right after its claim commits, and the task
\* takes the session's resource permit before any native work. A crash in
\* between leaves a running row that the next startup reclaims. An attempt is
\* launched once: a task that ended is never started again. The replay cursor
\* starts from zero.
Launch(r) ==
    /\ online /\ state[r] = "running" /\ ~launched[r]
    /\ (~Live(r) \/ Fault = "redispatch")
    /\ (owner[r] = boot \/ Fault = "stale-launch")
    /\ Cardinality(live) < Capacity
    /\ live' = live \cup {<<r, attempt[r]>>}
    /\ consumed' = [consumed EXCEPT ![r] = 0]
    /\ launched' = [launched EXCEPT ![r] = TRUE]
    /\ UNCHANGED <<state, attempt, owner, history, progress, closed, pinned, boot,
                   online, resumes, unresumed, frozen, progressBy, witness>>

\* stop_attempt after a task ends without completing, and the schedule-time
\* rule for a claim whose acknowledgement was lost: a running run owned by this
\* startup with no live task becomes stuck. The model reduces both fences to
\* running and owner = boot.
MarkStuck(r) ==
    /\ online /\ state[r] = "running" /\ owner[r] = boot
    /\ (~Live(r) \/ Fault = "stuck-while-live")
    /\ state' = [state EXCEPT ![r] = "stuck"]
    /\ unresumed' = unresumed \cup {r}
    /\ UNCHANGED <<attempt, owner, history, progress, closed, pinned, boot, online,
                   live, consumed, resumes, launched, frozen, progressBy, witness>>

\* Native work for the next effect. Adapter progress (the filesystem journal)
\* is not fenced by run state, so a cancelled attempt's work can still land.
\* A retry of an effect whose progress already landed does not land it twice.
Execute(r, a) ==
    /\ online /\ <<r, a>> \in live
    /\ consumed[r] = history[r] /\ progress[r] = history[r] /\ history[r] < Steps
    /\ progress' = [progress EXCEPT ![r] = @ + 1]
    /\ progressBy' = [progressBy EXCEPT ![r] = a]
    /\ UNCHANGED <<state, attempt, owner, history, closed, pinned, boot, online,
                   live, consumed, resumes, launched, unresumed, frozen, witness>>

\* append_history: the outer outcome commits only for the current attempt of a
\* running run. A retry finds the adapter's receipt and commits the outcome
\* without new native work.
CommitOutcome(r, a) ==
    /\ online /\ <<r, a>> \in live
    /\ consumed[r] = history[r] /\ progress[r] = history[r] + 1
    /\ (Fault = "stale-commit" \/ (a = attempt[r] /\ state[r] = "running"))
    /\ history' = [history EXCEPT ![r] = progress[r]]
    /\ witness' = Saw("retry-commits-prior-progress", progressBy[r] # a)
    /\ UNCHANGED <<state, attempt, owner, progress, closed, pinned, boot, online,
                   live, consumed, resumes, launched, unresumed, frozen, progressBy>>

\* The guest consumes committed history only.
Consume(r, a) ==
    /\ online /\ <<r, a>> \in live /\ consumed[r] < history[r]
    /\ consumed' = [consumed EXCEPT ![r] = @ + 1]
    /\ UNCHANGED <<state, attempt, owner, history, progress, closed, pinned, boot,
                   online, live, resumes, launched, unresumed, frozen, progressBy, witness>>

\* ResolveRun::execute_recorded: terminal state, exact response and the
\* session's last result commit together, fenced by attempt and state, after
\* the whole history was consumed.
Complete(r, a) ==
    /\ online /\ <<r, a>> \in live /\ a = attempt[r] /\ state[r] = "running"
    /\ consumed[r] = Steps /\ history[r] = Steps
    /\ state' = [state EXCEPT ![r] = "completed"]
    /\ witness' = (Saw("unrelated-session-progress", r = 3 /\ state[1] = "stuck")
                    \/ Saw("legacy-successor-completed", Legacy /\ r = 2))
    /\ UNCHANGED <<attempt, owner, history, progress, closed, pinned, boot, online,
                   live, consumed, resumes, launched, unresumed, frozen, progressBy>>

\* The task ends: completion, fuel, timeout, a fence it cannot pass, or the
\* cancellation token. The runner drains native work before it returns.
Exit(r, a) ==
    /\ online /\ <<r, a>> \in live
    /\ live' = live \ {<<r, a>>}
    /\ witness' = Saw("fenced-task-drains", state[r] = "cancelled")
    /\ UNCHANGED <<state, attempt, owner, history, progress, closed, pinned, boot,
                   online, consumed, resumes, launched, unresumed, frozen, progressBy>>

\* recovery::resume: the only transition out of stuck other than cancellation.
Resume(r) ==
    /\ online /\ state[r] = "stuck" /\ Session(r) \notin closed /\ resumes[r] = 0
    /\ state' = [state EXCEPT ![r] = "queued"]
    /\ resumes' = [resumes EXCEPT ![r] = 1]
    /\ unresumed' = unresumed \ {r}
    /\ UNCHANGED <<attempt, owner, history, progress, closed, pinned, boot, online,
                   live, consumed, launched, frozen, progressBy, witness>>

\* CloseSession: the session closes and its retained runs are cancelled in one
\* transaction. Each cancelled run's attempt advances so a still-live task can
\* only drain. Indeterminate runs keep their state.
Close(s) ==
    /\ online /\ s \notin closed
    /\ closed' = closed \cup {s}
    /\ state' = [r \in Runs |-> IF ClosingCancels(s, r) THEN "cancelled" ELSE state[r]]
    /\ attempt' = [r \in Runs |-> IF ClosingCancels(s, r) THEN attempt[r] + 1 ELSE attempt[r]]
    /\ frozen' = [r \in Runs |-> IF ClosingCancels(s, r) THEN history[r] ELSE frozen[r]]
    /\ UNCHANGED <<owner, history, progress, pinned, boot, online, live, consumed,
                   resumes, launched, unresumed, progressBy, witness>>

\* recovery::retire from the scheduler or from session cleanup: private files
\* and history go only for a terminal run whose task has drained.
Retire(r) ==
    /\ online /\ r \in pinned /\ state[r] \in Terminal
    /\ (~Live(r) \/ Fault = "early-retire")
    /\ pinned' = pinned \ {r}
    /\ UNCHANGED <<state, attempt, owner, history, progress, closed, boot, online,
                   live, consumed, resumes, launched, unresumed, frozen, progressBy, witness>>

\* The process dies with its tasks. Committed rows survive.
Crash ==
    /\ online /\ boot < MaxBoot
    /\ online' = FALSE /\ live' = {}
    /\ UNCHANGED <<state, attempt, owner, history, progress, closed, pinned, boot,
                   consumed, resumes, launched, unresumed, frozen, progressBy, witness>>

\* A new startup has a fresh generation and no live tasks. It changes no run
\* state by itself: stuck runs wait for an explicit resume.
Restart ==
    /\ ~online
    /\ online' = TRUE /\ boot' = boot + 1
    /\ state' = [r \in Runs |-> IF AutoResumed(r) THEN "queued" ELSE state[r]]
    /\ UNCHANGED <<attempt, owner, history, progress, closed, pinned, live, consumed,
                   resumes, launched, unresumed, frozen, progressBy, witness>>

Next ==
    \/ \E r \in Runs :
         Admit(r) \/ Claim(r) \/ Launch(r) \/ MarkStuck(r) \/ Resume(r) \/ Retire(r)
    \/ \E r \in Runs, a \in Attempts :
         Execute(r, a) \/ CommitOutcome(r, a) \/ Consume(r, a) \/ Complete(r, a) \/ Exit(r, a)
    \/ \E s \in Sessions : Close(s)
    \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ state \in [Runs -> {"absent", "queued", "running", "stuck", "completed",
                           "cancelled", "indeterminate"}]
    /\ attempt \in [Runs -> 0..(MaxAttempt + 1)]
    /\ owner \in [Runs -> 0..MaxBoot]
    /\ history \in [Runs -> 0..Steps] /\ progress \in [Runs -> 0..Steps]
    /\ consumed \in [Runs -> 0..Steps]
    /\ boot \in 1..MaxBoot /\ online \in BOOLEAN
    /\ live \subseteq Runs \X Attempts /\ launched \in [Runs -> BOOLEAN]
    /\ closed \subseteq Sessions /\ pinned \subseteq Runs /\ unresumed \subseteq Runs
    /\ resumes \in [Runs -> 0..1]

\* Two interpreters never execute one run.
OneLiveAttemptPerRun == \A x, y \in live : x[1] = y[1] => x = y
\* A live task is the run's current attempt unless the run was cancelled under it.
NoSupersededLiveAttempt == \A p \in live : p[2] = attempt[p[1]] \/ state[p[1]] = "cancelled"
\* No task from an earlier startup survives a restart, and no task runs for a
\* row this startup has not claimed.
LiveAttemptsBelongToThisBoot == \A p \in live : online /\ owner[p[1]] = boot
WithinCapacity == Cardinality(live) <= Capacity
\* The supervisor records stuck only once the task is gone.
StuckIsInert == \A r \in Runs : state[r] = "stuck" => ~Live(r)
\* A run that entered stuck stays stuck, or is cancelled, until an explicit resume.
ResumeIsExplicit == \A r \in unresumed : state[r] \in {"stuck", "cancelled"}
\* A running or live run has only finished, drained predecessors in its session.
OrderedExecution == \A r \in Runs : (state[r] = "running" \/ Live(r)) => EarlierDone(r)
\* Private files and history stay while a run may still need them.
ResourcesProtected == \A r \in Runs : (state[r] \in Retained \/ Live(r)) => r \in pinned
\* Cancellation fences history: nothing commits after it.
HistoryFenced == \A r \in Runs : state[r] = "cancelled" => history[r] = frozen[r]
ProgressBounded == \A r \in Runs :
    /\ consumed[r] <= history[r] /\ history[r] <= progress[r]
    /\ progress[r] <= history[r] + 1 /\ progress[r] <= Steps
TerminalCommitted == \A r \in Runs : state[r] = "completed" => history[r] = Steps
\* Legacy outcomes remain unknown, without execution or durable resources.
LegacyPreserved == Legacy => /\ state[1] = "indeterminate" /\ attempt[1] = 0
                             /\ ~Live(1) /\ 1 \notin pinned
NoWitness == ~witness
=============================================================================
