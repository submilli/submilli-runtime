------------------------- MODULE HistoryObjects -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS MaxObjects, MaxCrashes, Fault, Witness
Objects == 1..MaxObjects
None == 0
VARIABLES rows, staged, synced, present, durable, writer, worker, readers,
          history, position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness
vars == <<rows, staged, synced, present, durable, writer, worker, readers,
          history, position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Init == /\ rows = [o \in Objects |-> "absent"]
        /\ staged = {} /\ synced = {} /\ present = {} /\ durable = {}
        /\ writer = None /\ worker = None /\ readers = {}
        /\ history = None /\ position = 0 /\ bytes = 0
        /\ terminal = FALSE /\ retired = FALSE /\ acknowledged = FALSE
        /\ crashes = 0 /\ crashHistory = None /\ crashOrphans = {} /\ witness = FALSE
Reserve(o) ==
    /\ ~terminal /\ history = None /\ writer = None /\ worker = None
    /\ rows[o] = "absent"
    /\ rows' = [rows EXCEPT ![o] = "pending"] /\ writer' = o
    /\ UNCHANGED <<staged, synced, present, durable, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
StartIO(o) ==
    /\ writer = o /\ worker = None /\ rows[o] = "pending" /\ o \notin durable
    /\ worker' = o /\ staged' = staged \cup {o}
    /\ UNCHANGED <<rows, synced, present, durable, writer, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
SyncFile(o) ==
    /\ worker = o /\ o \in staged /\ o \notin synced
    /\ synced' = synced \cup {o}
    /\ UNCHANGED <<rows, staged, present, durable, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Publish(o) ==
    /\ worker = o /\ o \in synced /\ o \notin present
    /\ present' = present \cup {o}
    /\ UNCHANGED <<rows, staged, synced, durable, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
SyncDirectory(o) ==
    /\ worker = o /\ o \in present /\ o \notin durable
    /\ durable' = durable \cup {o}
    /\ UNCHANGED <<rows, staged, synced, present, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
FinishIO(o) ==
    /\ worker = o /\ (o \in durable \/ Fault = "early-reference")
    /\ worker' = None
    /\ UNCHANGED <<rows, staged, synced, present, durable, writer, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Commit(o) ==
    /\ writer = o /\ worker = None /\ ~terminal /\ history = None
    /\ rows[o] = "pending"
    /\ (o \in durable \/ (Fault = "early-reference" /\ o \in present))
    /\ history' = o /\ rows' = [rows EXCEPT ![o] = "bound"]
    /\ position' = 1 /\ bytes' = 2
    /\ UNCHANGED <<staged, synced, present, durable, writer, worker, readers,
                   terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Acknowledge ==
    /\ writer # None /\ history = writer
    /\ acknowledged' = TRUE /\ writer' = None
    /\ UNCHANGED <<rows, staged, synced, present, durable, worker, readers, history,
                   position, bytes, terminal, retired, crashes, crashHistory, crashOrphans, witness>>
CancelAwaiter ==
    /\ writer # None /\ history = None
    /\ writer' = None
    /\ witness' = (witness \/ (Witness = "cancel-worker" /\ worker # None))
    /\ UNCHANGED <<rows, staged, synced, present, durable, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans>>
Read ==
    /\ history # None /\ history \notin readers
    /\ readers' = readers \cup {history}
    /\ witness' = (witness \/ (Witness = "replay-after-crash" /\ history = crashHistory /\ crashHistory # None))
    /\ UNCHANGED <<rows, staged, synced, present, durable, writer, worker, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans>>
