-------------------------- MODULE AppendRecovery --------------------------
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS MaxCrashes, CrashMode, Fault, Witness, InitiallyExists
ASSUME /\ InitiallyExists \in BOOLEAN
       /\ MaxCrashes \in Nat
       /\ CrashMode \in {"process", "power"}
       /\ Fault \in {"none", "fresh-offset", "early-receipt", "path-retry",
                      "early-cleanup", "fresh-read", "unchecked-input", "skip-parent-sync",
                      "refresh-offset"}

(* One exclusive run, one immutable append request, then path replacement.
   SQLite commits are atomic; filesystem writes and persistence are separate.
   The prepared state stores only the original length. Completion replaces it
   with a receipt. Appending writes at the saved offset, including after an
   acknowledgement is lost. The replay-supplied path is resolved on each attempt;
   no resolved entry or directory identity is stored in the append state. *)
Original == IF InitiallyExists THEN <<0>> ELSE <<>>
Payload == <<1, 2>>
Expected == Original \o Payload
Replacement == <<9>>
Byte == {0, 1, 2, 9}
BoundedBytes == UNION { [1..n -> Byte] : n \in 0..5 }

VARIABLES sql, savedOffset, cache, disk, dirty, path, durablePath, journalPresent, up,
          phase, cursor, synced, crashes, consumed, reused, readLog,
          replayRead, replacementBytes, badReceipt, input, seen

vars == <<sql, savedOffset, cache, disk, dirty, path, durablePath, journalPresent, up,
          phase, cursor, synced, crashes, consumed, reused, readLog,
          replayRead, replacementBytes, badReceipt, input, seen>>

(* One owner controls this path until the append result is recorded. A saved
   offset replaces the need to preserve or hash the original file. journalPresent
   represents the SQL row (offset/receipt), not a file link or a payload copy.
   Replay supplies the same volatile input after every restart. *)
Init ==
    /\ sql = "absent"
    /\ savedOffset = -1
    /\ cache = Original
    /\ disk = Original
    /\ dirty = FALSE
    /\ path = IF InitiallyExists THEN "original" ELSE "absent"
    /\ durablePath = path
    /\ journalPresent = FALSE
    /\ up = TRUE
    /\ phase = "idle"
    /\ cursor = 0
    /\ synced = FALSE
    /\ crashes = 0
    /\ consumed = FALSE
    /\ reused = FALSE
    /\ readLog = <<>>
    /\ replayRead = <<>>
    /\ replacementBytes = <<>>
    /\ badReceipt = FALSE
    /\ input = <<>>
    /\ seen = {}

(* SupplyInput accepts already fingerprint-validated replay bytes; rejected
   offers stutter outside this abstraction. Equality stands for the fingerprint
   check. The unchecked-input fault bypasses this acceptance boundary, so
   InputMatches detects the bad acceptance before a public write. No payload
   is persisted in the filesystem intent. WriterRecovery separately models
   explicit wrong offers and their rejection. *)
SupplyInput ==
    /\ up /\ input = <<>> /\ sql \in {"absent", "pending"}
    /\ input' = IF Fault = "unchecked-input" /\ sql = "pending"
                  THEN <<2, 1>> ELSE Payload
    /\ seen' = seen \cup (IF crashes > 0 THEN {"input-replayed"} ELSE {})
    /\ UNCHANGED <<savedOffset, sql, cache, disk, dirty, path, durablePath, journalPresent, up,
                   phase, cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt>>

Intent ==
    /\ up /\ sql = "absent" /\ input = Payload
    /\ sql' = "pending"
    /\ savedOffset' = Len(cache)
    /\ journalPresent' = TRUE
    /\ phase' \in {"idle", "write"}
    /\ seen' = seen \cup {"intent"}
    /\ UNCHANGED <<cache, disk, dirty, path, durablePath, cursor, synced,
                   crashes, consumed, reused, readLog, replayRead, replacementBytes, badReceipt, up, input>>

