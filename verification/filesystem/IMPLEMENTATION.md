# Idempotent filesystem adapter

`submilli-durable::DurableFilesystem` implements the interpreter's
filesystem interface. The server supplies its SQLite `UnitOfWork`. Filesystem
operations that need saved progress have their own journal, independent of
execution history and durable objects. Plain byte writes have no local journal. No pending-work dispatcher or filesystem startup recovery is installed.
Ordinary server and CLI runs still use the default filesystem.

## Ownership and identity

The caller passes its configured `Vfs`, with all blueprint mounts, to `open`.
It supplies a stable UUID scope and stable operation IDs, retains the same roots
and journal across retries, and exclusively owns the writable filesystem. No other call changes a pending
operation's paths, and a cancelled native attempt drains before retry. Completed
operations may be followed by later writes from the same owner. The framework
skips completed byte writes; local receipts protect other completed operations.
Opening establishes access and private recovery directories; it neither creates
a public run directory nor executes unfinished operations. A fingerprint of the
root and mount identities, cwd, access modes and quotas binds each journal record
to that configuration. Retry against a changed filesystem configuration fingerprint fails. Replacing the
entire journal cannot be detected by this component and violates its contract.

There is no workspace registry, database identity table or stored program plan.
The caller's code determines operation order. It repeats calls with stable IDs;
completed journaled calls return their receipts and pending calls repeat their
saved state. A pending byte write can repeat even after native success: it writes
the same bytes again until the framework durably records its completion. The
framework must skip completed byte writes and admit no later effect beforehand. Ordered execution history,
resumption and the bridge assigning these IDs remain future work.

## Tables

Migration 10 creates only filesystem-owned tables. None references a run row.

| Table | Purpose |
| --- | --- |
| `durable_filesystem_operations` | Request/configuration digests and one optional encoded state, keyed by scope and operation ID. `status` is derived solely for pending admission. |
| `durable_filesystem_readers` | Bound path/file identity and options, open error, last effect ID and before/after logical read/close state. |
| `durable_filesystem_listings` | Open request, immutable ordered snapshot, last effect ID and before/after next/close index. |
| `durable_filesystem_writers` | Resource ID, bound open request and close state. |
| `durable_filesystem_chunks` | Stable writer chunk IDs, sequence, lengths and fingerprints; offsets are derived from preceding lengths. |

Sensitive metadata and outputs are sealed. Ordinary records have a 64 KiB bound;
directory enumeration is stored inline in SQLite with a 4 MiB bound. File input
bytes are never stored in SQL. Large-output placement is a later history-storage
concern, behind the same byte interface. There is no durable-object dependency.

## Byte-write flow

`write_bytes` uses no SQL and has no state enum or codecs. Each invocation:

1. Resolves the destination and removes any stale `<effect-id>.replacement`
   name in the run's private directory, before validating public hard links.
   This needs no scan of unrelated private entries and leaves no per-write directory.
2. Validates the destination, checks its net public size change against quota,
   and creates a fresh temporary file. It never truncates an old inode: a power
   crash can retain a private name that aliases the already published destination.
3. Writes all replay-supplied bytes and syncs the temporary file.
4. Renames it over the destination and immediately applies the public size delta.
5. Syncs the destination and private directories, then returns.

Private bytes cost fuel, not disk quota. Retrying measures the current destination
size, so a previously published write adds no second quota delta. An error after
publication remains uncertain; it cannot become a recorded preflight rejection.
The worker retains the mutation gate through cancellation until it exits.

The caller must preserve the run/effect identity, path, bytes, mount configuration
and exclusive ownership. The future framework records completion separately,
skips completed effects on replay and prevents later effects until that record is
durable. Calling an old byte write directly will overwrite a later file. This
adapter does not implement that framework. [WriteRecovery](writes/README.md)
checks the composition and has negative controls for violating those obligations.

## Journaled mutation flow

1. Commit the immutable request, input fingerprint and filesystem configuration fingerprint.
2. Save `Prepared` after validation and required preparation. Append saves its
   original offset; the other mutations need only a marker with no fields.
   Paths come from the call arguments or writer row. Writer close locates its
   synchronized candidate using the close effect ID. Do not preserve/hash old contents.
3. Apply the mutation and synchronize its file/directory changes. Move publishes
   and syncs the destination before removing the source. Append retries write at
   the saved offset, including offset zero for a newly created file.
4. Replace the saved state with `Completed(result)` before returning success.
5. On retry, validate the same request/configuration and return a completed
   receipt; otherwise repeat the saved state using matching replay input.

