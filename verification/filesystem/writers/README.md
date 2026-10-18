# Staged writer output and execution history

`WriterRecovery.tla` checks one writer with two chunks, a close, a bounded crash
and cancellation. The staging file is output-in-progress. SQLite stores writer
identity/state, chunk IDs, sequence, lengths/fingerprints, and the close outcome;
no input payload bytes are copied to SQL. Earlier execution outputs reconstruct
write arguments, abstracted by `Payload`.

The old model had filesystem receipts but no independent framework history. It
replayed all chunk calls after restart and collapsed publication/sync/close
receipt into one action. It therefore could not demonstrate skipped chunk calls,
a chunk receipt ahead of history, or a close receipt ahead of history. Those
boundaries are explicit in the revised model.

## Commit boundaries

| Boundary | Model actions | Required behavior |
| --- | --- | --- |
| Open | `OpenWriter`, `CommitOpen` | Commit the writer row before recording its ID as an execution result. |
| Restore | `Hydrate`, `SkipHistory` | Rebuild from the recorded writer ID. Skipped chunks need no native handle; hydration must not reset staged output. |
| Chunk | `Stage`, `SyncStage`, `Receipt`, `CommitChunkEffect` | Persist staged bytes, then the chunk receipt, then framework completion. |
| Close admission | `ReserveClose`, `AdmitClose`, `PrepareClose` | Reserve the close ID for this writer; record its immutable operation and prepared intent before publication. |
| Close success | `Publish`, `SyncTarget`, `CloseReceipt`, `MarkClosed`, `ReturnClose`, `CommitCloseEffect` | File persistence, filesystem outcome, writer closed-state and execution completion are distinct boundaries. |
| Close refusal | `RejectClose`, `MarkClosed`, `ReturnClose`, `CommitCloseEffect` | Only preflight refusal is definitive; record and replay it without publication. |
| Lifetime | `Finalize`, `Release`, `DropNative`, `Collect` | Finalization releases local resources only; durable retirement authorizes deletion later. |

New chunks write at the sum of acknowledged chunk lengths, replacing any
unacknowledged tail. `SyncStage` persists that output before the receipt. An
identical chunk ID/fingerprint returns the receipt without writing. Thus a crash
between the chunk receipt and framework completion does not append again.
If framework history already contains the chunk, replay skips its backend call;
the first later call derives its offset from the existing receipts. No extra
reader-style before/after cursor pair is needed for chunks.

Close validates acknowledged staging and fences further new chunks. Only this
writer's close can use its reserved effect ID. Publication, persistence and
outcome commits are distinct. A saved outcome repairs an interrupted transition
to `closed` and returns the same success/refusal without needing to republish.
Framework completion happens after that. A duplicate close cannot overwrite a
later file at the target pathname. Local `closed` flags are caches, not authority.

`Publish` abstracts exclusive-owner replacement publication supplied by the separate
namespace model. It must revalidate/reconcile after a crash before synchronizing
and committing an outcome; an earlier attempt's publication flag is insufficient.
This composition does not reproduce every rename/candidate detail. The replacement keeps only its new candidate,
not an original-destination backup or hash. Staging and chunk receipts remain
necessary because execution replay may skip already-recorded chunk calls.
The close journal's `Prepared` state is a marker with no fields: the writer row
supplies its target path, and the close effect ID locates the synchronized
candidate. Retry resolves those locations again. It does not need saved parent
or candidate inode identities. Writer rows and chunk receipts still retain the
resource information needed when framework replay skips earlier calls.

Cancellation drains owned work before retry. It can leave an unacknowledged tail,
a receipt without framework completion, or a pending close. The same IDs and
staging must remain usable. Finalization never publishes and never deletes the
writer row, chunk receipts, close outcome or staging needed by replay. Eventual
cleanup needs a durable no-further-use decision; it is not a destructor or a
startup scanner. The model's successful retirement follows a recorded terminal
close; abandoning an unfinished writer requires a separate durable lifecycle rule.

## Verification and implementation status

Checks cover durable acknowledged chunks, fixed offsets, fingerprint validation,
receipt-before-history, close-outcome-before-closed-state, persistence-before-
outcome, reserved close identity, no republication, no implicit publication on
finalization, retained rows and progress. Negative variants remove those guards.
Witnesses require open/chunk/close history gaps, publication-before-receipt,
ID-only history skipping before hydration, cancellation at chunk/close boundaries,
finalization during unfinished work, refused close and pathname reuse.

The low-level Rust chunk and close protocols already provide IDs and receipts.
The generic trait now exposes a typed writer descriptor and ID-only restoration.
A new local writer object does not create a new logical writer or truncate staging.
SQLite integration tests skip recorded chunk calls, retry unrecorded receipts and
restore close outcomes. The remaining integration is the execution framework
providing stable IDs and recording/skipping results. See the
[resource plan and implementation mapping](../resources/README.md).

Liveness assumes eventual correct replay bytes, finite crashes/cancellations,
fair storage work, exclusive ownership and serialized framework dispatch. Wrong
arguments are refused, not promised progress. This does not prove Rust/SQLite,
cryptography, physical device behavior, multi-node coordination or full execution
integration. Existing process tests cover primitive storage gaps; new tests must
add framework commit gaps, skipped calls and ID-only handles.

```sh
python3 verification/check.py --model filesystem/writers
```
