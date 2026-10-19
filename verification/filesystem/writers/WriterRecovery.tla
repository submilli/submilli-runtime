--------------------------- MODULE WriterRecovery ---------------------------
EXTENDS Naturals, Sequences, TLC
CONSTANTS MaxCrashes, MaxCancels, Fault, Witness
ASSUME Fault \in {"none", "fresh-offset", "early-receipt", "unchecked-input",
                  "repeat-close", "close-collision", "history-first-chunk",
                  "history-first-close", "early-close-receipt", "early-writer-closed",
                  "finalizer-deletes", "finalizer-publishes", "open-history-first", "hydrate-resets-stage"}
VARIABLE s
vars == <<s>>
Payload == <<1, 2>>
Fingerprint(byte) == IF byte = 1 THEN "hash-one" ELSE IF byte = 2 THEN "hash-two" ELSE "wrong"
Init == s = [up |-> TRUE, crashes |-> 0, cancels |-> 0, row |-> FALSE, openHistory |-> FALSE,
             native |-> FALSE, finalized |-> FALSE, released |-> FALSE,
             receipts |-> <<>>, history |-> 0, cursor |-> 0,
             cache |-> <<>>, disk |-> <<>>, input |-> 0,
             written |-> FALSE, synced |-> FALSE, ready |-> FALSE,
             closing |-> FALSE, owner |-> "none", intent |-> FALSE,
             target |-> <<>>, targetDisk |-> <<>>, targetSynced |-> FALSE,
             publishedByClose |-> FALSE, applied |-> FALSE, outcome |-> "none", closed |-> FALSE,
             closeReady |-> FALSE, closeHistory |-> "none", returned |-> FALSE,
             replaced |-> FALSE, badInput |-> FALSE, seen |-> {}]
(* Open admission and the framework's resource-valued result are separate.
   Rehydration from the recorded ID must not initialize a new staged output. *)
OpenWriter == /\ s.up /\ ~s.row /\ ~s.released
              /\ s' = [s EXCEPT !.row = TRUE]
CommitOpen == /\ s.up /\ ~s.openHistory /\ (s.row \/ Fault = "open-history-first")
              /\ s' = [s EXCEPT !.openHistory = TRUE]
Hydrate == /\ s.up /\ s.row /\ s.openHistory /\ ~s.released /\ ~s.native
           /\ s' = [s EXCEPT !.native = TRUE,
                             !.disk = IF Fault = "hydrate-resets-stage" /\ s.crashes > 0 THEN <<>> ELSE s.disk,
                             !.cache = IF Fault = "hydrate-resets-stage" /\ s.crashes > 0 THEN <<>> ELSE s.cache,
                             !.seen = @ \cup (IF s.crashes > 0 THEN {"hydrated-id"} ELSE {}) \cup
                               (IF "skipped-id-only" \in s.seen THEN {"hydrated-after-skip"} ELSE {})]
SkipHistory == /\ s.up /\ s.cursor < s.history
               /\ s' = [s EXCEPT !.cursor = @ + 1,
                                 !.seen = @ \cup (IF s.crashes > 0 THEN {"skipped-chunk"} ELSE {}) \cup
                                   (IF s.crashes > 0 /\ ~s.native THEN {"skipped-id-only"} ELSE {})]
Supply == /\ s.up /\ s.row /\ s.native /\ ~s.released
          /\ s.cursor = s.history /\ s.cursor < 2 /\ s.input = 0 /\ ~s.ready
          /\ s' = [s EXCEPT !.input = Payload[s.cursor + 1]]
WrongReplay == /\ s.up /\ s.native /\ s.cursor = s.history
               /\ s.cursor < Len(s.receipts) /\ s.input = 0 /\ ~s.ready
               /\ s' = [s EXCEPT !.input = 9]
ReplayChunk == /\ s.up /\ s.row /\ s.native /\ s.cursor = s.history
               /\ s.cursor < Len(s.receipts) /\ s.input # 0
               /\ (Fingerprint(s.input) = s.receipts[s.cursor + 1] \/ Fault = "unchecked-input")
               /\ s' = [s EXCEPT !.ready = TRUE, !.input = 0,
                                 !.badInput = @ \/ Fingerprint(s.input) # s.receipts[s.cursor + 1],
                                 !.seen = @ \cup {"chunk-receipt-replayed"}]
RejectInput == /\ s.up /\ s.cursor < Len(s.receipts) /\ s.input # 0
               /\ Fingerprint(s.input) # s.receipts[s.cursor + 1]
               /\ s' = [s EXCEPT !.input = 0, !.seen = @ \cup {"mismatch-refused"}]
