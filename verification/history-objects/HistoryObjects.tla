------------------------- MODULE HistoryObjects -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS MaxObjects, MaxCrashes, Fault, Witness
Objects == 1..MaxObjects
None == 0
VARIABLES used, deleting, staged, synced, present, durable, writer, worker, readers,
          history, position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness
vars == <<used, deleting, staged, synced, present, durable, writer, worker, readers,
          history, position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Init == /\ used = {} /\ deleting = FALSE
        /\ staged = {} /\ synced = {} /\ present = {} /\ durable = {}
        /\ writer = None /\ worker = None /\ readers = {}
        /\ history = None /\ position = 0 /\ bytes = 0
        /\ terminal = FALSE /\ retired = FALSE /\ acknowledged = FALSE
        /\ crashes = 0 /\ crashHistory = None /\ crashOrphans = {} /\ witness = FALSE
BeginAppend(o) ==
    /\ ~terminal /\ history = None /\ writer = None /\ worker = None
    /\ o \notin used
    /\ used' = used \cup {o} /\ writer' = o
    /\ UNCHANGED deleting
    /\ UNCHANGED <<staged, synced, present, durable, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
StartIO(o) ==
    /\ writer = o /\ worker = None /\ o \notin durable
    /\ worker' = o /\ staged' = staged \cup {o}
    /\ UNCHANGED <<used, deleting, synced, present, durable, writer, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
SyncFile(o) ==
    /\ worker = o /\ o \in staged /\ o \notin synced
    /\ synced' = synced \cup {o}
    /\ UNCHANGED <<used, deleting, staged, present, durable, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Publish(o) ==
    /\ worker = o /\ o \in synced /\ o \notin present
    /\ present' = present \cup {o}
    /\ UNCHANGED <<used, deleting, staged, synced, durable, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
SyncDirectory(o) ==
    /\ worker = o /\ o \in present /\ o \notin durable
    /\ durable' = durable \cup {o}
    /\ UNCHANGED <<used, deleting, staged, synced, present, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
FinishIO(o) ==
    /\ worker = o /\ (o \in durable \/ Fault = "early-reference")
    /\ worker' = None
    /\ UNCHANGED <<used, deleting, staged, synced, present, durable, writer, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Commit(o) ==
    /\ writer = o /\ worker = None /\ ~terminal /\ history = None

    /\ (o \in durable \/ (Fault = "early-reference" /\ o \in present))
    /\ history' = o
    /\ UNCHANGED <<used, deleting>>
    /\ position' = 1 /\ bytes' = 2
    /\ UNCHANGED <<staged, synced, present, durable, writer, worker, readers,
                   terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Acknowledge ==
    /\ writer # None /\ history = writer
    /\ acknowledged' = TRUE /\ writer' = None
    /\ UNCHANGED <<used, deleting, staged, synced, present, durable, worker, readers, history,
                   position, bytes, terminal, retired, crashes, crashHistory, crashOrphans, witness>>
CancelAwaiter ==
    /\ writer # None /\ history = None
    /\ writer' = None
    /\ witness' = (witness \/ (Witness = "cancel-worker" /\ worker # None))
    /\ UNCHANGED <<used, deleting, staged, synced, present, durable, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans>>
Read ==
    /\ ~terminal /\ history # None /\ history \notin readers
    /\ readers' = readers \cup {history}
    /\ witness' = (witness \/ (Witness = "replay-after-crash" /\ history = crashHistory /\ crashHistory # None))
    /\ UNCHANGED <<used, deleting, staged, synced, present, durable, writer, worker, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans>>