`append_bytes`, `move_path`, `remove_path` and `create_directory`
create or get a `JournalEntry` through `UnitOfWork`, perform their own IO steps,
and commit the outcome. Appends receive the path and bytes on every invocation.
Their `Initial` branch uses `Files::size` to check and synchronize the existing
file, then saves only `Prepared { offset }`. Each pending attempt resolves the supplied path again
and writes at that saved offset. The request digest rejects changed paths or
inputs; no resolved file location is stored in the append state.
Writer close has its own publication flow and journal admission using its
reserved close effect. It streams validated staged output; it does not call
`write_bytes` or `append_bytes`. Each journaled operation owns an explicit state loop. Only replacement-file IO, contained path access, and persistence primitives are shared. No mode flag or optional writer selects behavior
inside these flows. An uncertain SQL commit is resolved by reading the same identity.
The interpreter's existing filesystem port dispatches its write/append flag to
the corresponding entry point. The journal encodings and preparation/commit order
match the replacement, append, and writer TLA+ models.
After public mutation starts, operational failures remain pending; only a
preflight refusal can become a definitive rejected outcome. No SQL transaction
spans filesystem mutation. Cancellation keeps the gate held until a blocking
worker drains. Prepared replacement files and writer staging remain private filesystem output,
not a second SQL input-payload store. Mutation code never pins the old destination.
Writer-close publication retains its new candidate under a private name so a
lost acknowledgement can be retried at full quota without another allocation.

## Implementation map

Each journaled operation has a file under `src/filesystem/`, containing its state type,
execution loop and state codecs. The simpler `write_bytes.rs` is straight-line
IO; `append_bytes.rs` introduces the journaled state machine.
`move_path.rs`, `remove_path.rs`, and `create_directory.rs` contain their native
steps inline. `open_writer.rs`, `write_chunk.rs`, and `close_writer.rs` cover the
writer lifecycle. Reader and listing open/next/close each have a separate file.

Mutation variants are `Initial`, `Prepared` with operation-specific fields, and
`Completed(Result<Outcome, String>)`. Each operation serializes its enum directly
into a single optional `state` payload. No saved payload means `Initial`; it is
never inferred from invalid bytes. Prepared data is replaced by the completed
result, so there are no separate recovery/outcome columns or rejection flag.
The journal validates forward transitions and freezes completed bytes. SQL
`status` is derived from this state for the partial unique index and checked on
read; it does not select an operation's execution state. Each transition commits
before the next iteration. A terminal refusal can only leave `Initial`; errors during publication
leave `Prepared` retryable. Native worker phases remain separate from async SQL.

`FilesystemOperation` in `filesystem/operation.rs` holds the per-call work budget
and shared journal admission/commit methods. `DurableFilesystem` in `filesystem.rs`
owns mounts, gate and quotas and implements the interpreter port.
Writer-close `replacement.rs` supplies explicit byte/file variants; cursors and input adaptation
stay inside those helpers. Public mutations apply their actual size delta before
any further fallible work; private preparation does not change disk quota. `readers.rs` and `writers.rs` adapt handles and share resource access.
`io.rs` owns contained paths and disk accounting; `unit_of_work.rs` owns records
and monotonic transition checks; `codec.rs` owns bounded encryption/serialization.

`AppendState::Prepared { offset }` saves only the original length. Move, remove,
mkdir and writer close use `Prepared {}`: their paths are supplied again, and
remove inspects an existing target to choose file or directory deletion. Writer
close reads its deterministic candidate name and compares current file identities
to recognize an already published candidate. None of these states stores an
`Entry` or inode identity. `Entry` is only a transient contained path resolution.
No SQL transaction spans IO. Journal format version 4 rejects earlier state
shapes; the prior unmerged adapter format is not a compatibility target.
Migration 10 changes within this unmerged PR; main-branch application data and
other tables remain unchanged.

## Resources and retention

A writer stages output on the target volume. New chunks overwrite at the sum of
acknowledged lengths, truncate any unacknowledged tail, and sync before recording
the chunk receipt. Duplicate chunks return without writing; changed arguments
fail. Close validates the staged bytes, fences further chunks, and uses the
replacement protocol. A completed close never republishes after path reuse.
Dropping the writer does not publish. Staging, candidates and retained recovery
links do not count toward disk quota. Close checks only the destination's net
size change. Public mutations update the shared counter immediately after each
successful native change; partial append prefixes count, completed replay does
not reapply a delta. Fresh open measures public files once; same-process reopen
shares live counters. The [quota model](quotas/README.md) checks this boundary.

Readers and listings expose effect-aware logical steps. Each step commits its
last effect ID and before/after position before returning; execution history
records its output separately. Repeating an unfinished effect uses its before
state, while a new effect uses its after state. No duplicate read bytes are
stored. The framework must serialize calls and keep pending input unchanged.
Readers reopen the configured path and validate identity instead of pinning an
inode. Native read-ahead never advances SQL progress. A failed logical read leaves
its position unchanged. Listings persist their final component-wise traversal
order before restoring any index.