Stage == /\ s.up /\ s.row /\ s.native /\ ~s.closing /\ ~s.closed
         /\ s.cursor = s.history /\ s.cursor = Len(s.receipts) /\ s.cursor < 2
         /\ s.input = Payload[s.cursor + 1] /\ ~s.written /\ ~s.ready /\ Len(s.cache) < 4
         /\ s' = [s EXCEPT !.cache = Append(IF Fault = "fresh-offset" THEN s.cache ELSE SubSeq(s.cache, 1, Len(s.receipts)), s.input),
                           !.written = TRUE, !.synced = FALSE]
SyncStage == /\ s.up /\ s.written /\ ~s.synced
             /\ s' = [s EXCEPT !.disk = s.cache, !.synced = TRUE]
Receipt == /\ s.up /\ s.written /\ (s.synced \/ Fault = "early-receipt")
           /\ s' = [s EXCEPT !.receipts = Append(@, Fingerprint(s.input)),
                             !.ready = TRUE, !.input = 0, !.written = FALSE, !.synced = FALSE]
CommitChunkEffect == /\ s.up /\ s.cursor = s.history /\ s.cursor < 2
                     /\ (s.ready \/ (Fault = "history-first-chunk" /\ s.written))
                     /\ s' = [s EXCEPT !.history = @ + 1, !.ready = FALSE]
(* Cancellation drains native work but keeps staging, receipts and the ID. *)
Cancel == /\ s.up /\ s.cancels < MaxCancels
          /\ (s.written \/ s.ready \/ (s.closing /\ s.closeHistory = "none"))
          /\ s' = [s EXCEPT !.cancels = @ + 1, !.input = 0, !.written = FALSE,
                            !.synced = FALSE, !.ready = FALSE, !.closeReady = FALSE,
                            !.seen = @ \cup {"cancelled"} \cup
                              (IF s.written THEN {"cancel-tail"} ELSE {}) \cup
                              (IF s.ready THEN {"cancel-receipt"} ELSE {}) \cup
                              (IF s.closing THEN {"cancel-close"} ELSE {})]
ReserveClose == /\ s.up /\ s.row /\ s.native /\ s.cursor = 2 /\ s.history = 2
                /\ Len(s.receipts) = 2 /\ ~s.closing /\ s.disk = Payload
                /\ s' = [s EXCEPT !.closing = TRUE]
AdmitClose == /\ s.up /\ s.native /\ s.closing /\ s.owner = "none" /\ s.cursor = 2
              /\ s' = [s EXCEPT !.owner = "writer"]
OtherMutation == /\ s.up /\ s.closing /\ s.owner = "none"
                 /\ s' = [s EXCEPT !.owner = IF Fault = "close-collision" THEN "other" ELSE s.owner,
                                   !.seen = @ \cup {"other-call-refused"}]
PrepareClose == /\ s.up /\ s.native /\ s.owner # "none" /\ ~s.intent /\ s.outcome = "none"
                /\ s' = [s EXCEPT !.intent = TRUE]
RejectClose == /\ s.up /\ s.native /\ s.owner = "writer" /\ ~s.intent /\ s.outcome = "none"
               /\ s' = [s EXCEPT !.outcome = "rejected"]
(* Namespace details are checked separately; publication and sync remain
   separate here from the filesystem receipt and the framework completion. *)
Publish == /\ s.up /\ s.native /\ s.intent /\ ~s.applied /\ s.outcome = "none"
           /\ s' = [s EXCEPT !.target = IF s.owner = "writer" THEN Payload ELSE s.target,
                             !.publishedByClose = TRUE, !.applied = TRUE, !.targetSynced = FALSE]
SyncTarget == /\ s.up /\ s.native /\ s.applied /\ s.outcome = "none" /\ ~s.targetSynced
              /\ s' = [s EXCEPT !.targetDisk = s.target, !.targetSynced = TRUE]
CloseReceipt == /\ s.up /\ s.intent /\ s.outcome = "none"
                /\ (s.targetSynced \/ Fault = "early-close-receipt")
                /\ s' = [s EXCEPT !.outcome = "done"]
MarkClosed == /\ s.up /\ s.closing /\ ~s.closed
              /\ (s.outcome # "none" \/ Fault = "early-writer-closed")
              /\ s' = [s EXCEPT !.closed = TRUE]
ReturnClose == /\ s.up /\ s.native /\ s.cursor = 2 /\ s.closed /\ s.outcome # "none"
               /\ s' = [s EXCEPT !.closeReady = TRUE,
                                 !.seen = @ \cup (IF s.crashes > 0 THEN {"close-receipt-replayed"} ELSE {})]
CommitCloseEffect == /\ s.up /\ s.closeHistory = "none"
                     /\ (s.closeReady \/ (Fault = "history-first-close" /\ s.closing))
                     /\ s' = [s EXCEPT !.closeHistory = IF s.outcome = "none" THEN "done" ELSE s.outcome]
