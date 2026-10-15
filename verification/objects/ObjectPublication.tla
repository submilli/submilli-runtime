------------------------- MODULE ObjectPublication -------------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS MaxObjects, MaxReaders, ChunkCount, MaxCrashes,
          CrashMode, Lifecycle, AllowAbandon, Fault, Witness

ASSUME /\ MaxObjects > 0 /\ MaxReaders > 0 /\ ChunkCount > 1
       /\ MaxCrashes \in Nat /\ CrashMode \in {"process", "power"}
       /\ Lifecycle \in BOOLEAN /\ AllowAbandon \in BOOLEAN

Objects == 1..MaxObjects
Readers == 1..MaxReaders
Users == {"run", "response"}
Body(o) == [i \in 1..ChunkCount |-> o]
CorruptBody == [i \in 1..ChunkCount |-> 0]
FileContents(o) == UNION {[1..n -> {0, o}] : n \in 0..ChunkCount}
ValidPrefix(bytes, o) == bytes = SubSeq(Body(o), 1, Len(bytes))
EmptyFile == [name |-> "absent", bytes |-> <<>>, dirty |-> FALSE]
EmptyLocal == [known |-> FALSE, fileSynced |-> FALSE, dirSynced |-> FALSE,
               deletionSynced |-> FALSE,
               transaction |-> FALSE, delivered |-> FALSE,
               readers |-> [r \in Readers |-> "none"], workspace |-> "none"]

\* db and disk survive both crash kinds. cache is the kernel-visible filesystem;
\* a process crash preserves it, whereas power loss restores disk's namespace.
\* local includes reader pins: a crashed process has no surviving readers/I/O.
VARIABLES db, cache, disk, local, machine, audit, witnessStage, witnessed
vars == <<db, cache, disk, local, machine, audit, witnessStage, witnessed>>

Init ==
    /\ db = [o \in Objects |-> [phase |-> "absent", gc |-> "available", needs |-> {}]]
    /\ cache = [o \in Objects |-> EmptyFile]
    /\ disk = cache
    /\ local = [o \in Objects |-> EmptyLocal]
    /\ machine = [online |-> TRUE, crashes |-> 0]
    /\ audit = [badCommit |-> FALSE, commits |-> [o \in Objects |-> 0]]
    /\ witnessStage = FALSE /\ witnessed = FALSE

Writing(o) == machine.online /\ db[o].phase = "pending" /\
              db[o].gc = "available" /\ local[o].known
Complete(file, o) == file.name = "published" /\ file.bytes = Body(o)
HasReaders(o) == \E r \in Readers : local[o].readers[r] # "none"
Saw(label, condition) == witnessed \/ (Witness = label /\ condition)

\* A durable identity/pending pin precedes every filesystem write. Commit and
\* discovery are separate, so losing the reservation acknowledgement is safe.
Reserve(o) ==
    /\ machine.online /\ db[o].phase = "absent"
    /\ db' = [db EXCEPT ![o].phase = "pending"]
    /\ UNCHANGED <<cache, disk, local, machine, audit, witnessStage, witnessed>>

Discover(o) ==
    /\ machine.online /\ db[o].phase = "pending" /\ ~local[o].known
    /\ local' = [local EXCEPT ![o].known = TRUE]
    /\ UNCHANGED <<db, cache, disk, machine, audit, witnessStage, witnessed>>

CreateStaging(o) ==
    /\ Writing(o) /\ cache[o].name = "absent"
    /\ cache' = [cache EXCEPT ![o] = [name |-> "staging", bytes |-> <<>>, dirty |-> TRUE]]
    \* A fresh inode replaces an unreachable unsynced inode, never a named file.
    /\ disk' = [disk EXCEPT ![o].bytes = <<>>]
    /\ UNCHANGED <<db, local, machine, audit, witnessStage, witnessed>>

WriteChunk(o) ==
    /\ Writing(o) /\ cache[o].name = "staging"
    /\ ValidPrefix(cache[o].bytes, o) /\ Len(cache[o].bytes) < ChunkCount
    /\ cache' = [cache EXCEPT ![o].bytes = Append(@, o), ![o].dirty = TRUE]
    /\ UNCHANGED <<db, disk, local, machine, audit, witnessStage, witnessed>>

