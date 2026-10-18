# Idempotent filesystem adapter

`submilli-durable::DurableFilesystem` implements the interpreter's
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
completed calls return their receipts and a pending call reconciles its checkpoint.
Even if an outer execution effect remains pending after filesystem success, its
retry cannot repeat that filesystem operation. Ordered execution history,
resumption and the bridge assigning these IDs remain future work.

## Tables

Migration 10 creates only filesystem-owned tables. None references a run row.

| Table | Purpose |
| --- | --- |
| `durable_filesystem_operations` | Request and binding digests, operation-specific checkpoint, and completed/refused outcome, keyed by scope and operation ID. |
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
   a replacement candidate when needed. Commit the operation-specific checkpoint.
3. Apply against expected inode identities and fixed append offsets; sync files
   and containing directories.
4. Commit the filesystem outcome before returning success.
5. On retry, validate the same request and binding. Return a completed receipt;
   otherwise reconcile the prepared operation using matching replay input.

`write_bytes`, `move_path`, `remove_path` and `create_directory` attach or load
an operation row through `UnitOfWork`, perform their own IO steps, and commit the
outcome. Writes receive bytes on every invocation. They store fingerprints, not
inputs. Writer close streams its validated staging file through the same write
path; there is no replay-input provider or generic mutation dispatcher. An
uncertain SQL commit is resolved by reading the same identity.
After public mutation starts, operational failures remain pending; only a
preflight refusal can become a definitive rejected outcome. No SQL transaction
spans filesystem mutation. Cancellation keeps the gate held until a blocking
worker drains. Recovery candidates and inode pins are filesystem evidence,
not a second input-payload store.

## Implementation map

The production crate has six implementation files and `lib.rs`:

`DurableFilesystem` holds shared storage and mount state. Each call constructs a
`FilesystemOperation` with its work budget; direct operations such as `write_bytes`
live on that value. Native IO helpers receive the budget explicitly, and owned
workers retain it until they finish. Reader and writer handles keep only the
shared filesystem and take a fresh budget from each invocation.

- `filesystem.rs`: interpreter port and direct write/append, move, remove and
  mkdir operations. `attach` checks identity and admission within the caller's
  unit; `save_checkpoint` and `save_outcome` make the two subsequent commits.
- `readers.rs`: reader/listing handles, immutable listing outputs, and the last
  effect's before/after cursor. Native handles are reopened only when dispatched.
- `writers.rs`: staged output, fixed-offset chunks, and close reservation/publication.
- `io.rs`: capability-relative paths, inode validation, barriers and shared quotas.
- `unit_of_work.rs`: the persistence port, record types and transition validation.
- `codec.rs`: bounded encodings, encryption and associated-data binding.

`Target` saves the original entry identity, type and append baseline;
`WriteCheckpoint` adds the candidate inode and `MoveCheckpoint` adds the
expected destination. These are persisted facts used by the operation's code,
not instructions interpreted by a recovery engine. No SQL transaction spans IO.
Operation format version 2 uses distinct write/append/move/remove/mkdir kinds
and two digests for the request. The earlier, unmerged PR format is not a
compatibility target. Existing main-branch application tables are unchanged.

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