Discover ==
    /\ up /\ sql = "pending" /\ phase = "idle" /\ journalPresent
    /\ phase' = "write"
    /\ savedOffset' = IF Fault = "refresh-offset" THEN Len(cache) ELSE savedOffset
    /\ cursor' = 0
    /\ synced' = FALSE
    /\ seen' = seen \cup {"retry"}
    /\ UNCHANGED <<sql, cache, disk, dirty, path, durablePath, journalPresent, up,
                   crashes, consumed, reused, readLog, replayRead, replacementBytes, badReceipt, input>>

(* A new append target is recreated if its unsynchronized name was lost.
   The fixed offset is zero; no candidate/backup is needed. *)
CreateFile ==
    /\ up /\ sql = "pending" /\ path = "absent"
    /\ path' = "original" /\ cache' = Original /\ disk' = Original
    /\ UNCHANGED <<savedOffset, sql, dirty, durablePath, journalPresent, up, phase, cursor,
                   synced, crashes, consumed, reused, readLog, replayRead,
                   replacementBytes, badReceipt, input, seen>>

Max(a, b) == IF a > b THEN a ELSE b
WriteAt(bytes, offset, value) ==
    [i \in 1..Max(Len(bytes), offset + 1) |->
        IF i = offset + 1 THEN value
        ELSE IF i <= Len(bytes) THEN bytes[i] ELSE 0]

WriteChunk ==
    /\ up /\ sql = "pending" /\ phase = "write" /\ journalPresent
    /\ input # <<>>
    /\ path = "original" /\ cursor < Len(Payload)
    /\ LET offset == IF Fault = "fresh-offset" THEN Len(cache)
                     ELSE savedOffset + cursor
           next == WriteAt(cache, offset, input[cursor + 1])
       IN /\ Len(next) <= 5
          /\ cache' = next
    /\ cursor' = cursor + 1
    /\ dirty' = TRUE
    /\ synced' = FALSE
    /\ seen' = seen \cup {"write"}
    /\ UNCHANGED <<savedOffset, sql, disk, path, durablePath, journalPresent, up, phase, crashes,
                   consumed, reused, readLog, replayRead, replacementBytes, badReceipt, input>>

SyncFile ==
    /\ up /\ sql = "pending" /\ phase = "write"
    /\ cursor = Len(Payload) /\ journalPresent
    /\ disk' = cache
    /\ dirty' = FALSE
    /\ synced' = TRUE
    /\ phase' = "sync-parent"
    /\ seen' = seen \cup {"sync"}
    /\ UNCHANGED <<savedOffset, sql, cache, path, durablePath, journalPresent, up, cursor, crashes,
                   consumed, reused, readLog, replayRead, replacementBytes, badReceipt, input>>

SyncParent ==
    /\ up /\ sql = "pending" /\ phase = "sync-parent"
    /\ durablePath' = path /\ phase' = "commit"
    /\ UNCHANGED <<savedOffset, sql, cache, disk, dirty, path, journalPresent, up, cursor,
                   synced, crashes, consumed, reused, readLog, replayRead,
                   replacementBytes, badReceipt, input, seen>>

CommitReceipt ==
    /\ up /\ sql = "pending"
    /\ ((phase = "commit" /\ synced)
        \/ (Fault = "skip-parent-sync" /\ phase = "sync-parent" /\ synced)
        \/ (Fault = "early-receipt" /\ cursor = Len(Payload)))
    /\ sql' = "done"
    /\ savedOffset' = -1
    /\ badReceipt' = (disk # Expected \/ ~journalPresent \/ durablePath # "original")
    /\ phase' \in {"idle", "done"}
    /\ seen' = seen \cup {"receipt"}
    /\ UNCHANGED <<cache, disk, dirty, path, durablePath, journalPresent, up, cursor,
                   synced, crashes, consumed, reused, readLog, replayRead, replacementBytes, input>>

ObserveReceipt ==
    /\ up /\ sql = "done" /\ ~consumed
    /\ consumed' = TRUE
    /\ phase' = "done"
    /\ readLog' = Expected
    /\ seen' = seen \cup {"consumed"}
    /\ UNCHANGED <<savedOffset, sql, cache, disk, dirty, path, durablePath, journalPresent, up,
                   cursor, synced, crashes, reused, replayRead, replacementBytes, badReceipt, input>>

