------------------------ MODULE NamespaceRecovery -------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Operation, MaxCrashes, CrashMode, Fault, Witness
ASSUME /\ Operation \in {"replace", "move", "remove"}
       /\ MaxCrashes \in Nat
       /\ CrashMode \in {"power", "process"}
       /\ Fault \in {"none", "early-unlink", "no-file-sync",
                      "no-directory-sync", "early-cleanup", "path-retry"}

Paths == {"source", "destination"}
Versions == {"absent", "original", "old-destination", "candidate", "newer"}
InitialNames == [p \in Paths |-> IF p = "source" THEN "original"
                               ELSE "old-destination"]
DesiredNames ==
    CASE Operation = "replace" -> [InitialNames EXCEPT !["source"] = "candidate"]
      [] Operation = "move" -> [p \in Paths |-> IF p = "source" THEN "absent"
                                  ELSE "original"]
      [] OTHER -> [InitialNames EXCEPT !["source"] = "absent"]
NeedsCandidate == Operation = "replace"

(* sql = pending is a fieldless Prepared marker. Source/destination arguments
   are supplied again by replay; names/diskNames describe the filesystem, not
   saved Entry values. Replacement uses the deterministic effect artifact path
   and compares the current candidate/destination identities only in native IO. *)
VARIABLES sql, names, diskNames, candidate, diskCandidate,
          candidateDirty, up, phase, syncedDirs, fileSynced, crashes, consumed,
          reused, badReceipt, inputAvailable, seen
vars == <<sql, names, diskNames, candidate, diskCandidate,
          candidateDirty, up, phase, syncedDirs, fileSynced, crashes, consumed,
          reused, badReceipt, inputAvailable, seen>>

Init ==
    /\ sql = "absent"
    /\ names = InitialNames /\ diskNames = InitialNames
    /\ candidate = "empty" /\ diskCandidate = "empty"
    /\ candidateDirty = FALSE
    /\ up = TRUE /\ phase = "idle"
    /\ syncedDirs = {} /\ fileSynced = FALSE
    /\ crashes = 0 /\ consumed = FALSE /\ reused = FALSE
    /\ badReceipt = FALSE /\ inputAvailable = FALSE /\ seen = {}

SupplyInput ==
    /\ up /\ ~inputAvailable /\ sql \in {"absent", "pending"}
    /\ inputAvailable' = TRUE
    /\ UNCHANGED <<sql, names, diskNames, candidate, diskCandidate,
                   candidateDirty, up, phase, syncedDirs, fileSynced, crashes, consumed,
                   reused, badReceipt, seen>>

Intent ==
    /\ up /\ sql = "absent"
    /\ sql' = "pending"
    /\ phase' = "prepare"
    /\ UNCHANGED <<names, diskNames, candidate, diskCandidate,
                   candidateDirty, up, syncedDirs, fileSynced, crashes, consumed,
                   reused, badReceipt, inputAvailable, seen>>

Discover ==
    /\ up /\ sql = "pending" /\ phase = "idle"
    /\ phase' = "prepare"
    /\ seen' = seen \cup {"retry"}
    /\ UNCHANGED <<sql, names, diskNames, candidate, diskCandidate,
                   candidateDirty, up, syncedDirs, fileSynced, crashes, consumed,
                   reused, badReceipt, inputAvailable>>

(* No original-file backup is needed under exclusive ownership. A move keeps
   the original source name until the destination name has been synchronized.
   Replacement input is supplied by replay; deletion needs no original bytes. *)
PrepareCandidate ==
    /\ up /\ sql = "pending" /\ phase = "prepare" /\ NeedsCandidate
    /\ inputAvailable /\ candidate # "complete"
    /\ candidate' = IF candidate = "partial" THEN "complete" ELSE "partial"
    /\ candidateDirty' = TRUE
    /\ fileSynced' = FALSE
    /\ UNCHANGED <<sql, names, diskNames, diskCandidate,
                   up, phase, syncedDirs, crashes, consumed, reused, badReceipt, inputAvailable, seen>>

SyncCandidate ==
    /\ up /\ sql = "pending" /\ candidate = "complete" /\ ~fileSynced
    /\ diskCandidate' = candidate
    /\ candidateDirty' = FALSE
    /\ fileSynced' = TRUE
    /\ UNCHANGED <<sql, names, diskNames, candidate,
                   up, phase, syncedDirs, crashes, consumed, reused, badReceipt, inputAvailable, seen>>