\* Dirty writeback may retain length without corresponding data, or any mixture
\* of written chunks and holes/stale chunks (0). It cannot damage synced bytes.
WriteBackData(o, bytes) ==
    /\ cache[o].name # "absent" /\ cache[o].dirty
    /\ bytes \in FileContents(o) /\ Len(bytes) <= Len(cache[o].bytes)
    /\ bytes # disk[o].bytes
    /\ disk' = [disk EXCEPT ![o].bytes = bytes]
    /\ UNCHANGED <<db, cache, local, machine, audit, witnessStage, witnessed>>

\* After restart, length alone is not a resumable prefix. Failed validation
\* permits rebuilding only a pending staging file, never a published object.
ResetStaging(o) ==
    /\ Writing(o) /\ cache[o].name = "staging"
    /\ ~ValidPrefix(cache[o].bytes, o) /\ Fault # "skip-staging-repair"
    /\ cache' = [cache EXCEPT ![o].bytes = <<>>, ![o].dirty = TRUE]
    /\ witnessStage' = (witnessStage \/
         (Witness = "recover-damaged-staging" /\ o = 1 /\ machine.crashes > 0 /\
          Len(cache[o].bytes) = ChunkCount))
    /\ UNCHANGED <<db, disk, local, machine, audit, witnessed>>

WriteBackName(o) ==
    /\ disk[o].name # cache[o].name
    /\ disk' = [disk EXCEPT ![o].name = cache[o].name,
                   ![o].bytes = IF cache[o].name = "absent" THEN <<>> ELSE @]
    /\ UNCHANGED <<db, cache, local, machine, audit, witnessStage, witnessed>>

SyncFile(o) ==
    /\ Writing(o) /\ cache[o].name # "absent"
    /\ cache[o].bytes = Body(o) /\ ~local[o].fileSynced
    /\ disk' = [disk EXCEPT ![o].bytes = cache[o].bytes]
    /\ cache' = [cache EXCEPT ![o].dirty = FALSE]
    /\ local' = [local EXCEPT ![o].fileSynced = TRUE]
    /\ UNCHANGED <<db, machine, audit, witnessStage, witnessed>>

Publish(o) ==
    /\ Writing(o) /\ cache[o].name = "staging" /\ cache[o].bytes = Body(o)
    /\ local[o].fileSynced \/ Fault = "skip-file-sync"
    /\ cache' = [cache EXCEPT ![o].name = "published"]
    /\ UNCHANGED <<db, disk, local, machine, audit, witnessStage, witnessed>>

SyncPublication(o) ==
    /\ Writing(o) /\ cache[o].name = "published" /\ ~local[o].dirSynced
    /\ disk' = [disk EXCEPT ![o].name = "published"]
    /\ local' = [local EXCEPT ![o].dirSynced = TRUE]
    /\ UNCHANGED <<db, cache, machine, audit, witnessStage, witnessed>>

PrepareReference(o) ==
    /\ Writing(o) /\ ~local[o].transaction /\ cache[o].bytes = Body(o)
    /\ Fault = "early-reference" \/
         (cache[o].name = "published" /\
          (local[o].fileSynced \/ Fault = "skip-file-sync") /\
          (local[o].dirSynced \/ Fault = "skip-directory-sync"))
    /\ local' = [local EXCEPT ![o].transaction = TRUE]
    /\ UNCHANGED <<db, cache, disk, machine, audit, witnessStage, witnessed>>

\* This SQL transaction inserts the reference/event and its retention roots,
\* and consumes the pending pin. Its conditional check is at COMMIT, not prepare.
\* The transaction remains locally uncertain until a later read/forget/crash.
CommitReference(o) ==
    /\ machine.online /\ local[o].transaction /\ db[o].phase # "bound"
    /\ Fault = "late-reference" \/
         (db[o].phase = "pending" /\ db[o].gc = "available")
    /\ db' = [db EXCEPT ![o].phase = "bound", ![o].needs = Users]
    /\ audit' = [audit EXCEPT
         !.badCommit = @ \/ db[o].phase # "pending" \/ db[o].gc # "available",
         !.commits[o] = IF @ < 2 THEN @ + 1 ELSE @]
    /\ witnessed' = (Saw("rollback-retry", o = 1 /\ witnessStage) \/
                      Saw("recover-published", o = 1 /\ witnessStage) \/
                      Saw("recover-damaged-staging", o = 1 /\ witnessStage))
    /\ UNCHANGED <<cache, disk, local, machine, witnessStage>>

ForgetTransaction(o) ==
    /\ machine.online /\ local[o].transaction
    /\ local' = [local EXCEPT ![o].transaction = FALSE]
    /\ witnessStage' = (witnessStage \/
         (Witness = "rollback-retry" /\ o = 1 /\ db[o].phase = "pending"))
    /\ witnessed' = Saw("lost-reference-ack", db[o].phase = "bound")
    /\ UNCHANGED <<db, cache, disk, machine, audit>>