(* A later, separately committed operation reuses the path. It cannot start
   while an earlier effect is pending. Retrying the old effect reads its saved
   receipt and never resolves or writes the reused pathname. *)
ReplacePath ==
    /\ up /\ sql = "done" /\ consumed /\ ~reused
    /\ path' = "replacement"
    /\ durablePath' = "replacement"
    /\ reused' = TRUE
    /\ replacementBytes' = Replacement
    /\ seen' = seen \cup {"reuse"}
    /\ UNCHANGED <<savedOffset, sql, cache, disk, dirty, journalPresent, up, phase, cursor, synced,
                   crashes, consumed, readLog, replayRead, badReceipt, input>>

Replay ==
    /\ up /\ sql = "done" /\ consumed /\ replayRead = <<>>
    /\ replayRead' = IF Fault = "fresh-read" /\ reused THEN Replacement
                      ELSE readLog
    /\ replacementBytes' = IF Fault = "path-retry" /\ reused
                             THEN replacementBytes \o Payload ELSE replacementBytes
    /\ seen' = seen \cup {"replay"}
                 \cup (IF reused THEN {"replay-after-reuse"} ELSE {})
    /\ UNCHANGED <<savedOffset, sql, cache, disk, dirty, journalPresent, up, phase, cursor, synced,
                   crashes, consumed, reused, readLog, badReceipt, path, durablePath, input>>

(* Dirty appended bytes may reach disk incompletely or with damaged contents.
   The already durable original prefix is outside the write range. A completed
   sync removes the dirty condition, so writeback cannot undo acknowledged data. *)
Writeback ==
    /\ dirty /\ sql = "pending"
    /\ disk' \in {Original \o suffix : suffix \in
                    UNION {[1..n -> {0, 1, 2}] : n \in 0..(Len(cache)-Len(Original))}}
    /\ UNCHANGED <<savedOffset, sql, cache, dirty, path, durablePath, journalPresent, up, phase,
                   cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt, input, seen>>

NameWriteback ==
    /\ sql = "pending" /\ path # durablePath
    /\ durablePath' = path
    /\ UNCHANGED <<savedOffset, sql, cache, disk, dirty, path, journalPresent, up, phase,
                   cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt, input, seen>>

