---------------------- MODULE DurableUnitOfWork ----------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Fault, Witness
VARIABLES rows, pins, stableRows, stablePins, stagedRows, stagedPins,
          active, acknowledged, lost, crashed, witnessed, readOK
vars == <<rows, pins, stableRows, stablePins, stagedRows, stagedPins,
          active, acknowledged, lost, crashed, witnessed, readOK>>
Slots == 1..2
Empty == [s \in Slots |-> 0]
Consistent(r, p) == \A s \in Slots : (r[s] # 0) = (s \in p)
Init ==
    /\ rows = Empty /\ pins = {} /\ stableRows = Empty /\ stablePins = {}
    /\ stagedRows = Empty /\ stagedPins = {} /\ active = FALSE
    /\ acknowledged = {} /\ lost = {} /\ crashed = FALSE
    /\ witnessed = FALSE /\ readOK = TRUE
Begin ==
    /\ ~active /\ active' = TRUE
    /\ stagedRows' = rows /\ stagedPins' = pins
    /\ UNCHANGED <<rows, pins, stableRows, stablePins, acknowledged, lost, crashed, witnessed, readOK>>
WriteIntent(s) ==
    /\ active /\ stagedRows[s] = 0
    /\ stagedRows' = [stagedRows EXCEPT ![s] = 1]
    /\ UNCHANGED <<rows, pins, stableRows, stablePins, stagedPins, active, acknowledged, lost, crashed, witnessed, readOK>>
Pin(s) ==
    /\ active /\ stagedRows[s] = 1 /\ s \notin stagedPins
    /\ stagedPins' = stagedPins \cup {s}
    /\ UNCHANGED <<rows, pins, stableRows, stablePins, stagedRows, active, acknowledged, lost, crashed, witnessed, readOK>>
Read(s) ==
    /\ active /\ stagedRows[s] # rows[s]
    /\ readOK' = ((IF Fault = "stale-read" THEN rows[s] ELSE stagedRows[s]) = stagedRows[s])
    /\ witnessed' = (witnessed \/ Witness = "read-own-writes")
    /\ UNCHANGED <<rows, pins, stableRows, stablePins, stagedRows, stagedPins, active, acknowledged, lost, crashed>>
Commit ==
    /\ active /\ (Consistent(stagedRows, stagedPins) \/ Fault = "early-commit")
    /\ rows' = stagedRows /\ pins' = stagedPins
    /\ stableRows' = stagedRows /\ stablePins' = stagedPins
    /\ active' = FALSE
    /\ UNCHANGED <<stagedRows, stagedPins, acknowledged, lost, crashed, witnessed, readOK>>
Drop ==
    /\ active /\ active' = FALSE
    /\ rows' = IF Fault = "drop-writes" THEN stagedRows ELSE rows
    /\ pins' = IF Fault = "drop-writes" THEN stagedPins ELSE pins
    /\ witnessed' = (witnessed \/ (Witness = "rollback" /\ stagedRows # rows))
    /\ UNCHANGED <<stableRows, stablePins, stagedRows, stagedPins, acknowledged, lost, crashed, readOK>>
Return(s) ==
    /\ s \notin acknowledged
    /\ (rows[s] # 0 \/ (Fault = "early-return" /\ active /\ stagedRows[s] # 0))
    /\ acknowledged' = acknowledged \cup {s}
    /\ witnessed' = (witnessed \/ (Witness = "lost-ack" /\ crashed /\ s \in lost))
    /\ UNCHANGED <<rows, pins, stableRows, stablePins, stagedRows, stagedPins, active, lost, crashed, readOK>>
Crash ==
    /\ ~crashed /\ crashed' = TRUE /\ active' = FALSE
    /\ stagedRows' = rows /\ stagedPins' = pins
    /\ lost' = {s \in Slots : rows[s] # 0 /\ s \notin acknowledged}
    /\ UNCHANGED <<rows, pins, stableRows, stablePins, acknowledged, witnessed, readOK>>
Next == Begin \/ (\E s \in Slots : WriteIntent(s) \/ Pin(s) \/ Read(s) \/ Return(s)) \/ Commit \/ Drop \/ Crash
TypeOK ==
    /\ rows \in [Slots -> 0..1] /\ stagedRows \in [Slots -> 0..1] /\ stableRows \in [Slots -> 0..1]
    /\ pins \subseteq Slots /\ stagedPins \subseteq Slots /\ stablePins \subseteq Slots
    /\ acknowledged \subseteq Slots /\ lost \subseteq Slots
    /\ active \in BOOLEAN /\ crashed \in BOOLEAN /\ witnessed \in BOOLEAN /\ readOK \in BOOLEAN
AtomicAdmissionAndPins == Consistent(rows, pins)
DropDiscards == rows = stableRows /\ pins = stablePins
ReadOwnWrites == readOK
CommitBeforeReturn == \A s \in acknowledged : rows[s] # 0 /\ s \in pins
NeverWitness == ~witnessed
Spec == Init /\ [][Next]_vars
=============================================================================