ReadReference(o) ==
    /\ machine.online /\ db[o].phase = "bound" /\ ~local[o].delivered
    /\ Complete(cache[o], o)
    /\ local' = [local EXCEPT ![o].delivered = TRUE]
    /\ witnessed' = Saw("recover-committed", o = 1 /\ witnessStage)
    /\ UNCHANGED <<db, cache, disk, machine, audit, witnessStage>>

\* The per-object gate drains/excludes filesystem I/O before abandonment.
\* A prepared SQL write may remain; its conditional commit must now refuse.
Abandon(o) ==
    /\ AllowAbandon /\ machine.online /\ db[o].phase = "pending"
    /\ db' = [db EXCEPT ![o].phase = "abandoned"]
    /\ witnessStage' = (witnessStage \/
         (Witness = "orphan-collected" /\ o = 1 /\ Complete(disk[o], o)))
    /\ UNCHANGED <<cache, disk, local, machine, audit, witnessed>>

FinishRun(o) ==
    /\ Lifecycle /\ machine.online /\ db[o].phase = "bound"
    /\ local[o].delivered /\ "run" \in db[o].needs
    /\ db' = [db EXCEPT ![o].needs = @ \ {"run"}]
    /\ UNCHANGED <<cache, disk, local, machine, audit, witnessStage, witnessed>>

ExpireResponse(o) ==
    /\ Lifecycle /\ machine.online /\ db[o].phase = "bound"
    /\ "response" \in db[o].needs
    /\ db' = [db EXCEPT ![o].needs = @ \ {"response"}]
    /\ witnessed' = Saw("response-expired-run-retained", "run" \in db[o].needs)
    /\ UNCHANGED <<cache, disk, local, machine, audit, witnessStage>>

RetireReference(o) ==
    /\ Lifecycle /\ machine.online /\ db[o].phase = "bound"
    /\ db[o].needs = {} \/ Fault = "early-retirement"
    /\ db' = [db EXCEPT ![o].phase = "retired"]
    /\ witnessStage' = (witnessStage \/
         (Witness = "reader-retirement" /\ o = 1 /\ HasReaders(o)))
    /\ UNCHANGED <<cache, disk, local, machine, audit, witnessed>>

\* Pin registration and GC's claim share an exclusion gate. Opening the file is
\* deliberately later: an open descriptor alone cannot protect that interval.
PinReader(o, r) ==
    /\ machine.online /\ db[o].phase = "bound" /\ db[o].gc = "available"
    /\ local[o].readers[r] = "none"
    /\ local' = [local EXCEPT ![o].readers[r] = "pinned"]
    /\ UNCHANGED <<db, cache, disk, machine, audit, witnessStage, witnessed>>

OpenReader(o, r) ==
    /\ machine.online /\ local[o].readers[r] = "pinned"
    /\ Complete(cache[o], o)
    /\ local' = [local EXCEPT ![o].readers[r] = "open"]
    /\ witnessed' = Saw("reader-retirement",
                         o = 1 /\ witnessStage /\ db[o].phase = "retired")
    /\ UNCHANGED <<db, cache, disk, machine, audit, witnessStage>>

ReleaseReader(o, r) ==
    /\ machine.online /\ local[o].readers[r] # "none"
    /\ local' = [local EXCEPT ![o].readers[r] = "none"]
    /\ witnessed' = Saw("remaining-reader",
                         db[o].phase = "retired" /\
                         \E other \in Readers \ {r} : local[o].readers[other] # "none")
    /\ UNCHANGED <<db, cache, disk, machine, audit, witnessStage>>

MaterializeWorkspace(o) ==
    /\ machine.online /\ db[o].phase = "bound" /\ local[o].workspace = "none"
    /\ \E r \in Readers : local[o].readers[r] = "open"
    /\ local' = [local EXCEPT ![o].workspace =
         IF Fault = "writable-alias" THEN "alias" ELSE "copy"]
    /\ UNCHANGED <<db, cache, disk, machine, audit, witnessStage, witnessed>>

EditWorkspace(o) ==
    /\ machine.online /\ local[o].workspace \in {"copy", "alias"}
    /\ cache' = IF local[o].workspace = "alias"
                THEN [cache EXCEPT ![o].bytes = CorruptBody, ![o].dirty = TRUE] ELSE cache
    /\ local' = [local EXCEPT ![o].workspace = "edited"]
    /\ UNCHANGED <<db, disk, machine, audit, witnessStage, witnessed>>