CloseRead(o) ==
    /\ o \in readers /\ readers' = readers \ {o}
    /\ UNCHANGED <<used, deleting, staged, synced, present, durable, writer, worker, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Terminate ==
    /\ ~terminal /\ terminal' = TRUE
    /\ UNCHANGED <<used, deleting, staged, synced, present, durable, writer, worker, readers,
                   history, position, bytes, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Delete ==
    /\ (terminal \/ Fault = "collect-bound") /\ ~retired /\ ~deleting
    /\ ((writer = None /\ worker = None) \/ Fault = "collect-writer")
    /\ (readers = {} \/ Fault = "collect-reader")
    /\ deleting' = TRUE
    /\ present' = {} /\ staged' = {} /\ synced' = {}
    /\ UNCHANGED <<used, durable, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans, witness>>
SyncDelete ==
    /\ deleting /\ present = {} /\ staged = {}
    /\ durable' = {}
    /\ witness' = (witness \/ (Witness = "collect-after-crash" /\ crashOrphans # {}))
    /\ UNCHANGED <<used, deleting, staged, synced, present, writer, worker, readers, history,
                   position, bytes, terminal, retired, acknowledged, crashes, crashHistory, crashOrphans>>
Retire ==
    /\ terminal /\ ~retired /\ deleting /\ durable = {}
    /\ retired' = TRUE /\ history' = None
    /\ UNCHANGED <<used, deleting, staged, synced, present, durable, writer, worker, readers,
                   position, bytes, terminal, acknowledged, crashes, crashHistory, crashOrphans, witness>>
Crash ==
    /\ crashes < MaxCrashes
    /\ crashes' = crashes + 1
    /\ crashHistory' = IF ~acknowledged THEN history ELSE None
    /\ crashOrphans' = present \ {history}
    /\ \E remaining \in SUBSET (present \cup durable):
          /\ durable \subseteq remaining /\ present' = remaining
    /\ writer' = None /\ worker' = None /\ readers' = {} /\ deleting' = FALSE
    /\ staged' = {} /\ synced' = {}
    /\ UNCHANGED <<used, durable, history, position, bytes, terminal, retired, acknowledged, witness>>
Next == (\E o \in Objects: BeginAppend(o) \/ StartIO(o) \/ SyncFile(o) \/ Publish(o)
           \/ SyncDirectory(o) \/ FinishIO(o) \/ Commit(o) \/ CloseRead(o)
           )
        \/ Acknowledge \/ CancelAwaiter \/ Read \/ Terminate \/ Delete \/ SyncDelete \/ Retire \/ Crash
Spec == Init /\ [][Next]_vars
CollectionFairness == /\ (\A o \in Objects: WF_vars(CloseRead(o)))
                      /\ WF_vars(Delete) /\ WF_vars(SyncDelete) /\ WF_vars(Retire)
FairSpec == Spec /\ CollectionFairness
RetiredCollected == (terminal /\ writer = None /\ worker = None) ~> retired
TypeOK == /\ used \subseteq Objects /\ deleting \in BOOLEAN
          /\ staged \subseteq Objects /\ synced \subseteq Objects
          /\ present \subseteq Objects /\ durable \subseteq Objects /\ readers \subseteq Objects
          /\ writer \in Objects \cup {None} /\ worker \in Objects \cup {None}
          /\ history \in Objects \cup {None} /\ position \in 0..1 /\ bytes \in {0,2}
          /\ terminal \in BOOLEAN /\ retired \in BOOLEAN /\ acknowledged \in BOOLEAN
          /\ crashes \in 0..MaxCrashes /\ witness \in BOOLEAN
          /\ crashHistory \in Objects \cup {None} /\ crashOrphans \subseteq Objects
ReferenceDurable == (~terminal /\ history # None) => history \in present /\ history \in durable
HistoryAtomic == (position = 1 <=> bytes = 2) /\ (history # None => position = 1)
ReadersProtected == readers \subseteq present
WriterProtected == deleting => writer = None /\ worker = None
RetirementFinal == retired => history = None /\ durable = {} /\ present = {}
AcknowledgedCommitted == acknowledged => position = 1
NoWitness == ~witness
=============================================================================
