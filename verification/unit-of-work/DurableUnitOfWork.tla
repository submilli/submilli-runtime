---------------------- MODULE DurableUnitOfWork ----------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Fault, Witness
VARIABLES rows, fingerprints, stableRows, stableFingerprints, stagedRows, stagedFingerprints,
          active, acknowledged, lost, crashed, witnessed, readOK
vars == <<rows, fingerprints, stableRows, stableFingerprints, stagedRows, stagedFingerprints,
          active, acknowledged, lost, crashed, witnessed, readOK>>
Slots == 1..2
Empty == [s \in Slots |-> 0]
Consistent(r, p) == \A s \in Slots : (r[s] # 0) = (s \in p)
Init ==
    /\ rows = Empty /\ fingerprints = {} /\ stableRows = Empty /\ stableFingerprints = {}
    /\ stagedRows = Empty /\ stagedFingerprints = {} /\ active = FALSE
    /\ acknowledged = {} /\ lost = {} /\ crashed = FALSE
    /\ witnessed = FALSE /\ readOK = TRUE
Begin ==
    /\ ~active /\ active' = TRUE
    /\ stagedRows' = rows /\ stagedFingerprints' = fingerprints
    /\ UNCHANGED <<rows, fingerprints, stableRows, stableFingerprints, acknowledged, lost, crashed, witnessed, readOK>>
WriteIntent(s) ==
    /\ active /\ stagedRows[s] = 0
    /\ stagedRows' = [stagedRows EXCEPT ![s] = 1]
    /\ UNCHANGED <<rows, fingerprints, stableRows, stableFingerprints, stagedFingerprints, active, acknowledged, lost, crashed, witnessed, readOK>>
Fingerprint(s) ==
    /\ active /\ stagedRows[s] = 1 /\ s \notin stagedFingerprints
    /\ stagedFingerprints' = stagedFingerprints \cup {s}
    /\ UNCHANGED <<rows, fingerprints, stableRows, stableFingerprints, stagedRows, active, acknowledged, lost, crashed, witnessed, readOK>>
Read(s) ==
    /\ active /\ stagedRows[s] # rows[s]
    /\ readOK' = ((IF Fault = "stale-read" THEN rows[s] ELSE stagedRows[s]) = stagedRows[s])
    /\ witnessed' = (witnessed \/ Witness = "read-own-writes")
    /\ UNCHANGED <<rows, fingerprints, stableRows, stableFingerprints, stagedRows, stagedFingerprints, active, acknowledged, lost, crashed>>
Commit ==
    /\ active /\ (Consistent(stagedRows, stagedFingerprints) \/ Fault = "early-commit")
    /\ rows' = stagedRows /\ fingerprints' = stagedFingerprints
    /\ stableRows' = stagedRows /\ stableFingerprints' = stagedFingerprints
    /\ active' = FALSE
    /\ UNCHANGED <<stagedRows, stagedFingerprints, acknowledged, lost, crashed, witnessed, readOK>>
Drop ==
    /\ active /\ active' = FALSE
    /\ rows' = IF Fault = "drop-writes" THEN stagedRows ELSE rows
    /\ fingerprints' = IF Fault = "drop-writes" THEN stagedFingerprints ELSE fingerprints
    /\ witnessed' = (witnessed \/ (Witness = "rollback" /\ stagedRows # rows))
    /\ UNCHANGED <<stableRows, stableFingerprints, stagedRows, stagedFingerprints, acknowledged, lost, crashed, readOK>>
Return(s) ==
    /\ s \notin acknowledged
    /\ (rows[s] # 0 \/ (Fault = "early-return" /\ active /\ stagedRows[s] # 0))
    /\ acknowledged' = acknowledged \cup {s}
    /\ witnessed' = (witnessed \/ (Witness = "lost-ack" /\ crashed /\ s \in lost))
    /\ UNCHANGED <<rows, fingerprints, stableRows, stableFingerprints, stagedRows, stagedFingerprints, active, lost, crashed, readOK>>
Crash ==
    /\ ~crashed /\ crashed' = TRUE /\ active' = FALSE
    /\ stagedRows' = rows /\ stagedFingerprints' = fingerprints
    /\ lost' = {s \in Slots : rows[s] # 0 /\ s \notin acknowledged}
    /\ UNCHANGED <<rows, fingerprints, stableRows, stableFingerprints, acknowledged, witnessed, readOK>>
Next == Begin \/ (\E s \in Slots : WriteIntent(s) \/ Fingerprint(s) \/ Read(s) \/ Return(s)) \/ Commit \/ Drop \/ Crash
TypeOK ==
    /\ rows \in [Slots -> 0..1] /\ stagedRows \in [Slots -> 0..1] /\ stableRows \in [Slots -> 0..1]
    /\ fingerprints \subseteq Slots /\ stagedFingerprints \subseteq Slots /\ stableFingerprints \subseteq Slots
    /\ acknowledged \subseteq Slots /\ lost \subseteq Slots
    /\ active \in BOOLEAN /\ crashed \in BOOLEAN /\ witnessed \in BOOLEAN /\ readOK \in BOOLEAN
AtomicAdmissionAndFingerprint == Consistent(rows, fingerprints)
DropDiscards == rows = stableRows /\ fingerprints = stableFingerprints
ReadOwnWrites == readOK
CommitBeforeReturn == \A s \in acknowledged : rows[s] # 0 /\ s \in fingerprints
NeverWitness == ~witnessed
Spec == Init /\ [][Next]_vars
=============================================================================