\* A durable deletion claim prevents new pins/references before unlink. Tombstones
\* and unique, never-reused object IDs make a restarted deletion unambiguous.
ClaimCollection(o) ==
    /\ machine.online /\ db[o].gc = "available"
    /\ db[o].phase \in {"abandoned", "retired"} \/
         (Fault = "collect-pending" /\ db[o].phase = "pending" /\
          cache[o].name = "published")
    /\ ~HasReaders(o) \/ Fault = "ignore-reader"
    /\ db' = [db EXCEPT ![o].gc = "deleting"]
    /\ UNCHANGED <<cache, disk, local, machine, audit, witnessStage, witnessed>>

Unlink(o) ==
    /\ machine.online /\ db[o].gc = "deleting" /\ cache[o].name # "absent"
    /\ cache' = [cache EXCEPT ![o] = EmptyFile]
    /\ UNCHANGED <<db, disk, local, machine, audit, witnessStage, witnessed>>

SyncDeletion(o) ==
    /\ machine.online /\ db[o].gc = "deleting" /\ cache[o].name = "absent"
    /\ ~local[o].deletionSynced
    /\ disk' = [disk EXCEPT ![o] = EmptyFile]
    /\ local' = [local EXCEPT ![o].deletionSynced = TRUE]
    /\ UNCHANGED <<db, cache, machine, audit, witnessStage, witnessed>>

FinishCollection(o) ==
    /\ machine.online /\ db[o].gc = "deleting" /\ cache[o].name = "absent"
    /\ local[o].deletionSynced \/ Fault = "unsynced-deletion"
    /\ db' = [db EXCEPT ![o].gc = "gone"]
    /\ witnessed' = (Saw("orphan-collected", o = 1 /\ witnessStage) \/
                      Saw("recover-unlink", o = 1 /\ witnessStage))
    /\ UNCHANGED <<cache, disk, local, machine, audit, witnessStage>>

