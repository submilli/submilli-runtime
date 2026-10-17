-------------------------- MODULE AppendRecovery --------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS MaxCrashes, CrashMode, Fault, Witness
ASSUME /\ MaxCrashes \in Nat
       /\ CrashMode \in {"process", "power"}
       /\ Fault \in {"none", "fresh-offset", "early-receipt", "path-retry",
                      "early-cleanup", "fresh-read", "unchecked-input"}

(* One exclusive run, one immutable append request, then path replacement.
   SQLite commits are atomic; filesystem writes and persistence are separate.
   The original inode is retained until terminal cleanup. Appending writes at
   the intent's fixed offset, including after an acknowledgement is lost. *)
Original == <<0>>
Payload == <<1, 2>>
Expected == Original \o Payload
Replacement == <<9>>
Byte == {0, 1, 2, 9}
BoundedBytes == UNION { [1..n -> Byte] : n \in 0..5 }

VARIABLES sql, cache, disk, dirty, path, durablePath, pin, up,
          phase, cursor, synced, crashes, consumed, reused, readLog,
          replayRead, replacementBytes, badReceipt, input, seen

vars == <<sql, cache, disk, dirty, path, durablePath, pin, up,
          phase, cursor, synced, crashes, consumed, reused, readLog,
          replayRead, replacementBytes, badReceipt, input, seen>>

(* Request identity, original inode identity and offset are immutable constants
   in this bounded instance. pin represents durable evidence retaining that
   inode, not the payload. Payload bytes are volatile and supplied again by
   deterministic replay of recorded prior outputs. SQL retains only identity. *)
Init ==
    /\ sql = "absent"
    /\ cache = Original
    /\ disk = Original
    /\ dirty = FALSE
    /\ path = "original"
    /\ durablePath = "original"
    /\ pin = FALSE
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
    /\ UNCHANGED <<sql, cache, disk, dirty, path, durablePath, pin, up,
                   phase, cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt>>

Intent ==
    /\ up /\ sql = "absent" /\ input = Payload
    /\ sql' = "pending"
    /\ pin' = TRUE
    /\ phase' \in {"idle", "write"}
    /\ seen' = seen \cup {"intent"}
    /\ UNCHANGED <<cache, disk, dirty, path, durablePath, cursor, synced,
                   crashes, consumed, reused, readLog, replayRead, replacementBytes, badReceipt, up, input>>

Discover ==
    /\ up /\ sql = "pending" /\ phase = "idle" /\ pin
    /\ path = "original"
    /\ phase' = "write"
    /\ cursor' = 0
    /\ synced' = FALSE
    /\ seen' = seen \cup {"retry"}
    /\ UNCHANGED <<sql, cache, disk, dirty, path, durablePath, pin, up,
                   crashes, consumed, reused, readLog, replayRead, replacementBytes, badReceipt, input>>

Max(a, b) == IF a > b THEN a ELSE b
WriteAt(bytes, offset, value) ==
    [i \in 1..Max(Len(bytes), offset + 1) |->
        IF i = offset + 1 THEN value
        ELSE IF i <= Len(bytes) THEN bytes[i] ELSE 0]

WriteChunk ==
    /\ up /\ sql = "pending" /\ phase = "write" /\ pin
    /\ input # <<>>
    /\ path = "original" /\ cursor < Len(Payload)
    /\ LET offset == IF Fault = "fresh-offset" THEN Len(cache)
                     ELSE Len(Original) + cursor
           next == WriteAt(cache, offset, input[cursor + 1])
       IN /\ Len(next) <= 5
          /\ cache' = next
    /\ cursor' = cursor + 1
    /\ dirty' = TRUE
    /\ synced' = FALSE
    /\ seen' = seen \cup {"write"}
    /\ UNCHANGED <<sql, disk, path, durablePath, pin, up, phase, crashes,
                   consumed, reused, readLog, replayRead, replacementBytes, badReceipt, input>>

SyncFile ==
    /\ up /\ sql = "pending" /\ phase = "write"
    /\ cursor = Len(Payload) /\ pin
    /\ disk' = cache
    /\ dirty' = FALSE
    /\ synced' = TRUE
    /\ phase' = "commit"
    /\ seen' = seen \cup {"sync"}
    /\ UNCHANGED <<sql, cache, path, durablePath, pin, up, cursor, crashes,
                   consumed, reused, readLog, replayRead, replacementBytes, badReceipt, input>>

CommitReceipt ==
    /\ up /\ sql = "pending"
    /\ ((phase = "commit" /\ synced)
        \/ (Fault = "early-receipt" /\ cursor = Len(Payload)))
    /\ sql' = "done"
    /\ badReceipt' = (disk # Expected \/ ~pin \/ durablePath # "original")
    /\ phase' \in {"idle", "done"}
    /\ seen' = seen \cup {"receipt"}
    /\ UNCHANGED <<cache, disk, dirty, path, durablePath, pin, up, cursor,
                   synced, crashes, consumed, reused, readLog, replayRead, replacementBytes, input>>

ObserveReceipt ==
    /\ up /\ sql = "done" /\ ~consumed
    /\ consumed' = TRUE
    /\ phase' = "done"
    /\ readLog' = Expected
    /\ seen' = seen \cup {"consumed"}
    /\ UNCHANGED <<sql, cache, disk, dirty, path, durablePath, pin, up,
                   cursor, synced, crashes, reused, replayRead, replacementBytes, badReceipt, input>>

(* A later, separately committed operation reuses the path. It cannot start
   while an earlier effect is pending. The old inode remains pinned for
   reconstructed open resources; retrying the old effect must not use the path. *)
ReplacePath ==
    /\ up /\ sql = "done" /\ consumed /\ ~reused
    /\ path' = "replacement"
    /\ durablePath' = "replacement"
    /\ reused' = TRUE
    /\ replacementBytes' = Replacement
    /\ seen' = seen \cup {"reuse"}
    /\ UNCHANGED <<sql, cache, disk, dirty, pin, up, phase, cursor, synced,
                   crashes, consumed, readLog, replayRead, badReceipt, input>>

Replay ==
    /\ up /\ sql = "done" /\ consumed /\ replayRead = <<>>
    /\ replayRead' = IF Fault = "fresh-read" /\ reused THEN Replacement
                      ELSE readLog
    /\ replacementBytes' = IF Fault = "path-retry" /\ reused
                             THEN replacementBytes \o Payload ELSE replacementBytes
    /\ seen' = seen \cup {"replay"}
                 \cup (IF reused THEN {"replay-after-reuse"} ELSE {})
    /\ UNCHANGED <<sql, cache, disk, dirty, pin, up, phase, cursor, synced,
                   crashes, consumed, reused, readLog, badReceipt, path, durablePath, input>>

(* Dirty appended bytes may reach disk incompletely or with damaged contents.
   The already durable original prefix is outside the write range. A completed
   sync removes the dirty condition, so writeback cannot undo acknowledged data. *)
Writeback ==
    /\ dirty /\ sql = "pending"
    /\ disk' \in {Original \o suffix : suffix \in
                    UNION {[1..n -> {0, 1, 2}] : n \in 0..(Len(cache)-1)}}
    /\ UNCHANGED <<sql, cache, dirty, path, durablePath, pin, up, phase,
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
         \cup (IF sql = "pending" /\ synced THEN {"synced-no-receipt"} ELSE {})
         \cup (IF sql = "done" /\ ~consumed THEN {"lost-receipt-ack"} ELSE {})
    /\ UNCHANGED <<sql, disk, durablePath, pin, consumed, reused, readLog,
                   replacementBytes, badReceipt>>

