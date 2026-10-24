---------------------------- MODULE ToolCallbacks ----------------------------
EXTENDS Naturals, Sequences, FiniteSets

\* SUB-1695: one keyed run, several sequential tools, and independent HTTP
\* observers. Only the run, completed effects and terminal result survive a
\* server crash. Pending calls, received answers and HTTP waiters do not.
\* RunRecovery supplies exclusive attempt ownership; DurableExecution supplies
\* the generic journal commit. This model expands their application-tool seam.
CONSTANTS MaxCalls, MaxCrashes, AllowTimeouts, Fault, Witness
ASSUME /\ MaxCalls \in Nat \ {0} /\ MaxCrashes \in Nat
       /\ AllowTimeouts \in BOOLEAN
       /\ Fault \in {"none", "duplicate-result", "late-result", "duplicate-run",
                     "retry-running", "provider-counter", "wrong-call",
                     "early-ack", "replay-ack", "skip-replay", "terminal-needs-history"}

Calls == 1..MaxCalls
Values == {"a", "b"}
Terminal == {"completed", "expired"}
CallId(run, effect) == <<run, effect>>
NoCall == [id |-> <<0, 0>>, effect |-> 0, args |-> <<>>]
NoOutcome == [id |-> <<0, 0>>, effect |-> 0, args |-> <<>>, value |-> "none"]
Answer(id, value) == [id |-> id, value |-> value]
Outcome(call, value) == [id |-> call.id, effect |-> call.effect,
                         args |-> call.args, value |-> value]
EmptyMemory == [phase |-> "idle", seen |-> <<>>, pending |-> NoCall,
                candidate |-> NoOutcome, requests |-> {}, published |-> 0,
                timedOut |-> {}]

VARIABLES db, memory, client, online, crashes, audit, witnessed
vars == <<db, memory, client, online, crashes, audit, witnessed>>

Init ==
    /\ db = [run |-> 0, state |-> "absent", history |-> <<>>, result |-> <<>>]
    /\ memory = EmptyMemory
    /\ client = [known |-> {}, answers |-> {}]
    /\ online = TRUE /\ crashes = 0
    \* Ghost evidence never enables protocol actions or reconstructs execution.
    /\ audit = [commits |-> <<>>, acknowledged |-> {},
                 badView |-> FALSE, badTerminal |-> FALSE, badUnresolved |-> FALSE,
                 lostCall |-> NoCall, crashedCall |-> NoCall,
                 crashPoint |-> "none", recovered |-> {}]
    /\ witnessed = FALSE

Saw(name, condition) == witnessed \/ (Witness = name /\ condition)
NextEffect == Len(memory.seen) + 1
Entries(id) == {db.history[i] : i \in {j \in 1..Len(db.history) : db.history[j].id = id}}
Committed(id) == Entries(id) # {}
Saved(id) == CHOOSE e \in Entries(id) : TRUE
HasTimedOut == \E i \in 1..Len(db.history) : db.history[i].value = "timeout"
PendingCall ==
    [id |-> CallId(db.run, IF Fault = "provider-counter"
                          THEN memory.published + 1 ELSE NextEffect),
     effect |-> NextEffect, args |-> <<db.run, NextEffect, memory.seen>>]
View == CASE db.state = "completed" -> [kind |-> "finished", call |-> NoCall, result |-> db.result]
          [] db.state = "expired" -> [kind |-> "expired", call |-> NoCall, result |-> <<>>]
          [] memory.phase = "waiting" -> [kind |-> "suspended", call |-> memory.pending, result |-> <<>>]
          [] OTHER -> [kind |-> "running", call |-> NoCall, result |-> <<>>]