DeliverClose == /\ s.up /\ s.cursor = 2 /\ s.closeHistory # "none" /\ ~s.returned
                /\ s' = [s EXCEPT !.returned = TRUE]
Reuse == /\ s.up /\ s.returned /\ s.closeHistory # "none" /\ ~s.replaced /\ ~s.released
         /\ s' = [s EXCEPT !.target = <<9>>, !.targetDisk = <<9>>, !.replaced = TRUE]
ReplayClose == /\ s.up /\ s.row /\ s.native /\ s.replaced /\ s.outcome # "none"
               /\ s' = [s EXCEPT !.target = IF Fault = "repeat-close" THEN Payload ELSE s.target,
                                 !.targetDisk = IF Fault = "repeat-close" THEN Payload ELSE s.targetDisk,
                                 !.seen = @ \cup {"replayed-after-reuse"}]
Finalize == /\ s.up /\ s.native /\ ~s.written /\ ~s.finalized
            /\ s' = [s EXCEPT !.native = FALSE, !.finalized = TRUE,
                              !.row = IF Fault = "finalizer-deletes" THEN FALSE ELSE s.row,
                              !.target = IF Fault = "finalizer-publishes" /\ ~s.closing THEN s.cache ELSE s.target,
                              !.seen = @ \cup {"finalized"} \cup
                                (IF s.closeHistory = "none" THEN {"finalized-pending"} ELSE {})]
Release == /\ s.up /\ s.returned /\ s.closeHistory # "none" /\ ~s.released
           /\ s' = [s EXCEPT !.released = TRUE]
DropNative == /\ s.up /\ s.released /\ s.native
              /\ s' = [s EXCEPT !.native = FALSE]
Collect == /\ s.up /\ s.released /\ ~s.native /\ s.row
           /\ s' = [s EXCEPT !.row = FALSE, !.cache = <<>>, !.disk = <<>>]