Restart ==
    /\ ~up
    /\ up' = TRUE
    /\ seen' = seen \cup {"restart"}
    /\ UNCHANGED <<sql, cache, disk, dirty, path, durablePath, pin, phase,
                   cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt, input>>

Terminal ==
    /\ up /\ sql = "done" /\ consumed /\ reused /\ replayRead # <<>>
    /\ sql' = "terminal"
    /\ UNCHANGED <<cache, disk, dirty, path, durablePath, pin, up, phase,
                   cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt, input, seen>>

Cleanup ==
    /\ up /\ pin
    /\ (sql = "terminal" \/ Fault = "early-cleanup")
    /\ pin' = FALSE
    /\ seen' = seen \cup {"cleanup"}
    /\ UNCHANGED <<sql, cache, disk, dirty, path, durablePath, up, phase,
                   cursor, synced, crashes, consumed, reused, readLog,
                   replayRead, replacementBytes, badReceipt, input>>

Next == SupplyInput \/ Intent \/ Discover \/ WriteChunk \/ SyncFile \/ CommitReceipt
        \/ ObserveReceipt \/ ReplacePath \/ Replay \/ Writeback \/ Crash
        \/ Restart \/ Terminal \/ Cleanup
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(SupplyInput) /\ WF_vars(Intent) /\ WF_vars(Discover)
             /\ WF_vars(WriteChunk) /\ WF_vars(SyncFile)
             /\ WF_vars(CommitReceipt) /\ WF_vars(ObserveReceipt)
             /\ WF_vars(ReplacePath) /\ WF_vars(Replay)
             /\ WF_vars(Restart) /\ WF_vars(Terminal) /\ WF_vars(Cleanup)

TypeOK ==
    /\ sql \in {"absent", "pending", "done", "terminal"}
    /\ cache \in BoundedBytes /\ disk \in BoundedBytes
    /\ path \in {"original", "replacement"}
    /\ durablePath \in {"original", "replacement"}
    /\ phase \in {"idle", "write", "commit", "done"}
    /\ cursor \in 0..2 /\ crashes \in 0..MaxCrashes
    /\ {dirty, pin, up, synced, consumed, reused, badReceipt} \subseteq BOOLEAN
    /\ readLog \in {<<>>, Expected}
    /\ replayRead \in {<<>>, Expected, Replacement}
    /\ replacementBytes \in {<<>>, Replacement, Replacement \o Payload}
InputMatches == input \in {<<>>, Payload}
ReceiptIsDurable == ~badReceipt
AppendExactlyOnce == sql \in {"done", "terminal"} => disk = Expected
NewPathUntouched == reused =>
    /\ path = "replacement" /\ durablePath = "replacement"
    /\ replacementBytes = Replacement
ReplayReadStable == replayRead = <<>> \/ replayRead = readLog
RecoveryEvidenceRetained == sql \in {"pending", "done"} => pin
SuccessAfterReceipt == consumed => sql \in {"done", "terminal"}
OriginalPrefixPreserved == Head(cache) = 0 /\ Head(disk) = 0
Progress == <>(sql = "terminal" /\ ~pin)
NeverWitness ==
    CASE Witness = "partial-retry" ->
             ~({"partial-crash", "retry", "consumed"} \subseteq seen)
      [] Witness = "synced-no-receipt" ->
             ~({"synced-no-receipt", "restart", "consumed"} \subseteq seen)
      [] Witness = "lost-receipt-ack" ->
             ~({"lost-receipt-ack", "restart", "consumed"} \subseteq seen)
      [] Witness = "reuse-replay" -> "replay-after-reuse" \notin seen
      [] OTHER -> TRUE
=============================================================================