\* Every Execute uses the same idempotency key and source. Observation and
\* response delivery are separate: delivered=FALSE loses the HTTP response.
Execute(delivered) ==
    /\ online
    /\ IF db.state = "absent" \/ (Fault = "duplicate-run" /\ db.state = "running" /\ db.run = 1)
       THEN /\ db' = [db EXCEPT !.run = @ + 1, !.state = "running"]
            /\ UNCHANGED <<memory, client, audit, witnessed>>
       ELSE LET response == IF Fault = "retry-running" /\ View.kind = "suspended"
                            THEN [View EXCEPT !.kind = "running", !.call = NoCall]
                            ELSE View
            IN /\ client' = IF delivered /\ response.kind = "suspended"
                            THEN [client EXCEPT !.known = @ \cup {response.call}] ELSE client
               /\ audit' = [audit EXCEPT
                    !.badView = @ \/ (delivered /\ response # View),
                    !.lostCall = IF ~delivered /\ response.kind = "suspended"
                                 THEN response.call ELSE @]
               /\ witnessed' = (
                    Saw("repeat-execute", delivered /\ response.kind = "suspended" /\ response.call \in client.known)
                    \/ Saw("lost-execute-response", delivered /\ response.kind = "suspended" /\ response.call = audit.lostCall))
               /\ UNCHANGED <<db, memory>>
    /\ UNCHANGED <<online, crashes>>

\* The supervisor starts/restarts without any connected client. Replay starts
\* from the beginning, and reconstructs arguments from committed outcomes.
Recover ==
    /\ online /\ db.state = "running" /\ memory.phase = "idle"
    /\ memory' = [memory EXCEPT !.phase = "ready"]
    /\ UNCHANGED <<db, client, online, crashes, audit, witnessed>>

Publish ==
    /\ online /\ db.state = "running" /\ memory.phase = "ready"
    /\ NextEffect <= MaxCalls /\ NextEffect > Len(db.history)
    /\ memory' = [memory EXCEPT
         !.pending = PendingCall, !.published = @ + 1,
         !.phase = IF HasTimedOut THEN "answer" ELSE "waiting",
         !.candidate = IF HasTimedOut THEN Outcome(PendingCall, "timeout") ELSE NoOutcome]
    /\ audit' = [audit EXCEPT !.recovered =
         IF crashes > 0 /\ PendingCall = audit.crashedCall
         THEN @ \cup {PendingCall.id} ELSE @]
    /\ witnessed' = (Saw("recover-wait", crashes > 0 /\ PendingCall = audit.crashedCall)
         \/ Saw("recover-second-call", PendingCall.effect > 1 /\ PendingCall = audit.crashedCall))
    /\ UNCHANGED <<db, client, online, crashes>>

\* The client may finish its tool while the server is down. Its cached answer
\* survives a SERVER crash. Client-process durability is outside this model.
ClientAnswer(call, value) ==
    /\ call \in client.known /\ value \in Values
    /\ ~\E a \in client.answers : a.id = call.id
    /\ client' = [client EXCEPT !.answers = @ \cup {Answer(call.id, value)}]
    /\ UNCHANGED <<db, memory, online, crashes, audit, witnessed>>

\* Identical retries, conflicting duplicates, and invalid identities all use
\* the same endpoint. Two values allow a conflicting second submission.
Submissions == client.answers
    \cup {Answer(a.id, IF a.value = "a" THEN "b" ELSE "a") : a \in client.answers}
    \cup {Answer(CallId(2, c.effect), "a") : c \in client.known}
    \cup {Answer(CallId(1, MaxCalls + 1), "a") : c \in client.known}

Submit(a) ==
    /\ online /\ a \in Submissions
    /\ memory' = [memory EXCEPT !.requests = @ \cup {a}]
    /\ witnessed' = (Saw("duplicate-before-commit", a \in memory.requests /\
                          memory.candidate.id = a.id /\ ~Committed(a.id))
                    \/ Saw("resume-during-recovery", crashes > 0 /\ memory.phase = "idle"))
    /\ UNCHANGED <<db, client, online, crashes, audit>>

