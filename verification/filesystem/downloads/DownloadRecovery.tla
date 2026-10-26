-------------------------- MODULE DownloadRecovery --------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS MaxCrashes, Fault, Witness, InitiallyExists, Overwrite
ASSUME /\ MaxCrashes \in Nat /\ InitiallyExists \in BOOLEAN
       /\ Fault \in {"none", "reuse-inode", "skip-file-sync", "skip-directory-sync",
                      "reapply-completed", "advance-pending", "skip-admission", "repeat-admission", "wrong-metadata"}

(* Checked admission and history are independent SQL commits. phase is
   process-local; attempt is a ghost counter allocating fresh inode identities,
   not persisted implementation state. The journal holds admission only.
   Each attempt may fetch a different response, represented by its inode number.
   The same owner runs later effects only after history says "completed". *)
Inodes == 0..(MaxCrashes + 1)
Absent == MaxCrashes + 2
Contents(name, values) == IF name = Absent THEN "absent" ELSE values[name]
VARIABLES checked, result, history, phase, up, crashes, attempt, temp, destination, data,
          diskTemp, diskDestination, diskData, later, seen
vars == <<checked, result, history, phase, up, crashes, attempt, temp, destination, data,
          diskTemp, diskDestination, diskData, later, seen>>

Init ==
    /\ checked = FALSE /\ result = "absent"
    /\ history = "pending" /\ phase = "start" /\ up = TRUE
    /\ crashes = 0 /\ attempt = 0 /\ temp = Absent /\ destination = IF InitiallyExists THEN 0 ELSE Absent
    /\ data = [i \in Inodes |-> IF i = 0 THEN "old" ELSE "empty"]
    /\ diskTemp = temp /\ diskDestination = destination /\ diskData = data
    /\ later = FALSE /\ seen = {}

Admit ==
    /\ up /\ phase = "start" /\ ~checked
    /\ (Overwrite \/ ~InitiallyExists \/ Fault = "skip-admission")
    /\ checked' = TRUE
    /\ UNCHANGED <<result, history, phase, up, crashes, attempt, temp, destination,
                   data, diskTemp, diskDestination, diskData, later, seen>>

Reject ==
    /\ up /\ phase = "start"
    /\ ((~checked /\ InitiallyExists /\ ~Overwrite)
        \/ (Fault = "repeat-admission" /\ checked /\ destination # Absent /\ ~Overwrite))
    /\ history' = "rejected" /\ phase' = "done"
    /\ UNCHANGED <<checked, result, up, crashes, attempt, temp, destination,
                   data, diskTemp, diskDestination, diskData, later, seen>>

CreateFresh ==
    /\ up /\ phase = "start" /\ checked
    /\ attempt' = attempt + 1
    /\ temp' = IF Fault = "reuse-inode" /\ temp # Absent THEN temp ELSE attempt'
    /\ data' = [data EXCEPT ![temp'] = "empty"]
    /\ phase' = "write"
    /\ seen' = IF temp # Absent /\ temp = destination
               THEN seen \cup {"alias-retry"} ELSE seen
    /\ UNCHANGED <<checked, result, history, up, crashes, destination, diskTemp,
                   diskDestination, diskData, later>>

Write ==
    /\ up /\ phase = "write"
    /\ data' = [data EXCEPT ![temp] = ToString(attempt)] /\ phase' = "sync-file"
    /\ UNCHANGED <<checked, result, history, up, crashes, attempt, temp, destination,
                   diskTemp, diskDestination, diskData, later, seen>>

SyncFile ==
    /\ up /\ phase = "sync-file"
    /\ diskData' = IF Fault = "skip-file-sync" THEN diskData
                    ELSE [diskData EXCEPT ![temp] = data[temp]]
    /\ phase' = "rename"
    /\ UNCHANGED <<checked, result, history, up, crashes, attempt, temp, destination,
                   data, diskTemp, diskDestination, later, seen>>

Rename ==
    /\ up /\ phase = "rename"
    /\ destination' = temp /\ temp' = Absent /\ phase' = "sync-directories"
    /\ UNCHANGED <<checked, result, history, up, crashes, attempt, data, diskTemp,
                   diskDestination, diskData, later, seen>>

