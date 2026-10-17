# Reader progress across execution replay

This records the protocol and implementation plan for effect-aware resource
handles. The filesystem traits and SQLite adapter now implement per-effect
reader/listing progress and ID-only restoration. Production execution history,
replay-stable ID assignment and guest-run restart remain SUB-1837 work; ordinary
runs still use the default backend.

## The missing boundary

The previous model made `RecordRead` an atomic update of the observation and its
cursor in execution history. It assumed the integration that should have been
checked. It had one read and no separate filesystem-progress commit, no dispatch
of the first new read after skipping recorded reads, and no attempt finalization
while a read remained unrecorded. That omitted the failure the user identified.

The new model separates physical reading, filesystem progress, framework effect
completion, result delivery, replay, native-handle finalization and durable
resource release. Filesystem progress contains positions and identity, not a
copy of the read result. The framework alone records result bytes.

## Protocol

The framework supplies a stable reader ID from the recorded open result, and a
stable effect ID for each logical read. A native file descriptor is only a local
cache; an ID-only handle is valid after replay and opens lazily on first use.
An ordinary read is not a new resource: its effect ID differs from its reader ID.

The reader row contains the scope/reader ID, configured path and filesystem
binding, expected file identity, immutable reader options, and the last read's
effect ID, request fingerprint, before-state and after-state. State includes the
logical byte offset and decoding flags needed for line readers. The existing
pending-open/error representation remains until open admission completes.
No descriptor, read buffer, file input bytes or duplicate read result is stored.

For a new effect ID, begin at the saved after-state. Always seek from that saved
state, even if a native handle is already open: a cancelled read may have moved
its cursor without committing progress. For the same effect ID,
validate the request and begin at the saved before-state. Read the logical item,
then atomically save its ID, request fingerprint and before/after states in the
filesystem unit of work before returning. Repeating the same effect rewrites the
same transition, not another advancement. A lost progress-commit acknowledgement
is resolved by rereading the row. The framework commits the result separately.

Example: E6 reads bytes 500..600 and commits that progress. A crash before the
framework commits E6 loses the native handle. Replay skips recorded E1..E5,
reconstructs reader R from its recorded ID, then dispatches E6 again. R seeks to
500 and returns that range again, keeping its after-position at 600. If E6 was
already recorded, replay skips it and E7 begins at 600.

### Required framework contract

- Dispatch one logical operation at a time for a reader. Do not dispatch a new
  effect ID until the previous effect's result has committed to history. IDs are
  opaque; the integer IDs in TLA+ represent order, not a numeric API requirement.
- Dispatch only the first unrecorded call or its identical retry. Older committed
  calls return history values without entering the filesystem. A single last
  transition cannot support arbitrary out-of-order old-ID retries.
- Prevent conflicting filesystem mutation while a read result is not yet
  recorded, including other tasks in the same run. Single-run ownership alone
  does not prevent intra-run concurrency. Drain previous native work on pause or
  cancellation before permitting a new attempt. No new locking system is added
  here; the execution dispatcher must satisfy this serial contract.
- Keep read arguments/mode/version fixed for a retried ID. Reconstruct ID-only
  resources without eagerly requiring a live file or SQL row when all their
  subsequent calls will be replayed from history.
- Store results and errors in history before allowing dependent execution.
  Preserve the logical resource ID in result serialization; never serialize a
  native `externref`, pointer, file descriptor or Rust trait object.

Without the stable-file window, rereading cannot reproduce an earlier result.
The negative model deliberately violates that contract and must fail. Digests
alone can detect a changed input; they cannot reconstruct the original bytes.

## Implementation mapping and remaining integration

1. **Make effect identity explicit at the interpreter boundary.** Use the existing
   `Invocation` context to carry a typed effect ID for filesystem I/O and resource
   steps, including reader/listing `next` and explicit close. Configuration-only
   methods such as `info` need no effect ID. The per-host-call `EffectIds` provider replaces the old fixed per-store identity. The temporary provider assigns fresh UUIDs; a caller retrying a backend call must retain its `Invocation`. This provider is not restart-stable. DefaultFilesystem ignores persistence identity. Ordinary runs
   keep using the default backend until SUB-1837 supplies stable history IDs.