Deliver(a) ==
    /\ online /\ db.state = "running" /\ a \in memory.requests
    /\ (memory.phase = "waiting" \/
        (Fault = "late-result" /\ memory.phase = "answer" /\ memory.candidate.value = "timeout"))
    /\ (a.id = memory.pending.id \/ Fault = "wrong-call")
    /\ memory' = [memory EXCEPT !.phase = "answer",
         !.candidate = [Outcome(memory.pending, a.value) EXCEPT !.id = a.id]]
    /\ UNCHANGED <<db, client, online, crashes, audit, witnessed>>

\* Timeout selection and its ordinary effect commit are separate. This does
\* not assume a durable pending row or a persisted wall-clock deadline.
Timeout ==
    /\ AllowTimeouts /\ online /\ db.state = "running" /\ memory.phase = "waiting"
    /\ memory' = [memory EXCEPT !.phase = "answer",
         !.candidate = Outcome(memory.pending, "timeout"),
         !.timedOut = @ \cup {memory.pending.effect}]
    /\ UNCHANGED <<db, client, online, crashes, audit, witnessed>>

Commit ==
    /\ online /\ db.state = "running" /\ memory.phase = "answer"
    /\ memory.candidate.effect = Len(db.history) + 1
    /\ db' = [db EXCEPT !.history = Append(@, memory.candidate)]
    /\ memory' = [memory EXCEPT !.phase = "committed"]
    /\ audit' = [audit EXCEPT !.commits = Append(@, memory.candidate)]
    /\ witnessed' = (
         Saw("resume-after-recovery", memory.candidate.id \in audit.recovered /\ memory.candidate.value \in Values)
         \/ Saw("retry-uncommitted-answer", audit.crashPoint = "answer" /\
                 memory.candidate.id = audit.crashedCall.id /\ memory.candidate.value \in Values))
    /\ UNCHANGED <<client, online, crashes>>

\* Negative control: a retry appends another outcome for the same logical call.
DuplicateCommit(a) ==
    /\ Fault = "duplicate-result" /\ online /\ db.state = "running"
    /\ a \in memory.requests /\ Committed(a.id)
    /\ LET entry == [Saved(a.id) EXCEPT !.value = a.value]
       IN /\ db' = [db EXCEPT !.history = Append(@, entry)]
          /\ audit' = [audit EXCEPT !.commits = Append(@, entry)]
    /\ UNCHANGED <<memory, client, online, crashes, witnessed>>

Consume ==
    /\ online /\ db.state = "running" /\ memory.phase \in {"ready", "committed"}
    /\ NextEffect <= Len(db.history)
    /\ LET value == db.history[NextEffect].value
           replayed == IF Fault = "skip-replay" /\ crashes > 0
                       THEN IF value = "a" THEN "b" ELSE "a" ELSE value
       IN memory' = [memory EXCEPT !.seen = Append(@, replayed),
                     !.phase = "ready", !.pending = NoCall, !.candidate = NoOutcome]
    /\ UNCHANGED <<db, client, online, crashes, audit, witnessed>>

Complete ==
    /\ online /\ db.state = "running" /\ memory.phase = "ready"
    /\ Len(memory.seen) = MaxCalls
    /\ db' = [db EXCEPT !.state = "completed", !.result = memory.seen]
    /\ memory' = [memory EXCEPT !.phase = "done"]
    /\ UNCHANGED <<client, online, crashes, audit, witnessed>>