SyncParent ==
    /\ up /\ phase = "sync-directories"
    /\ diskDestination' = IF Fault = "skip-directory-sync" THEN diskDestination
                           ELSE destination
    /\ phase' = "sync-temp-directory"
    /\ UNCHANGED <<checked, result, history, up, crashes, attempt, temp, diskTemp, destination, data,
                   diskData, later, seen>>

SyncTempDirectory ==
    /\ up /\ phase = "sync-temp-directory"
    /\ diskTemp' = temp /\ phase' = "returned"
    /\ UNCHANGED <<checked, result, history, up, crashes, attempt, temp, destination, data,
                   diskDestination, diskData, later, seen>>

SaveCompletion ==
    /\ up /\ phase = "returned"
    /\ history' = "completed" /\ phase' = "done"
    /\ result' = IF Fault = "wrong-metadata" THEN "wrong" ELSE ToString(attempt)
    /\ UNCHANGED <<checked, up, crashes, attempt, temp, destination, data,
                   diskTemp, diskDestination, diskData, later, seen>>

LaterEffect ==
    /\ up /\ ~later
    /\ (history = "completed" \/ (Fault = "advance-pending" /\ phase = "returned"))
    /\ data' = [data EXCEPT ![destination] = "later"] /\ diskData' = data'
    /\ later' = TRUE
    /\ UNCHANGED <<checked, result, history, phase, up, crashes, attempt, temp, destination,
                   diskTemp, diskDestination, seen>>

(* Directory entries may persist independently. A crash after rename may
   expose BOTH the old private name and the new public name for the same inode.
   Retrying must unlink the private name, never truncate that inode. *)
Writeback ==
    /\ up
    /\ diskTemp' \in {diskTemp, temp}
    /\ diskDestination' \in {diskDestination, destination}
    /\ UNCHANGED <<checked, result, history, phase, up, crashes, attempt, temp, destination,
                   data, diskData, later, seen>>

Crash ==
    /\ up /\ crashes < MaxCrashes
    /\ up' = FALSE /\ crashes' = crashes + 1 /\ phase' = "offline"
    /\ temp' = diskTemp /\ destination' = diskDestination /\ data' = diskData
    /\ seen' = IF phase = "returned" /\ history = "pending"
               THEN seen \cup {"completion-gap"} ELSE seen
    /\ UNCHANGED <<checked, result, history, attempt, diskTemp, diskDestination, diskData, later>>

Restart ==
    /\ ~up /\ up' = TRUE
    /\ phase' = IF history \in {"completed", "rejected"} /\ Fault # "reapply-completed"
                THEN "done" ELSE "start"
    /\ seen' = seen \cup {"restart"}
    /\ UNCHANGED <<checked, result, history, crashes, attempt, temp, destination, data,
                   diskTemp, diskDestination, diskData, later>>

Step == Admit \/ Reject \/ CreateFresh \/ Write \/ SyncFile \/ Rename \/ SyncParent \/ SyncTempDirectory \/ SaveCompletion
Next == Step \/ LaterEffect \/ Writeback \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Step) /\ WF_vars(Restart)
Progress == <>(history \in {"completed", "rejected"})
TypeOK == /\ history \in {"pending", "completed", "rejected"}
          /\ destination \in Inodes \cup {Absent}
          /\ diskDestination \in Inodes \cup {Absent}
          /\ temp \in Inodes \cup {Absent} /\ diskTemp \in Inodes \cup {Absent}
          /\ data \in [Inodes -> ({"old", "empty", "later"} \cup {ToString(i): i \in Inodes})]
          /\ diskData \in [Inodes -> ({"old", "empty", "later"} \cup {ToString(i): i \in Inodes})]
          /\ attempt \in Inodes /\ crashes \in 0..MaxCrashes
AtomicReplacement == Contents(destination, data) # "empty"
CompletionDurable == history = "completed" /\ ~later => Contents(diskDestination, diskData) = result
LaterPreserved == later => Contents(destination, data) = "later"
AdmissionProtects == InitiallyExists /\ ~Overwrite => Contents(destination, data) = "old"
RetryAllowed == checked => history # "rejected"
NeverWitness == CASE Witness = "gap" ->
                      ~({"completion-gap", "restart"} \subseteq seen /\ history = "completed")
                  [] Witness = "alias" -> "alias-retry" \notin seen
                  [] OTHER -> TRUE
=============================================================================