Crash ==
    /\ up /\ crashes < MaxCrashes
    /\ up' = FALSE
    /\ crashes' = crashes + 1
    /\ cache' = IF CrashMode = "power" THEN disk ELSE cache
    /\ path' = IF CrashMode = "power" THEN durablePath ELSE path
    /\ dirty' = IF CrashMode = "power" THEN FALSE ELSE dirty
    /\ phase' = "idle"
    /\ cursor' = 0
    /\ synced' = FALSE
    /\ input' = <<>>
    /\ replayRead' = <<>>
    /\ seen' = seen \cup
         (IF sql = "pending" /\ cache # Original THEN {"partial-crash"} ELSE {})
         \cup (IF ~InitiallyExists /\ sql = "pending" /\ durablePath = "original"
                     /\ disk # Expected THEN {"new-name-partial"} ELSE {})
         \cup (IF sql = "pending" /\ synced THEN {"synced-no-receipt"} ELSE {})
         \cup (IF sql = "done" /\ ~consumed THEN {"lost-receipt-ack"} ELSE {})
    /\ UNCHANGED <<savedOffset, sql, disk, durablePath, journalPresent, consumed, reused, readLog,
                   replacementBytes, badReceipt>>

Restart ==
    /\ ~up
    /\ up' = TRUE
    /\ seen' = seen \cup {"restart"}
    /\ UNCHANGED <<savedOffset, sql, cache, disk, dirty, path, durablePath, journalPresent, phase,
                   cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt, input>>

Terminal ==
    /\ up /\ sql = "done" /\ consumed /\ reused /\ replayRead # <<>>
    /\ sql' = "terminal"
    /\ UNCHANGED <<savedOffset, cache, disk, dirty, path, durablePath, journalPresent, up, phase,
                   cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt, input, seen>>

Cleanup ==
    /\ up /\ journalPresent
    /\ (sql = "terminal" \/ Fault = "early-cleanup")
    /\ journalPresent' = FALSE
    /\ seen' = seen \cup {"cleanup"}
    /\ UNCHANGED <<savedOffset, sql, cache, disk, dirty, path, durablePath, up, phase,
                   cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt, input>>

(* One loop iteration dispatches on the saved journal state. The actions inside
   Prepared are separate machine steps: a crash can occur between any two.
   Initial abstracts admission, file/parent sync and saving the offset (Intent). *)
DecodeState == CASE sql = "absent" -> "Initial"
                 [] sql = "pending" -> "Prepared"
                 [] sql = "done" -> "Completed"
                 [] OTHER -> "Retired"
AppendLoop ==
    CASE DecodeState = "Initial" -> Intent
      [] DecodeState = "Prepared" -> Discover \/ CreateFile \/ WriteChunk \/ SyncFile \/ SyncParent \/ CommitReceipt
      [] DecodeState = "Completed" -> ObserveReceipt \/ Replay
      [] OTHER -> FALSE
(* Storage, caller replay/retention and failures can interleave with the loop.
   They are not made atomic with a state arm or a journal commit. *)
Next == AppendLoop \/ SupplyInput \/ ReplacePath \/ Writeback \/ NameWriteback \/ Crash
        \/ Restart \/ Terminal \/ Cleanup
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(SupplyInput) /\ WF_vars(Intent) /\ WF_vars(Discover)
             /\ WF_vars(CreateFile) /\ WF_vars(WriteChunk) /\ WF_vars(SyncFile) /\ WF_vars(SyncParent)
             /\ WF_vars(CommitReceipt) /\ WF_vars(ObserveReceipt)
             /\ WF_vars(ReplacePath) /\ WF_vars(Replay)
             /\ WF_vars(Restart) /\ WF_vars(Terminal) /\ WF_vars(Cleanup)

TypeOK ==
    /\ sql \in {"absent", "pending", "done", "terminal"}
    /\ savedOffset \in -1..5
    /\ cache \in BoundedBytes /\ disk \in BoundedBytes
    /\ path \in {"absent", "original", "replacement"}
    /\ durablePath \in {"absent", "original", "replacement"}
    /\ phase \in {"idle", "write", "sync-parent", "commit", "done"}
    /\ cursor \in 0..2 /\ crashes \in 0..MaxCrashes
    /\ {dirty, journalPresent, up, synced, consumed, reused, badReceipt} \subseteq BOOLEAN
    /\ readLog \in {<<>>, Expected}
    /\ replayRead \in {<<>>, Expected, Replacement}
    /\ replacementBytes \in {<<>>, Replacement, Replacement \o Payload}
InputMatches == input \in {<<>>, Payload}
OriginalOffsetRetained == sql = "pending" => savedOffset = Len(Original)
ReceiptIsDurable == ~badReceipt
AppendExactlyOnce == sql \in {"done", "terminal"} => disk = Expected
NewPathUntouched == reused =>
    /\ path = "replacement" /\ durablePath = "replacement"
    /\ replacementBytes = Replacement
ReplayReadStable == replayRead = <<>> \/ replayRead = readLog
JournalRetained == sql \in {"pending", "done"} => journalPresent
SuccessAfterReceipt == consumed => sql \in {"done", "terminal"}
OriginalPrefixPreserved ==
    /\ SubSeq(cache, 1, Len(Original)) = Original
    /\ SubSeq(disk, 1, Len(Original)) = Original
Progress == <>(sql = "terminal" /\ ~journalPresent)
NeverWitness ==
    CASE Witness = "partial-retry" ->
             ~({"partial-crash", "retry", "consumed"} \subseteq seen)
      [] Witness = "synced-no-receipt" ->
             ~({"synced-no-receipt", "restart", "consumed"} \subseteq seen)
      [] Witness = "lost-receipt-ack" ->
             ~({"lost-receipt-ack", "restart", "consumed"} \subseteq seen)
      [] Witness = "reuse-replay" -> "replay-after-reuse" \notin seen
      [] Witness = "new-name-partial" ->
             ~({"new-name-partial", "restart", "consumed"} \subseteq seen)
      [] OTHER -> TRUE
=============================================================================