\* Terminal lookup comes BEFORE effect lookup. No callback history has to be
\* retained once the ordinary idempotent run result is available.
Reply(a) ==
    /\ online /\ a \in memory.requests
    /\ LET terminal == db.state \in Terminal
           saved == Committed(a.id)
           timingOut == memory.candidate.id = a.id /\ memory.candidate.value = "timeout"
           early == Fault = "early-ack" /\ memory.candidate.id = a.id /\ ~saved /\ ~timingOut
           replayAck == Fault = "replay-ack" /\ memory.phase \in {"idle", "ready"} /\ ~saved
           invalid == memory.phase = "waiting" /\ a.id # memory.pending.id /\ ~saved
           accepted == ~terminal /\ ~timingOut /\ (IF saved THEN Saved(a.id).value \in Values ELSE early)
           observed == IF terminal /\ Fault = "terminal-needs-history" /\ ~saved
                       THEN [kind |-> "unknown_call", call |-> NoCall, result |-> <<>>]
                       ELSE IF terminal THEN View
                       ELSE [kind |-> IF accepted THEN "accepted" ELSE "refused",
                             call |-> NoCall, result |-> <<>>]
       IN /\ (terminal \/ saved \/ timingOut \/ invalid \/ early \/ replayAck)
          /\ memory' = [memory EXCEPT !.requests = @ \ {a}]
          /\ audit' = [audit EXCEPT
               !.acknowledged = IF accepted
                   THEN @ \cup {IF saved THEN Saved(a.id) ELSE memory.candidate} ELSE @,
               !.badTerminal = @ \/ (terminal /\ observed # View),
               !.badUnresolved = @ \/ (~terminal /\ ~saved /\ ~timingOut /\ ~invalid)]
          /\ witnessed' = (
               Saw("duplicate-after-commit", saved /\ accepted /\ Saved(a.id) \in audit.acknowledged)
               \/ Saw("conflicting-duplicate", saved /\ accepted /\ Saved(a.id).value # a.value)
               \/ Saw("old-answer-next-call", saved /\ accepted /\ memory.phase = "waiting" /\
                       Saved(a.id).effect < memory.pending.effect)
               \/ Saw("late-resume", ~terminal /\ (timingOut \/ (saved /\ Saved(a.id).value = "timeout")))
               \/ Saw("timeout-after-restart", audit.crashPoint = "committed" /\
                       a.id = audit.crashedCall.id /\ saved /\ Saved(a.id).value = "timeout")
               \/ Saw("lost-resume-response", audit.crashPoint = "committed" /\ saved /\ accepted /\ a.id = audit.crashedCall.id)
               \/ Saw("terminal-without-history", terminal /\ db.history = <<>> /\ observed.kind = "finished")
               \/ Saw("expired-result", terminal /\ observed.kind = "expired"))
    /\ UNCHANGED <<db, client, online, crashes>>

\* Dropping HTTP waiters never cancels execution or the in-memory answer.
Disconnect ==
    /\ online /\ memory.requests # {}
    /\ memory' = [memory EXCEPT !.requests = {}]
    /\ UNCHANGED <<db, client, online, crashes, audit, witnessed>>

RetireHistory ==
    /\ online /\ db.state \in Terminal /\ db.history # <<>>
    /\ db' = [db EXCEPT !.history = <<>>]
    /\ UNCHANGED <<memory, client, online, crashes, audit, witnessed>>

ExpireResult ==
    /\ online /\ db.state = "completed"
    /\ db' = [db EXCEPT !.state = "expired", !.result = <<>>]
    /\ UNCHANGED <<memory, client, online, crashes, audit, witnessed>>

Crash ==
    /\ online /\ db.state # "absent" /\ crashes < MaxCrashes
    /\ online' = FALSE /\ crashes' = crashes + 1 /\ memory' = EmptyMemory
    /\ audit' = [audit EXCEPT !.crashedCall = memory.pending,
         !.crashPoint = CASE memory.phase = "answer" /\ memory.candidate.value = "timeout" -> "timeout"
                         [] memory.phase = "committed" /\ memory.candidate \in audit.acknowledged -> "acknowledged"
                         [] OTHER -> memory.phase]
    /\ UNCHANGED <<db, client, witnessed>>

Restart ==
    /\ ~online /\ online' = TRUE
    /\ UNCHANGED <<db, memory, client, crashes, audit, witnessed>>