Crash == /\ s.up /\ s.crashes < MaxCrashes
         /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1, !.cache = s.disk,
                           !.native = FALSE, !.cursor = 0, !.input = 0,
                           !.written = FALSE, !.synced = FALSE, !.ready = FALSE,
                           !.target = s.targetDisk, !.applied = FALSE, !.targetSynced = FALSE, !.closeReady = FALSE,
                           !.seen = @ \cup
                             (IF s.row /\ ~s.openHistory THEN {"open-history-gap"} ELSE {}) \cup
                             (IF Len(s.disk) > Len(s.receipts) THEN {"unacknowledged-tail"} ELSE {}) \cup
                             (IF Len(s.receipts) > s.history THEN {"chunk-history-gap"} ELSE {}) \cup
                             (IF s.closing /\ s.owner = "none" THEN {"reserved-close-crash"} ELSE {}) \cup
                             (IF s.publishedByClose /\ s.outcome = "none" THEN {"publication-gap"} ELSE {}) \cup
                             (IF s.outcome # "none" /\ s.closeHistory = "none" THEN {"close-history-gap"} ELSE {})]
Restart == /\ ~s.up /\ s' = [s EXCEPT !.up = TRUE]
(* Decode each call independently; there is no persisted executable plan.
   Chunk Initial contains native staging/sync substeps. Staged is volatile;
   only the fingerprint receipt makes Completed durable. *)
OpenWriterState == IF s.row THEN "Completed" ELSE "Initial"
WriteChunkState == CASE s.cursor < Len(s.receipts) -> "Completed"
                       [] s.written /\ s.synced -> "Staged"
                       [] OTHER -> "Initial"
(* Close retains only a Prepared marker. Its writer row supplies the target
   path and the reserved close ID locates the private candidate; neither a
   resolved Entry nor the candidate inode is persisted in the close journal. *)
CloseWriterState == CASE s.outcome # "none" -> "Completed"
                        [] s.intent -> "Prepared"
                        [] OTHER -> "Initial"
OpenWriterLoop == CASE OpenWriterState = "Initial" -> OpenWriter
                      [] OTHER -> FALSE
WriteChunkLoop == CASE WriteChunkState = "Initial" -> Stage \/ SyncStage
                      [] WriteChunkState = "Staged" -> Receipt
                      [] WriteChunkState = "Completed" -> ReplayChunk
                      [] OTHER -> FALSE
CloseWriterLoop == CASE CloseWriterState = "Initial" -> ReserveClose \/ AdmitClose \/ PrepareClose \/ RejectClose
                       [] CloseWriterState = "Prepared" -> Publish \/ SyncTarget \/ CloseReceipt
                       [] CloseWriterState = "Completed" -> MarkClosed \/ ReturnClose \/ ReplayClose
                       [] OTHER -> FALSE
(* Keep faulty ordering reachable: dispatch must not hide the negative controls. *)
FaultyOrdering == (Fault = "early-receipt" /\ Receipt)
                 \/ (Fault = "early-writer-closed" /\ MarkClosed)
FrameworkStep == CommitOpen \/ Hydrate \/ SkipHistory \/ Supply \/ CommitChunkEffect
                 \/ CommitCloseEffect \/ DeliverClose
Core == OpenWriterLoop \/ WriteChunkLoop \/ CloseWriterLoop \/ FaultyOrdering
        \/ FrameworkStep \/ Cancel \/ Reuse \/ Finalize
        \/ Release \/ DropNative \/ Collect \/ Crash \/ Restart
Next == Core \/ WrongReplay \/ RejectInput \/ OtherMutation
Spec == Init /\ [][Next]_vars
LiveSpec == Init /\ [][Core]_vars /\ WF_vars(OpenWriter) /\ WF_vars(CommitOpen) /\ WF_vars(Hydrate) /\ WF_vars(SkipHistory)
            /\ WF_vars(Supply) /\ WF_vars(ReplayChunk) /\ WF_vars(Stage) /\ WF_vars(SyncStage)
            /\ WF_vars(Receipt) /\ WF_vars(CommitChunkEffect) /\ WF_vars(ReserveClose)
            /\ WF_vars(AdmitClose) /\ WF_vars(PrepareClose) /\ WF_vars(Publish)
            /\ WF_vars(SyncTarget) /\ WF_vars(CloseReceipt) /\ WF_vars(MarkClosed)
            /\ WF_vars(ReturnClose) /\ WF_vars(CommitCloseEffect) /\ WF_vars(DeliverClose)
            /\ WF_vars(Release) /\ WF_vars(DropNative) /\ WF_vars(Collect) /\ WF_vars(Restart)
TypeOK == /\ s.cursor \in 0..2 /\ s.history \in 0..2 /\ Len(s.receipts) <= 2 /\ Len(s.cache) <= 4
          /\ s.crashes \in 0..MaxCrashes /\ s.cancels \in 0..MaxCancels /\ s.input \in {0,1,2,9}
          /\ s.outcome \in {"none", "done", "rejected"} /\ s.closeHistory \in {"none", "done", "rejected"}
Retained == s.openHistory /\ ~s.released => s.row
DurableChunks == s.row => \A i \in 1..Len(s.receipts): i <= Len(s.disk) /\ Fingerprint(s.disk[i]) = s.receipts[i]
ChunkHistoryBacked == s.history <= Len(s.receipts)
CloseHistoryBacked == s.closeHistory # "none" => s.outcome = s.closeHistory /\ s.closed
ClosedAfterOutcome == s.closed => s.outcome # "none"
SameBytes == ~s.badInput
NoDuplicateTail == Len(s.disk) <= Len(s.receipts) + 1 /\ Len(s.disk) <= 2
NoRepublish == s.replaced => s.target = <<9>> /\ s.targetDisk = <<9>>
ClosePublishesOutput == s.outcome = "done" /\ ~s.replaced => s.targetDisk = Payload
NoImplicitPublication == s.target # <<>> /\ ~s.replaced => s.publishedByClose
Progress == <>(s.released /\ ~s.native /\ ~s.row)
NeverWitness == CASE Witness = "open" -> ~("open-history-gap" \in s.seen /\ s.returned)
                   [] Witness = "tail" -> ~("unacknowledged-tail" \in s.seen /\ s.returned)
                   [] Witness = "chunk-gap" -> ~({"chunk-history-gap", "chunk-receipt-replayed"} \subseteq s.seen /\ s.returned)
                   [] Witness = "skip" -> ~({"skipped-id-only", "hydrated-after-skip"} \subseteq s.seen /\ s.returned)
                   [] Witness = "publication" -> ~("publication-gap" \in s.seen /\ s.outcome = "done" /\ s.returned)
                   [] Witness = "close-gap" -> ~({"close-history-gap", "close-receipt-replayed"} \subseteq s.seen /\ s.returned)
                   [] Witness = "reservation" -> ~("reserved-close-crash" \in s.seen /\ "other-call-refused" \in s.seen /\ s.closed)
                   [] Witness = "cancel" -> ~("cancelled" \in s.seen /\ s.returned)
                   [] Witness = "cancel-tail" -> ~("cancel-tail" \in s.seen /\ s.returned)
                   [] Witness = "cancel-receipt" -> ~("cancel-receipt" \in s.seen /\ s.returned)
                   [] Witness = "cancel-close" -> ~("cancel-close" \in s.seen /\ s.returned)
                   [] Witness = "finalize" -> ~("finalized-pending" \in s.seen /\ s.returned /\ ~s.row)
                   [] Witness = "rejected" -> ~(s.outcome = "rejected" /\ s.returned)
                   [] Witness = "reuse" -> "replayed-after-reuse" \notin s.seen
                   [] OTHER -> "mismatch-refused" \notin s.seen
=============================================================================
