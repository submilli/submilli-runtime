# Idempotent filesystem adapter

`submilli-durable::filesystems::DurableFilesystem` implements the interpreter's
filesystem interface. The server supplies its SQLite `UnitOfWork`. Filesystem
operations have their own journal, independent of execution history and durable
objects. No pending-work dispatcher or filesystem startup recovery is installed.
Ordinary server and CLI runs still use the default filesystem.

## Ownership and identity

The caller passes its configured `Vfs`, with all blueprint mounts, to `open`.
It supplies a stable UUID scope and stable operation IDs, retains the same roots
and journal across retries, and exclusively owns the writable filesystem.
Opening establishes access and private recovery directories; it neither creates
a public run directory nor executes unfinished operations. A fingerprint of the
root and mount identities, cwd, access modes and quotas binds each journal record
to that configuration. Retry against a changed binding fails. Replacing the
entire journal cannot be detected by this component and violates its contract.

There is no workspace registry, database identity table or stored program plan.
The caller's code determines operation order. It repeats calls with stable IDs;
completed calls return their receipts and a pending call reconciles its intent.
Even if an outer execution effect remains pending after filesystem success, its
retry cannot repeat that filesystem operation. Ordered execution history,
resumption and the bridge assigning these IDs remain future work.

## Tables

Migration 10 creates only filesystem-owned tables. None references a run row.

| Table | Purpose |
| --- | --- |
| `durable_filesystem_operations` | Immutable request and binding, prepared intent, and completed/refused outcome, keyed by scope and operation ID. |
| `durable_filesystem_readers` | Bound path/file identity and options, open error, last effect ID and before/after logical read/close state. |
| `durable_filesystem_listings` | Open request, immutable ordered snapshot, last effect ID and before/after next/close index. |
| `durable_filesystem_writers` | Resource ID, bound open request and close state. |
| `durable_filesystem_chunks` | Stable writer chunk IDs, sequence, lengths and fingerprints; offsets are derived from preceding lengths. |

Sensitive metadata and outputs are sealed. Ordinary records have a 64 KiB bound;
directory enumeration is stored inline in SQLite with a 4 MiB bound. File input
bytes are never stored in SQL. Large-output placement is a later history-storage
concern, behind the same byte interface. There is no durable-object dependency.

## Mutation flow

1. Commit the immutable request, input fingerprint and filesystem binding.
2. Prepare private evidence: sync the baseline, pin the original inode, and build
   a replacement candidate when needed. Commit the prepared intent.
3. Apply against expected inode identities and fixed append offsets; sync files
   and containing directories.
4. Commit the filesystem outcome before returning success.
5. On retry, validate the same request and binding. Return a completed receipt;
   otherwise reconcile the prepared operation using matching replay input.

`execute_with_input` receives bytes reconstructed by execution replay.
`execute` without those bytes returns `ReplayRequired` for an unfinished write
or append. An uncertain SQL commit is resolved by reading the same identity.
After public mutation starts, operational failures remain pending; only a
preflight refusal can become a definitive rejected outcome. No SQL transaction
spans filesystem mutation. Cancellation keeps the gate held until a blocking
worker drains. Recovery candidates and inode pins are filesystem evidence,
not a second input-payload store.

## Resources and retention

A writer stages output on the target volume. New chunks overwrite at the sum of
acknowledged lengths, truncate any unacknowledged tail, and sync before recording
the chunk receipt. Duplicate chunks return without writing; changed arguments
fail. Close validates the staged bytes, fences further chunks, and uses the
replacement protocol. A completed close never republishes after path reuse.
Dropping the writer does not publish. Staging and candidates count toward quota.

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
resources. The temporary per-call ID provider is not a history/replay engine.
See the [resource protocol and test mapping](resources/README.md).

The caller owns retention and eventual cleanup. It must retain journal rows and
private evidence while retries or resources remain possible and drain work and
handles before disposal. There is no run-lifecycle check, terminal-workspace
cleanup or automatic row deletion. Low-level cleanup helpers have this explicit
caller precondition; the model's eligible cleanup actions describe that contract.

Code tools, Git and HTTP downloads retain their existing direct VFS access and
are outside this adapter's guarantees. Recursive mutations and copies require
caller orchestration of primitives; no compound plan is stored.

## Verification and limits

Append and namespace models distinguish volatile replay inputs from retained
filesystem evidence. The writer model checks staged-output ordering and fixed
offset retries. The operation-journal model checks immutable requests, intents
and outcomes; the unit-of-work model checks atomic commits, staged reads,
rollback and lost acknowledgements. Reader and listing models specify the
separate filesystem-progress/history boundary; the writer model separates chunk/close
receipts from framework completion. The directory model covers mkdir/rmdir.
The sequence model checks caller-code replay and the gap between a filesystem
receipt and an outer completion, without a persisted plan.

These are bounded protocol checks, not proofs of Rust, SHA-256, SQLite, storage
hardware or end-to-end execution. Progress assumes matching replay inputs,
eventual working storage, finite crashes and a caller that eventually retries.
One logical owner exclusively controls the roots, including aliases/native tools.
A shared `FilesystemContext` coordinates same-process admission and quotas.
Cross-process leases, fencing, HA and network-storage guarantees are absent.
Limits include 10,000 operation records per scope, 10,000 traversed entries and
depth 64. Process-kill tests establish application-crash behavior, not physical
power-loss guarantees.