Next == Execute(TRUE) \/ Execute(FALSE) \/ Recover \/ Publish \/ Timeout
    \/ Commit \/ Consume \/ Complete \/ Disconnect \/ RetireHistory \/ ExpireResult
    \/ Crash \/ Restart
    \/ (\E call \in client.known, value \in Values : ClientAnswer(call, value))
    \/ (\E a \in Submissions : Submit(a))
    \/ (\E a \in memory.requests : Deliver(a) \/ Reply(a) \/ DuplicateCommit(a))

TypeOK ==
    /\ db.run \in 0..2 /\ db.state \in {"absent", "running", "completed", "expired"}
    /\ online \in BOOLEAN /\ witnessed \in BOOLEAN /\ crashes \in 0..MaxCrashes
    /\ memory.phase \in {"idle", "ready", "waiting", "answer", "committed", "done"}
    /\ Len(memory.seen) <= MaxCalls /\ Len(audit.commits) <= MaxCalls + 1
    /\ memory.timedOut \subseteq Calls

OneRunPerKey == db.run <= 1
OneResultPerCall == \A effect \in Calls :
    Cardinality({i \in 1..Len(audit.commits) : audit.commits[i].effect = effect}) <= 1
TimeoutNotReplaced ==
    /\ (memory.candidate.effect \in memory.timedOut => memory.candidate.value = "timeout")
    /\ \A i, j \in 1..Len(audit.commits) :
         (audit.commits[i].effect = audit.commits[j].effect /\ audit.commits[i].value = "timeout")
         => audit.commits[j].value = "timeout"
StableCallIdentity == memory.pending # NoCall =>
    memory.pending.id = CallId(db.run, memory.pending.effect)
CorrectDelivery == memory.candidate # NoOutcome =>
    memory.candidate.id = CallId(db.run, memory.candidate.effect)
ExecuteReturnsCurrentState == ~audit.badView
TerminalReturnsStoredResult == ~audit.badTerminal
\* Even a successful "running" reply during replay would violate this: an
\* HTTP submission must first reach a stored, duplicate, or rejected decision.
SubmissionResolvedBeforeReply == ~audit.badUnresolved
AcknowledgedCommitted == \A e \in audit.acknowledged :
    \E i \in 1..Len(audit.commits) : audit.commits[i] = e
ReplayMatches ==
    /\ \A i \in 1..Len(memory.seen) : memory.seen[i] = audit.commits[i].value
    /\ memory.pending # NoCall =>
         memory.pending.args = <<db.run, memory.pending.effect,
             [i \in 1..(memory.pending.effect - 1) |-> audit.commits[i].value]>>
TerminalConsistent == db.state = "completed" =>
    /\ Len(audit.commits) = MaxCalls
    /\ db.result = [i \in 1..MaxCalls |-> audit.commits[i].value]
NeverWitness == ~witnessed

Spec == Init /\ [][Next]_vars
\* Fairness means eventual server availability, successful local commits, and
\* a client that eventually observes, computes and retries its cached answers.
\* Recovery itself has no dependency on those client actions.
LocalFairness == WF_vars(Restart) /\ WF_vars(Recover) /\ WF_vars(Publish)
    /\ WF_vars(Commit) /\ WF_vars(Consume) /\ WF_vars(Complete)
RecoverySpec == Spec /\ LocalFairness
RecoveryProgress == (db.state = "running" /\ memory.phase = "idle")
    ~> (memory.phase = "waiting" \/ db.state \in Terminal)
LiveSpec == RecoverySpec /\ WF_vars(Execute(TRUE))
    /\ WF_vars(\E call \in client.known, value \in Values : ClientAnswer(call, value))
    /\ (\A effect \in Calls, value \in Values :
          LET a == Answer(CallId(1, effect), value)
          IN SF_vars(Submit(a)) /\ SF_vars(Deliver(a)))
RunProgress == db.state = "running" ~> db.state \in Terminal
=============================================================================