2. **Expose logical reads through the filesystem traits.** Replace the reader's
   bare `Read + Seek` boundary with async, effect-aware logical byte-chunk/line
   steps and an identity accessor. Opening fixes its mode/options. Prefer a
   `next(invocation)` operation returning an optional bytes/line item, so one
   guest call is one checkpoint even if it needs multiple OS reads. Keep buffering
   and decoding in the backend's reader implementation/shared default helpers.
   The interpreter only passes context, owns the externref, marshals results and
   preserves capability checks, caller attribution, fuel and memory accounting.
   Read-ahead must never become the saved logical position. EOF, BOM handling,
   CRLF, malformed UTF-8 and guest-visible failures that consume input need
   defined before/after states; uncertain I/O or commit failures remain retryable.
3. **Carry durable identity in the externref.** Prefer an owned reader object with
   its reader ID and an optional local file/buffer, avoiding a global registry.
   Provide an ID-only reconstruction path that does not run open again. The
   backend lazily loads the row and opens/seeks on the first actual read. The
   framework sees a typed resource descriptor (kind + scope/reader ID), not the
   native file object. Implement default and durable versions through traits;
   keep SQL and recovery decisions out of the interpreter.
4. **Persist the last transition.** Extend the sealed reader record/codec and
   focused unit-of-work operations with the last effect ID, request fingerprint
   and before/after states. Keep updates atomic and immutable for a repeated ID.
   The server adapter only maps these records to SQL. Do not use shared execution
   effects, a new workspace registry or durable objects. Do not persist file
   input payloads or a second copy of read results. Preserve current record and
   run limits, and include checkpoint state in their bounds.
5. **Remove reader hard-link retention.** Reopen the configured path and verify
   the expected file identity, including when reusing a local handle. Deletion
   or replacement causes an explicit resource-unavailable/conflict failure;
   do not silently switch files. This changes the current original-inode-after-
   replacement reader contract. Update its tests and docs. In-place changes
   during a pending read are excluded by the framework contract. This removes
   reader pins only; mutation recovery evidence is a separate protocol.
6. **Separate native close from durable retirement.** Externref finalization and
   attempt teardown close local files/buffers and release local accounting after
   owned work drains. They do not delete the reader row or mark it permanently
   unusable: the run may just be pausing. Explicit logical close receives an
   effect ID and must remain replayable. Delete persistent state only after a
   durable no-further-use decision and drained native access; future execution
   cleanup owns this decision. Retain state conservatively until that integration
   exists. Do not issue asynchronous SQL from a Rust destructor or introduce a
   filesystem startup cleanup worker.
7. **Carry the same identity/cursor rule to directory iterators.** A listing's
   `next` needs an effect ID, listing ID and before/after entry index too. Keep
   its existing saved listing for now; whether the framework will own that
   snapshot is a separate table-design discussion. This model's immutable item
   sequence can describe cursor advancement, but it does not verify directory
   enumeration, sorting or snapshot publication. Add their mapping/tests before
   claiming listing replay is implemented.

SUB-1837 must supply the stable IDs, typed resource serialization/rehydration,
serial dispatch and retention decisions. Reader implementation can be tested
with an explicit fake framework before that integration. A stub is not a claim
of restart-safe production execution. The Rust backend now implements this protocol. Full guest-history serialization and replay are still outside this change.

## Regression and integration tests required

- Fake framework with independent progress and history commits. Commit several
  reads, reopen SQLite with no native handles, skip those calls and continue
  from an ID-only handle. Also crash after progress but before history commit.
- Inject crashes/cancellation before reading, after read but before progress,
  after progress commit before acknowledgement, after return before framework
  completion, and after framework completion before result delivery.
- Same ID/same request retries without double advancement; changed request,
  wrong reader/scope and overlapping new IDs fail under the dispatcher contract.
- EOF and zero-position advancement; line read-ahead across several lines, BOM,
  CRLF, decoding errors, maximum line/chunk size and partial native I/O errors.
  Preserve fuel settlement and memory bounds across successful and failed calls.
- Drop/finalize a reader during a paused attempt, confirm the FD closes and the
  row remains, then resume by ID. Verify explicit close and later authorized
  retirement separately, including lost acknowledgement and duplicate cleanup.
- Delete/replace the path and refuse lazy reopen; retry cannot silently read the
  replacement. Verify guard/dispatcher behavior for a conflicting in-place write.
- Trace each fault point to a model action and each invariant to a test. Do not
  replace this with a test that manually supplies an external saved position;
  that bypasses the missing reader-ID-to-progress mechanism.

## Model and reproduction

