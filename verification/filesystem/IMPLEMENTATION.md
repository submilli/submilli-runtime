# Filesystem recovery adapter

`submilli-durable::filesystems::DurableFilesystem` implements the interpreter's
filesystem interface. The server supplies its SQLite `UnitOfWork`. Neither the
filesystem nor its persistence contract depends on durable-object storage.
Execution history, program replay and startup supervision remain separate work.

## Inputs, outputs and recovery

The journal stores operation identity/version, paths, input length/hash,
filesystem recovery metadata, and the final outcome. It does **not** store file
write input bytes. `ReplayInput` holds bytes only for the current invocation.
After restart, execution replay reconstructs them from pinned initial run inputs,
recorded prior observations/outcomes, and deterministic computation. Recorded
reads, clocks, randomness and other nondeterminism are prerequisites for that
execution layer; the filesystem does not repeat those observations itself.

`execute_with_input` validates supplied input against the request fingerprint.
Admission compares the immutable request against any prior record. A mismatch
fails before public mutation. `recover` can discover pending work but returns
`ReplayRequired` for a pending write/append that needs caller bytes; the caller
replays execution and retries the same effect with `execute_with_input`. It must
not invent a new effect ID or redo an external download to obtain those bytes.
A completed effect returns its saved outcome without requesting input or touching
the current pathname. Retained workspace contents are never rebuilt by replaying
completed mutations.

Filesystem staging remains necessary: a replacement candidate is the new file
being constructed; original inode pins protect mutation/recovery evidence.
These are mutable filesystem artifacts, not immutable input captures or a
second general payload store.

## Ordering and storage

Migration 10 contains `durable_filesystem_store` (database identity), retained
workspaces, shared `durable_effects`, plans, readers, listings, writers and chunk
fingerprints. No object-store or filesystem-object producer tables are created.
Sensitive metadata and recorded outputs are sealed before reaching SQLite.
Directory enumeration is an output, stored inline in SQLite with a 4 MiB bound;
ordinary recovery records have a 64 KiB bound. Large-output placement is a later
history-storage enhancement behind the same byte interface.

| Boundary | Implementation/model mapping |
| --- | --- |
| Reserve workspace | Commit ownership/layout before creating a retained root |
| Admit effect | Commit immutable metadata and fingerprint; no input bytes |
| Prepare | Sync baseline, pin original, build candidate; private artifacts only |
| Prepare commit | Persist recovery intent before public mutation |
| Apply | Reconcile expected inode identities and fixed append offsets |
| Persistence barriers | Sync files and containing directories |
| Complete | Commit outcome before returning it or admitting the next mutation |
| Replay | Validate identity/fingerprint and return committed outcome |
| Cleanup | Require terminal state, drain workers/readers, remove artifacts, tombstone |

SQLite commit and acknowledgement are distinct. An uncertain commit is resolved
by rereading the same identity. After public mutation starts, an operational
failure keeps the effect pending; it must not become a definitive guest refusal.
Preflight refusals may be recorded and replayed. Cancellation cannot release the
shared filesystem gate while a blocking worker still owns it.

## Writers and resources

A writer has one private output file on the target volume. Each write validates
the stable chunk ID and fingerprint. New chunks overwrite at the sum of already
acknowledged chunk lengths, truncate any unacknowledged tail, and sync the staged
output before committing the chunk receipt. Duplicate acknowledged chunks return
without writing; changed arguments fail. SQLite stores chunk descriptors only.

Close verifies every acknowledged chunk against the staged output, computes the
whole-file fingerprint with bounded buffering, fences the writer, and uses the
ordinary replacement protocol. A lost close receipt is reconciled; a completed
close never republishes after pathname reuse. Staging counts toward filesystem
quota, including the candidate needed for publication. Missing/corrupt acknowledged
output blocks close rather than publishing different bytes. Dropping a writer
does not publish its output. Terminal cleanup removes unfinished staging.

Readers pin the original inode durably. Dropping a handle releases native access,
not its durable pin; cleanup waits for open readers. Execution history must record
read bytes and logical cursor/decoder state before exposing an observation.
Directory iterators replay their recorded listing and offset, never enumerate a
completed listing again.

Compound plans persist ordered mutation descriptors, derived effect IDs and
aggregate outcomes. Their inputs are supplied separately on replay. Completed
prefixes are skipped. Source enumerations and prepared Git commit values must
come from recorded execution outputs; fresh traversal/commit creation is not
recovery. Ordinary Git, code tools and HTTP downloads remain available through
the default VFS, outside this adapter's durability guarantee.

## Verification and limits

The append and namespace models now distinguish volatile replay inputs from
retained filesystem evidence: crash erases input availability, and recovery waits
for matching input. The writer model checks staged-output/receipt ordering,
fixed-offset retry, mismatched duplicate input, and completed-close replay.
Shared effects check request/fingerprint immutability. The unit-of-work model
checks atomic metadata, staged reads, rollback and lost acknowledgements.
Directory, retained-reader and sequence models cover their separate protocols.

These are bounded protocol checks, not a proof of Rust, SHA-256, SQLite or storage
hardware, and not an end-to-end execution/filesystem composition proof. Digest
comparison abstracts collision resistance. Progress assumes matching replay
inputs eventually arrive, storage eventually works, and only finitely many
crashes occur. Wrong replay remains blocked, not silently repaired.

One logical run exclusively owns the backing filesystem, including aliases and
native tools, until pending mutations reconcile. One shared `FilesystemContext`
coordinates same-process access and quotas. There are no cross-process leases,
locks, fencing, HA takeover or network-storage guarantees here. Successful file
and directory synchronization must survive the storage failure being modeled.
Process-kill tests exercise application crashes; they do not establish power-loss
behavior of a device. Limits include 10,000 traversed entries, depth 64 and 1,000
steps per compound plan. Ephemeral workspaces remain under the retained prefix
`submilli-vfs-run-`, outside the scratch boot sweep.