Crash ==
    /\ machine.online /\ machine.crashes < MaxCrashes
    /\ machine' = [machine EXCEPT !.online = FALSE, !.crashes = @ + 1]
    /\ local' = [o \in Objects |-> EmptyLocal]
    /\ cache' = IF CrashMode = "process" THEN cache
                ELSE [o \in Objects |-> IF disk[o].name = "absent"
                                       THEN EmptyFile ELSE disk[o]]
    /\ witnessStage' = (witnessStage \/
         (Witness = "recover-published" /\ db[1].phase = "pending" /\
          Complete(disk[1], 1)) \/
         (Witness = "recover-committed" /\ db[1].phase = "bound" /\
          local[1].transaction) \/
         (Witness = "recover-unlink" /\ db[1].gc = "deleting" /\
          cache[1].name = "absent" /\ disk[1].name = "published"))
    /\ witnessed' = (
         Saw("partial-staging-crash", \E o \in Objects :
             cache[o].name = "staging" /\ Len(cache[o].bytes) \in 1..(ChunkCount - 1)) \/
         Saw("lost-reservation-ack", \E o \in Objects :
             db[o].phase = "pending" /\ ~local[o].known) \/
         Saw("unsynced-rename-crash", CrashMode = "power" /\
             \E o \in Objects : cache[o].name = "published" /\ disk[o].name # "published"))
    /\ UNCHANGED <<db, disk, audit>>

Restart ==
    /\ ~machine.online
    /\ machine' = [machine EXCEPT !.online = TRUE]
    /\ UNCHANGED <<db, cache, disk, local, audit, witnessStage, witnessed>>

Next ==
    \/ \E o \in Objects :
         Reserve(o) \/ Discover(o) \/ CreateStaging(o) \/ WriteChunk(o) \/
         (\E bytes \in FileContents(o) : WriteBackData(o, bytes)) \/ WriteBackName(o) \/
         ResetStaging(o) \/
         SyncFile(o) \/ Publish(o) \/ SyncPublication(o) \/
         PrepareReference(o) \/ CommitReference(o) \/ ForgetTransaction(o) \/
         ReadReference(o) \/ Abandon(o) \/ FinishRun(o) \/ ExpireResponse(o) \/
         RetireReference(o) \/ MaterializeWorkspace(o) \/ EditWorkspace(o) \/
         ClaimCollection(o) \/ Unlink(o) \/ SyncDeletion(o) \/ FinishCollection(o) \/
         (\E r \in Readers : PinReader(o, r) \/ OpenReader(o, r) \/ ReleaseReader(o, r))
    \/ Crash \/ Restart

TypeOK ==
    /\ machine \in [online : BOOLEAN, crashes : 0..MaxCrashes]
    /\ witnessStage \in BOOLEAN /\ witnessed \in BOOLEAN
    /\ audit \in [badCommit : BOOLEAN, commits : [Objects -> 0..2]]
    /\ \A o \in Objects :
         /\ db[o] \in [phase : {"absent", "pending", "bound", "abandoned", "retired"},
                        gc : {"available", "deleting", "gone"}, needs : SUBSET Users]
         /\ local[o] \in [known : BOOLEAN, fileSynced : BOOLEAN, dirSynced : BOOLEAN,
              deletionSynced : BOOLEAN, transaction : BOOLEAN, delivered : BOOLEAN,
              readers : [Readers -> {"none", "pinned", "open"}],
              workspace : {"none", "copy", "alias", "edited"}]
         /\ \A file \in {cache[o], disk[o]} :
              /\ file.name \in {"absent", "staging", "published"}
              /\ file.bytes \in FileContents(o) /\ file.dirty \in BOOLEAN
         /\ ~disk[o].dirty

ReferencedObjectsDurable ==
    \A o \in Objects : db[o].phase = "bound" =>
        db[o].gc = "available" /\ Complete(cache[o], o) /\ Complete(disk[o], o)
PublishedBytesImmutable ==
    \A o \in Objects : \A file \in {cache[o], disk[o]} :
        file.name = "published" => file.bytes = Body(o)
PendingPublicationProtected ==
    \A o \in Objects : db[o].phase = "pending" => db[o].gc = "available"
RequiredReferenceRetained ==
    \A o \in Objects : db[o].needs # {} => db[o].phase = "bound"
ReadersProtected ==
    \A o \in Objects : HasReaders(o) =>
        db[o].gc = "available" /\ Complete(cache[o], o) /\ Complete(disk[o], o)
CollectionComplete ==
    \A o \in Objects : db[o].gc = "gone" =>
        db[o].phase \in {"abandoned", "retired"} /\
        cache[o].name = "absent" /\ disk[o].name = "absent"
NoLateReference == ~audit.badCommit
ReferenceCommittedOnce == \A o \in Objects : audit.commits[o] <= 1
PendingCanAdvance ==
    \A o \in Objects : (machine.online /\ db[o].phase = "pending") =>
        ENABLED (Discover(o) \/ CreateStaging(o) \/ ResetStaging(o) \/ WriteChunk(o) \/
                 SyncFile(o) \/ Publish(o) \/ SyncPublication(o) \/
                 PrepareReference(o) \/ CommitReference(o))
NeverWitness == ~witnessed

Spec == Init /\ [][Next]_vars

\* Finite crashes; the supervisor scans SQL without another client submission.
\* Strong commit fairness excludes an infinite sequence of transaction aborts.
\* Reader-release fairness assumes readers eventually finish or are cancelled.
FairObject(o) ==
    /\ WF_vars(Discover(o)) /\ WF_vars(CreateStaging(o)) /\ WF_vars(WriteChunk(o))
    /\ WF_vars(ResetStaging(o))
    /\ WF_vars(SyncFile(o)) /\ WF_vars(Publish(o)) /\ WF_vars(SyncPublication(o))
    /\ WF_vars(PrepareReference(o)) /\ SF_vars(CommitReference(o))
    /\ WF_vars(ReadReference(o)) /\ WF_vars(FinishRun(o)) /\ WF_vars(ExpireResponse(o))
    /\ WF_vars(RetireReference(o)) /\ WF_vars(ClaimCollection(o))
    /\ WF_vars(Unlink(o)) /\ WF_vars(SyncDeletion(o)) /\ WF_vars(FinishCollection(o))
    /\ \A r \in Readers : WF_vars(ReleaseReader(o, r))
LiveSpec == Spec /\ WF_vars(Restart) /\ (\A o \in Objects : FairObject(o))
PublicationProgress ==
    \A o \in Objects : (db[o].phase = "pending")
        ~> (db[o].phase \in {"bound", "abandoned", "retired"})
CollectionProgress ==
    \A o \in Objects : (db[o].phase \in {"abandoned", "retired"}) ~> (db[o].gc = "gone")
RetentionProgress ==
    Lifecycle => \A o \in Objects : (db[o].phase = "bound") ~> (db[o].phase = "retired")
=============================================================================