CloseRead(o) ==
    /\ o \in readers /\ readers' = readers \ {o}
    /\ UNCHANGED <<rows, staged, synced, present, durable, writer, worker, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Terminate ==
    /\ ~terminal /\ terminal' = TRUE
    /\ UNCHANGED <<rows, staged, synced, present, durable, writer, worker, readers,
                   history, position, bytes, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Retire ==
    /\ terminal /\ ~retired /\ writer = None /\ worker = None
    /\ retired' = TRUE /\ history' = None
    /\ rows' = [o \in Objects |-> IF rows[o] \in {"bound", "pending"} THEN "deleting" ELSE rows[o]]
    /\ UNCHANGED <<staged, synced, present, durable, writer, worker, readers,
                   position, bytes, terminal, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Claim(o) ==
    /\ rows[o] = "pending"
    /\ ((writer = None /\ worker = None) \/ Fault = "collect-writer")
    /\ rows' = [rows EXCEPT ![o] = "deleting"]
    /\ UNCHANGED <<staged, synced, present, durable, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Delete(o) ==
    /\ rows[o] = "deleting" \/ (Fault = "collect-bound" /\ rows[o] = "bound")
    /\ writer = None /\ worker = None
    /\ (readers = {} \/ Fault = "collect-reader")
    /\ o \in present \/ o \in staged
    /\ present' = present \ {o} /\ staged' = staged \ {o} /\ synced' = synced \ {o}
    /\ UNCHANGED <<rows, durable, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
SyncDelete(o) ==
    /\ rows[o] = "deleting" /\ o \notin present /\ o \notin staged
    /\ durable' = durable \ {o} /\ rows' = [rows EXCEPT ![o] = "gone"]
    /\ witness' = (witness \/ (Witness = "collect-after-crash" /\ o \in crashOrphans /\ ~retired))
    /\ UNCHANGED <<staged, synced, present, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans>>
Crash ==
    /\ crashes < MaxCrashes
    /\ crashes' = crashes + 1
    /\ crashHistory' = IF ~acknowledged THEN history ELSE None
    /\ crashOrphans' = {o \in present: rows[o] = "pending"}
    /\ \E remaining \in SUBSET (present \cup durable):
          /\ durable \subseteq remaining /\ present' = remaining
    /\ writer' = None /\ worker' = None /\ readers' = {}
    /\ staged' = {} /\ synced' = {}
    /\ UNCHANGED <<rows, durable, history, position, bytes, terminal, retired, acknowledged, witness>>
Next == (\E o \in Objects: Reserve(o) \/ StartIO(o) \/ SyncFile(o) \/ Publish(o)
           \/ SyncDirectory(o) \/ FinishIO(o) \/ Commit(o) \/ CloseRead(o)
           \/ Claim(o) \/ Delete(o) \/ SyncDelete(o))
        \/ Acknowledge \/ CancelAwaiter \/ Read \/ Terminate \/ Retire \/ Crash
Spec == Init /\ [][Next]_vars
CollectionFairness == \A o \in Objects:
    /\ WF_vars(CloseRead(o))
    /\ WF_vars(Delete(o))
    /\ WF_vars(SyncDelete(o))
FairSpec == Spec /\ CollectionFairness
RetiredCollected == retired ~> (\A o \in Objects: rows[o] \in {"absent", "gone"})
TypeOK == /\ rows \in [Objects -> {"absent", "pending", "bound", "deleting", "gone"}]
          /\ staged \subseteq Objects /\ synced \subseteq Objects
          /\ present \subseteq Objects /\ durable \subseteq Objects /\ readers \subseteq Objects
          /\ writer \in Objects \cup {None} /\ worker \in Objects \cup {None}
          /\ history \in Objects \cup {None} /\ position \in 0..1 /\ bytes \in {0,2}
          /\ terminal \in BOOLEAN /\ retired \in BOOLEAN /\ acknowledged \in BOOLEAN
          /\ crashes \in 0..MaxCrashes /\ witness \in BOOLEAN
          /\ crashHistory \in Objects \cup {None} /\ crashOrphans \subseteq Objects
ReferenceDurable == history # None => history \in present /\ history \in durable /\ rows[history] = "bound"
HistoryAtomic == (position = 1 <=> bytes = 2) /\ (history # None => position = 1)
ReadersProtected == readers \subseteq present
WriterProtected == \A o \in Objects: (writer = o \/ worker = o) => rows[o] \in {"pending", "bound"}
RetirementFinal == retired => history = None
AcknowledgedCommitted == acknowledged => position = 1
NoWitness == ~witness
=============================================================================