`ResourceDescriptor` contains scope, resource ID and kind/options. Restoration
constructs ID-only handles without I/O; the first dispatched call loads durable
state. Writer restoration preserves staging and existing chunk receipts. Explicit
reader/listing close checkpoints closed state; finalization only drops local
resources. `EffectId::stub()` supplies temporary IDs without prescribing the future history/replay implementation.
See the [resource protocol and test mapping](resources/README.md).

The caller owns retention and eventual cleanup. It must retain journal rows and
private evidence while retries or resources remain possible and drain work and
handles before disposal. There is no run-lifecycle check, terminal-workspace
cleanup or automatic row deletion. The model's eligible cleanup actions describe this future caller contract;
this crate exposes no cleanup or startup-recovery API.

Code tools, Git and HTTP downloads retain their existing direct VFS access and
are outside this adapter's guarantees. Recursive mutations and copies require
caller orchestration of primitives; no compound plan is stored.

## State-machine correspondence

The TLA+ models now dispatch through named loop actions, using `CASE` like Rust's
`match`. Repeating `Next` supplies the loop. Each native write, sync and SQL commit
remains a distinct transition: grouping an arm does not make its work atomic.
Crashes reset volatile execution state, never the committed journal state.

| Rust flow | Model loop | Persistence boundary |
| --- | --- | --- |
| Mutation journal codecs | `FilesystemOperations.OperationLoop` | One encoded state: missing → `Initial`, then `Prepared` → `Completed`; derived pending index |
| Byte write | `WriteRecovery.Step` | Fresh file sync and directory sync precede outer completion; no local SQL state |
| Append | `AppendRecovery.AppendLoop` | Fixed-range writes and sync precede receipt |
| Writer-close replacement/move/remove | `NamespaceRecovery.NamespaceLoop` | Replacement sync; move destination sync before source removal; parent sync before receipt |
| mkdir/rmdir | `DirectoryRecovery.DirectoryLoop` | Name and parent barriers precede receipt |
| Reader next | `ResourceRecovery.ReaderLoop` | Last effect and before/after positions precede framework history |
| Listing open/next | `ListingRecovery.OpenDirectoryLoop`, `NextListingLoop` | Frozen snapshot, then last-effect progress |
| Writer open/chunk/close | `WriterRecovery.OpenWriterLoop`, `WriteChunkLoop`, `CloseWriterLoop` | Row, chunk receipt, close intent/receipt and closed flag |
| Journaled caller replay | `SequenceRecovery.CallLoop` | Completed prefix is skipped using filesystem receipts |

The native mutation models abstract admission and preflight validation into
`Intent`; the journal model separately checks request-to-prepared ordering.
They are protocol abstractions, not line-by-line translations or Rust proofs.
Reader/listing `Completed` results are volatile until the separate framework
commit. Their saved before/after positions reconstruct `ReadAt` on retry. Explicit
close behavior remains covered by Rust cursor-transition and integration tests.

Negative controls remain enabled outside normal dispatch where needed. A broken
implementation can bypass a state arm, save an outcome too early, or reapply a
completed operation; making such traces unreachable would weaken verification.
Safety, liveness, negative and witness cases check the simplified protocols.
Backup-retention invariants are replaced by surviving move-data and journal
retention requirements. Append additionally checks new-file creation and parent
synchronization. Reader/listing progress and history-gap guarantees are unchanged.

## Verification and limits

Append and namespace models distinguish volatile replay inputs from retained
filesystem evidence. The writer model checks staged-output ordering and fixed
offset retries. The operation-journal model checks immutable requests, intents
and outcomes; the unit-of-work model checks atomic commits, staged reads,
rollback and lost acknowledgements. Reader and listing models specify the
separate filesystem-progress/history boundary; the writer model separates chunk/close
receipts from framework completion. The directory model covers mkdir/rmdir.
The byte-write model checks atomic replacement and the outer completion gap
without local receipts. The sequence model checks journaled caller-code replay and the gap between a filesystem
receipt and an outer completion, without a persisted plan.

These are bounded protocol checks, not proofs of Rust, SHA-256, SQLite, storage
hardware or end-to-end execution. Progress assumes matching replay inputs,
eventual working storage, finite crashes and a caller that eventually retries.
One logical owner exclusively controls the roots, including aliases/native tools.
`DurableFilesystem` owns same-process admission and quota sharing. Clones and
`reopen` share the gate and quota registry; fresh `open` requires exclusive root
ownership with no outstanding handles or workers.
Cross-process leases, fencing, HA and network-storage guarantees are absent.
Limits include 10,000 operation records per scope, 10,000 traversed entries and
depth 64. Process-kill tests establish application-crash behavior, not physical
power-loss guarantees.

Writable backing roots must be physically disjoint. Setup checks their held
directory identities and ancestry before initializing independent quota counters;
identical roots and ancestor/descendant roots are rejected. Guest mount names may
still be separate while their backing roots overlap, so virtual-path validation
alone is insufficient. This is a setup check, not a per-operation tree walk.
