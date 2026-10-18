-------------------------- MODULE WriteRecovery --------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS MaxCrashes, Fault, Witness, InitiallyExists
ASSUME /\ MaxCrashes \in Nat /\ InitiallyExists \in BOOLEAN
       /\ Fault \in {"none", "reuse-inode", "skip-file-sync", "skip-directory-sync",
                      "reapply-completed", "advance-pending"}

(* Only history is SQL state, owned by the execution framework. phase is
   process-local; attempt is a ghost counter allocating fresh inode identities,
   not persisted implementation state. There is no filesystem write journal.
   Input is deterministically regenerated: every attempt writes "replacement".
   The same owner runs later effects only after history says "completed". *)
Inodes == 0..(MaxCrashes + 1)
Absent == MaxCrashes + 2
Contents(name, values) == IF name = Absent THEN "absent" ELSE values[name]
VARIABLES history, phase, up, crashes, attempt, temp, destination, data,
          diskTemp, diskDestination, diskData, later, seen
vars == <<history, phase, up, crashes, attempt, temp, destination, data,
          diskTemp, diskDestination, diskData, later, seen>>

Init ==
    /\ history = "pending" /\ phase = "start" /\ up = TRUE
    /\ crashes = 0 /\ attempt = 0 /\ temp = Absent /\ destination = IF InitiallyExists THEN 0 ELSE Absent
    /\ data = [i \in Inodes |-> IF i = 0 THEN "old" ELSE "empty"]
    /\ diskTemp = temp /\ diskDestination = destination /\ diskData = data
    /\ later = FALSE /\ seen = {}

CreateFresh ==
    /\ up /\ phase = "start"
    /\ attempt' = attempt + 1
    /\ temp' = IF Fault = "reuse-inode" /\ temp # Absent THEN temp ELSE attempt'
    /\ data' = [data EXCEPT ![temp'] = "empty"]
    /\ phase' = "write"
    /\ seen' = IF temp # Absent /\ temp = destination
               THEN seen \cup {"alias-retry"} ELSE seen
    /\ UNCHANGED <<history, up, crashes, destination, diskTemp,
                   diskDestination, diskData, later>>

Write ==
    /\ up /\ phase = "write"
    /\ data' = [data EXCEPT ![temp] = "replacement"] /\ phase' = "sync-file"
    /\ UNCHANGED <<history, up, crashes, attempt, temp, destination,
                   diskTemp, diskDestination, diskData, later, seen>>

SyncFile ==
    /\ up /\ phase = "sync-file"
    /\ diskData' = IF Fault = "skip-file-sync" THEN diskData
                    ELSE [diskData EXCEPT ![temp] = data[temp]]
    /\ phase' = "rename"
    /\ UNCHANGED <<history, up, crashes, attempt, temp, destination,
                   data, diskTemp, diskDestination, later, seen>>

Rename ==
    /\ up /\ phase = "rename"
    /\ destination' = temp /\ temp' = Absent /\ phase' = "sync-directories"
    /\ UNCHANGED <<history, up, crashes, attempt, data, diskTemp,
                   diskDestination, diskData, later, seen>>

SyncParent ==
    /\ up /\ phase = "sync-directories"
    /\ diskDestination' = IF Fault = "skip-directory-sync" THEN diskDestination
                           ELSE destination
    /\ phase' = "sync-temp-directory"
    /\ UNCHANGED <<history, up, crashes, attempt, temp, diskTemp, destination, data,
                   diskData, later, seen>>

SyncTempDirectory ==
    /\ up /\ phase = "sync-temp-directory"
    /\ diskTemp' = temp /\ phase' = "returned"
    /\ UNCHANGED <<history, up, crashes, attempt, temp, destination, data,
                   diskDestination, diskData, later, seen>>

SaveCompletion ==
    /\ up /\ phase = "returned"
    /\ history' = "completed" /\ phase' = "done"
    /\ UNCHANGED <<up, crashes, attempt, temp, destination, data,
                   diskTemp, diskDestination, diskData, later, seen>>

LaterEffect ==
    /\ up /\ ~later
    /\ (history = "completed" \/ (Fault = "advance-pending" /\ phase = "returned"))
    /\ data' = [data EXCEPT ![destination] = "later"] /\ diskData' = data'
    /\ later' = TRUE
    /\ UNCHANGED <<history, phase, up, crashes, attempt, temp, destination,
                   diskTemp, diskDestination, seen>>

(* Directory entries may persist independently. A crash after rename may
   expose BOTH the old private name and the new public name for the same inode.
   Retrying must unlink the private name, never truncate that inode. *)
Writeback ==
    /\ up
    /\ diskTemp' \in {diskTemp, temp}
    /\ diskDestination' \in {diskDestination, destination}
    /\ UNCHANGED <<history, phase, up, crashes, attempt, temp, destination,
                   data, diskData, later, seen>>

Crash ==
    /\ up /\ crashes < MaxCrashes
    /\ up' = FALSE /\ crashes' = crashes + 1 /\ phase' = "offline"
    /\ temp' = diskTemp /\ destination' = diskDestination /\ data' = diskData
    /\ seen' = IF phase = "returned" /\ history = "pending"
               THEN seen \cup {"completion-gap"} ELSE seen
    /\ UNCHANGED <<history, attempt, diskTemp, diskDestination, diskData, later>>

Restart ==
    /\ ~up /\ up' = TRUE
    /\ phase' = IF history = "completed" /\ Fault # "reapply-completed"
                THEN "done" ELSE "start"
    /\ seen' = seen \cup {"restart"}
    /\ UNCHANGED <<history, crashes, attempt, temp, destination, data,
                   diskTemp, diskDestination, diskData, later>>

Step == CreateFresh \/ Write \/ SyncFile \/ Rename \/ SyncParent \/ SyncTempDirectory \/ SaveCompletion
Next == Step \/ LaterEffect \/ Writeback \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Step) /\ WF_vars(Restart)
Progress == <>(history = "completed")
TypeOK == /\ history \in {"pending", "completed"}
          /\ destination \in Inodes \cup {Absent}
          /\ diskDestination \in Inodes \cup {Absent}
          /\ temp \in Inodes \cup {Absent} /\ diskTemp \in Inodes \cup {Absent}
          /\ data \in [Inodes -> {"old", "empty", "replacement", "later"}]
          /\ diskData \in [Inodes -> {"old", "empty", "replacement", "later"}]
          /\ attempt \in Inodes /\ crashes \in 0..MaxCrashes
AtomicReplacement == Contents(destination, data) # "empty"
CompletionDurable == history = "completed" /\ ~later => Contents(diskDestination, diskData) = "replacement"
LaterPreserved == later => Contents(destination, data) = "later"
NeverWitness == CASE Witness = "gap" ->
                      ~({"completion-gap", "restart"} \subseteq seen /\ history = "completed")
                  [] Witness = "alias" -> "alias-retry" \notin seen
                  [] OTHER -> TRUE
=============================================================================