Apply ==
    /\ up /\ sql = "pending" /\ phase = "prepare"
    /\ (Operation # "move" \/ "original" \in {names[p] : p \in Paths})
    /\ (~NeedsCandidate \/ (candidate = "complete"
                             /\ (fileSynced \/ Fault = "no-file-sync")))
    /\ names' = IF Operation = "move" THEN [names EXCEPT !["destination"] = "original"] ELSE DesiredNames
    /\ phase' = IF Operation = "move" THEN "unlink" ELSE "sync"
    /\ syncedDirs' = {}
    /\ seen' = seen \cup {"applied"}
    /\ UNCHANGED <<sql, diskNames, candidate, diskCandidate,
                   candidateDirty, up, fileSynced, crashes, consumed, reused, badReceipt, inputAvailable>>

SyncDirectory(p) ==
    /\ up /\ sql = "pending" /\ phase \in {"sync", "unlink"} /\ p \notin syncedDirs
    /\ (phase = "sync" \/ p = "destination")
    /\ diskNames' = [diskNames EXCEPT ![p] = names[p]]
    /\ syncedDirs' = syncedDirs \cup {p}
    /\ UNCHANGED <<sql, names, candidate, diskCandidate,
                   candidateDirty, up, phase, fileSynced, crashes, consumed,
                   reused, badReceipt, inputAvailable, seen>>

RemoveSource ==
    /\ up /\ sql = "pending" /\ phase = "unlink"
    /\ ("destination" \in syncedDirs \/ Fault = "early-unlink")
    /\ names' = [names EXCEPT !["source"] = "absent"]
    /\ phase' = "sync"
    /\ UNCHANGED <<sql, diskNames, candidate, diskCandidate,
                   candidateDirty, up, syncedDirs, fileSynced, crashes, consumed,
                   reused, badReceipt, inputAvailable, seen>>

CommitReceipt ==
    /\ up /\ sql = "pending" /\ phase = "sync"
    /\ (syncedDirs = Paths \/ Fault = "no-directory-sync")
    /\ sql' = "done"
    /\ phase' = "idle"
    /\ badReceipt' = (diskNames # DesiredNames
                       \/ (NeedsCandidate /\ diskCandidate # "complete"))
    /\ seen' = seen \cup {"receipt"}
    /\ UNCHANGED <<names, diskNames, candidate, diskCandidate,
                   candidateDirty, up, syncedDirs, fileSynced, crashes, consumed, reused, inputAvailable>>

Consume ==
    /\ up /\ sql = "done" /\ ~consumed
    /\ consumed' = TRUE
    /\ seen' = seen \cup {"consumed"}
    /\ UNCHANGED <<sql, names, diskNames, candidate, diskCandidate,
                   candidateDirty, up, phase, syncedDirs, fileSynced, crashes, reused, badReceipt, inputAvailable>>

ReplacePaths ==
    /\ up /\ sql = "done" /\ consumed /\ ~reused
    /\ names' = [p \in Paths |-> "newer"]
    /\ diskNames' = names'
    /\ reused' = TRUE
    /\ UNCHANGED <<sql, candidate, diskCandidate, candidateDirty,
                   up, phase, syncedDirs, fileSynced, crashes, consumed, badReceipt, inputAvailable, seen>>

Replay ==
    /\ up /\ sql = "done" /\ reused
    /\ names' = IF Fault = "path-retry" THEN DesiredNames ELSE names
    /\ seen' = seen \cup {"replay-after-reuse"}
    /\ UNCHANGED <<sql, diskNames, candidate, diskCandidate,
                   candidateDirty, up, phase, syncedDirs, fileSynced, crashes,
                   consumed, reused, badReceipt, inputAvailable>>

Writeback ==
    /\ sql = "pending"
    /\ \E p \in Paths:
           /\ diskNames' = [diskNames EXCEPT ![p] = names[p]]
           /\ UNCHANGED <<sql, names, candidate, diskCandidate,
                          candidateDirty, up, phase, syncedDirs, fileSynced,
                          crashes, consumed, reused, badReceipt, inputAvailable, seen>>

CandidateWriteback ==
    /\ candidateDirty
    /\ diskCandidate' \in {"empty", "partial", "corrupt", candidate}
    /\ UNCHANGED <<sql, names, diskNames, candidate,
                   candidateDirty, up, phase, syncedDirs, fileSynced, crashes,
                   consumed, reused, badReceipt, inputAvailable, seen>>

Crash ==
    /\ up /\ crashes < MaxCrashes
    /\ up' = FALSE
    /\ inputAvailable' = FALSE /\ crashes' = crashes + 1
    /\ names' = IF CrashMode = "power" THEN diskNames ELSE names
    /\ candidate' = IF CrashMode = "power" THEN diskCandidate ELSE candidate
    /\ candidateDirty' = IF CrashMode = "power" THEN FALSE ELSE candidateDirty
    /\ phase' = "idle" /\ syncedDirs' = {} /\ fileSynced' = FALSE
    /\ seen' = seen \cup
         (IF sql = "pending" /\ names # InitialNames
             /\ names # DesiredNames THEN {"partial-namespace-crash"} ELSE {})
         \cup (IF sql = "pending" /\ diskNames = DesiredNames
                    THEN {"published-no-receipt"} ELSE {})
         \cup (IF sql = "done" /\ ~consumed THEN {"lost-receipt-ack"} ELSE {})
    /\ UNCHANGED <<sql, diskNames, diskCandidate, consumed, reused, badReceipt>>

Restart ==
    /\ ~up /\ up' = TRUE
    /\ seen' = seen \cup {"restart"}
    /\ UNCHANGED <<sql, names, diskNames, candidate, diskCandidate,
                   candidateDirty, phase, syncedDirs, fileSynced, crashes,
                   consumed, reused, badReceipt, inputAvailable>>

Terminal ==
    /\ up /\ sql = "done" /\ consumed /\ reused
    /\ "replay-after-reuse" \in seen
    /\ sql' = "terminal"
    /\ UNCHANGED <<names, diskNames, candidate, diskCandidate,
                   candidateDirty, up, phase, syncedDirs, fileSynced, crashes,
                   consumed, reused, badReceipt, inputAvailable, seen>>

DiscardJournal ==
    /\ up /\ sql = "done" /\ consumed /\ Fault = "early-cleanup"
    /\ sql' = "absent"
    /\ UNCHANGED <<names, diskNames, candidate, diskCandidate, candidateDirty,
                   up, phase, syncedDirs, fileSynced, crashes, consumed,
                   reused, badReceipt, inputAvailable, seen>>

(* The journal identifies the effect; phase tracks volatile native work inside
   an arm. Prepared may still need to recreate a replacement from replay bytes after a
   crash. This model abstracts request admission and preflight validation
   into Intent; FilesystemOperations checks their separate SQL transitions. *)
DecodeState == CASE sql = "absent" -> "Initial"
                 [] sql = "pending" -> "Prepared"
                 [] sql = "done" -> "Completed"
                 [] OTHER -> "Retired"
NamespaceLoop ==
    CASE DecodeState = "Initial" -> Intent
      [] DecodeState = "Prepared" -> Discover \/ PrepareCandidate \/ SyncCandidate \/ Apply
           \/ (\E p \in Paths: SyncDirectory(p)) \/ RemoveSource \/ CommitReceipt
      [] DecodeState = "Completed" -> Consume \/ Replay
      [] OTHER -> FALSE
Next == NamespaceLoop \/ SupplyInput \/ ReplacePaths \/ Writeback
        \/ CandidateWriteback \/ Crash \/ Restart \/ Terminal \/ DiscardJournal
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(SupplyInput) /\ WF_vars(Intent) /\ WF_vars(Discover)
             /\ WF_vars(PrepareCandidate) /\ WF_vars(SyncCandidate)
             /\ WF_vars(Apply) /\ (\A p \in Paths: WF_vars(SyncDirectory(p)))
             /\ WF_vars(RemoveSource) /\ WF_vars(CommitReceipt) /\ WF_vars(Consume)
             /\ WF_vars(ReplacePaths) /\ WF_vars(Replay)
             /\ WF_vars(Restart) /\ WF_vars(Terminal)

TypeOK ==
    /\ sql \in {"absent", "pending", "done", "terminal"}
    /\ names \in [Paths -> Versions] /\ diskNames \in [Paths -> Versions]
    /\ candidate \in {"empty", "partial", "corrupt", "complete"}
    /\ diskCandidate \in {"empty", "partial", "corrupt", "complete"}
    /\ {candidateDirty, up, fileSynced, consumed,
        reused, badReceipt} \subseteq BOOLEAN
    /\ phase \in {"idle", "prepare", "unlink", "sync"}
    /\ syncedDirs \subseteq Paths /\ crashes \in 0..MaxCrashes
ReceiptDurable ==
    /\ ~badReceipt
    /\ sql \in {"done", "terminal"} =>
         /\ (~reused => diskNames = DesiredNames)
         /\ (~NeedsCandidate \/ diskCandidate = "complete")
MoveSourceSurvives == Operation = "move" /\ sql = "pending" =>
    "original" \in {diskNames[p] : p \in Paths}
NewerPathsUntouched == reused => \A p \in Paths: names[p] = "newer"
SuccessAfterReceipt == consumed => sql \in {"done", "terminal"}
Progress == <>(sql = "terminal")
NeverWitness ==
    CASE Witness = "partial-namespace" ->
          ~({"partial-namespace-crash", "retry", "consumed"} \subseteq seen)
      [] Witness = "published-no-receipt" ->
          ~({"published-no-receipt", "restart", "consumed"} \subseteq seen)
      [] Witness = "lost-receipt-ack" ->
          ~({"lost-receipt-ack", "restart", "consumed"} \subseteq seen)
      [] Witness = "path-reuse" -> "replay-after-reuse" \notin seen
      [] OTHER -> TRUE
=============================================================================