`ResourceRecovery.tla` has one already-open reader, two one-byte reads and EOF,
one bounded crash and one bounded cancellation, a filesystem row and independent framework history. The native
handle and candidate result are volatile. A durable release is abstracted only
once this bounded program has no further reader use. There are no hard links.
Successful SQL commits survive crashes; encryption, file identity implementation,
line decoding, concurrency enforcement and real storage barriers are not proved.

| Check | Obligation |
| --- | --- |
| `ProgressBeforeHistory` | Framework completion cannot get ahead of filesystem progress. |
| `OneUncommittedRead` | A later read cannot overwrite an unfinished transition. |
| `LogicalProgress` | Persist consumed positions, not buffer read-ahead. |
| `ReadPositionCorrect` | The same effect starts before; the next effect starts after. |
| `ReadResultStable`, `HistoryCorrect` | Reread yields the same result under the stable-file assumption. |
| `ReaderRetained` | Keep the row until durable release, regardless of finalization. |
| `SameFile` | Refuse a replacement instead of silently switching resources. |

Nine negative variants break those boundaries, including no checkpoint, two
separate commits in the wrong order, ignoring the effect ID, early advancement,
changed pending input, trusting an advanced native cursor after cancellation,
and row deletion by a finalizer. Witnesses require the
progress/history crash gap, skipping committed reads before a fresh read,
finalizing pending work before retry, cancellation before/after progress with a
live handle, changed-file refusal, and eligible cleanup.
Liveness assumes eventual retry/storage success, finite crashes/cancellations, stable file
contents/identity and fair dispatch. Changed-file refusal is a safety case, not
promised successful continuation. This is bounded protocol verification, not a
proof of the current Rust code or the full SUB-1837 execution framework.

```sh
python3 verification/check.py --model filesystem/resources
```

## Audit of the other filesystem resources

The same review extends the implementation plan beyond readers:

| Component | Existing mechanism | Required addition |
| --- | --- | --- |
| Listing open | Bound request and immutable snapshot in SQL. | Stable open result containing listing ID; retry after snapshot commit must not enumerate again. |
| Listing next | In-memory iterator and caller-supplied offset; no per-call identity. | Effect ID, persisted last before/after entry indices, and lazy restoration from listing ID. |
| Writer open | Writer row and ID already exist. | Expose its ID through the trait/externref codec; restore from it without a new logical open or truncating staging. |
| Writer chunks | Stable chunk IDs/fingerprints and ordered receipts; fixed offsets from acknowledged lengths. | Supply the framework effect ID as chunk ID; preserve receipts/staging when history is behind or skips calls. No second cursor table or before/after pair is needed. |
| Writer close | Reserved close ID, filesystem outcome and separate closed-state transition. | Framework close completion is another commit; restore SQL closed/closing state and reconcile by the same close ID even with a fresh local object. |
| Finalization | Local resources are dropped; writer publication is explicit. | Preserve this rule for all ID-only handles: no SQL/staging deletion and no writer publication from finalizers. |
| Primitive mutations | Own immutable operation receipts suppress repeated mutations. | Framework must keep supplying the same IDs. No handle cursor is involved; the existing sequence model covers the outer-completion gap. |
| Whole-file reads/stat | Return values without a persistent iteration cursor. | Framework records observations; these do not need reader-position tables. Pass effect context consistently at the I/O boundary. |

### Add to the implementation sequence

- Extend `DirectoryReader` with effect-aware async `next` and resource identity;
  stop restoring its position from a transient counter. Freeze the guest-visible
  entry order before applying any persisted offset. Prefer storing that final
  ordering with a format version. The implementation now stores component-wise traversal order before applying any offset; the request codec versions this ordering.
- Include listing and writer descriptors in the same resource codec/restore
  contract as readers. Keep their scope, type and immutable options checked.
  Default backends can ignore persistence IDs; stubs cannot claim crash recovery.
- Keep chunk receipts as the write-progress authority. Do not introduce a local
  chunk counter that restarts at zero, generate new IDs while replaying, or
  recreate/truncate the staged file when rehydrating the writer.
- Treat SQL state/outcomes as authoritative for writer close, including definitive
  preflight refusal. A cached `is_closed` flag cannot replace outcome lookup or
  prevent retry reconciliation. Test the gap between outcome commit and marking
  the writer closed, as well as the gap before framework completion. Preserve
  guest error classification and local memory release for terminal close errors.
- Persisted resource cleanup belongs to a durable release/retirement decision.
  Keep rows, chunk receipts and staging conservatively until that integration
  exists. Temporary handle release during pause/cancellation is not retirement.
  No recovery scanner, shared-effects table or durable-object dependency is added.

### Additional regressions before implementation is complete

1. Listing open: crash after snapshot commit but before open-history completion;
   retry the same open, then mutate the directory and still see the saved snapshot.
2. Listing next: record two entries, reopen by listing ID only, skip their history,
   and continue at the third; also retry a next whose index committed without
   framework completion. Use paths `a`, `a-`, `a/z` to distinguish full-order seek
   from sorting the remaining suffix. Include EOF and empty listings.
3. Listing cancellation: discard a result before/after index commit while keeping
   the cached iterator advanced; retry the same ID. Finalize during pause and
   resume without losing the row or snapshot.
4. Writer open/chunks: reopen by ID without touching staging; skip completed chunk
   effects, retry the chunk with a receipt but no framework completion, then write
   another chunk at the acknowledged offset. Include empty chunks/empty writers,
   mismatched fingerprints, missing/corrupt staging and partial unacknowledged tails.
5. Writer close: fault after reservation, prepared intent, publication, sync,
   filesystem outcome, writer closed-state and framework completion. Repeat with
   preflight refusal and a replaced target after completion. Both fresh and
   cached handles must return the authoritative outcome without republication.
6. Lifetime: cancellation/finalization before/after chunk receipt and during close
   must drain work, release only local access, retain SQL/staging and never publish.
   Test authorized terminal cleanup separately, including idempotent deletion.

The new [listing model](../listings/README.md) and expanded
[writer/chunk model](../writers/README.md) explicitly separate their filesystem
commits from framework history and include negative variants for these gaps.
The filesystem-side additions are implemented. They do not imply that production execution history and restart orchestration are already wired up.

## Implemented behavior and test mapping

- `FileReader::next` receives an `Invocation` and current tenant limits. The
  default backend/shared read helper performs logical byte/line reads; the
  durable backend commits `Progress { effect, close, before, after }` before
  returning. Reader options are immutable, so effect ID plus operation kind is
  the per-step request identity. No read bytes enter the checkpoint.
- `ResourceDescriptor` is serializable (scope, ID, kind/options). The backend
  `restore_*` methods construct ID-only handles without opening a file or
  consulting SQLite. Reader calls reopen and validate the pathname every time;
  there is deliberately no native-cursor cache. Thus cancellation cannot leave
  a cached cursor ahead of SQL. Explicit close saves closed state; drop does not.
- Failed logical reads do not commit progress. Partial I/O, oversized lines and
  failed allocation leave the before-state authoritative. A guest-catchable
  size error can be recorded by the framework; a later distinct read starts
  from the same position. I/O/storage failures remain retryable. BOM is handled
  only after a complete successful line, and the offset includes delimiters.
- Listings persist the final traversal order and per-next/close transition in
  separate sealed columns. Loading a snapshot for a restored iterator admits
  bounded working memory before decoding. Directory open admits the maximum
  bounded working set before enumeration or SQL decoding (currently 32 MiB plus
  8 KiB); owned operations retain that reservation and their gate through cancellation
  until native enumeration and SQL work have finished. Chunks retain their existing receipt
  protocol. Restored writers consult SQL for close success/refusal and never
  initialize a new staged file.
- `filesystems/tests/resources.rs` uses independent history results, serialized
  descriptors and explicitly closed/reopened SQLite connections. It covers
  skipped calls, unrecorded results, duplicate IDs, changed options/scope,
  BOM/CRLF/lossy UTF-8, EOF, logical close, finalization and writer pathname reuse.
  A blocked checkpoint transaction also exercises same-process cancellation
  after reading and before progress commit.
- `filesystems/tests/process.rs` kills a child after reader/listing open, after
  physical next but before progress, and after progress before return. Writer
  cases include staged output, chunk receipts, intent, publication, close
  receipts and closed-state. The existing model actions map to these checkpoints.
- Default reader tests cover bounds, memory release, partial I/O and settled fuel.
  End-to-end interpreter tests exercise distinct IDs across writer chunks,
  line/chunk iteration and listing steps.

The dispatcher must still enforce the framework contract above, particularly no
new dependent effect before history completion and no conflicting mutation while
a read is pending. There is no automatic resource retirement; SQL and writer
staging are retained until the future execution lifecycle authorizes deletion.
