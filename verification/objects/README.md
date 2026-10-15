# Immutable object publication and recovery

This is SUB-1815's independently deliverable **immutable-object slice**, supplying
the proposed storage protocol for SUB-1846. It contains executable TLA+ and
bounded TLC checks, not a Rust object store. Mutable filesystem operations
(write/writer-close, append, move, remove and mkdir), resource reconstruction and
their idempotent-effect protocol remain unfinished parts of SUB-1815.

Downloads always use the object store. Other payloads above the configured size
threshold use it too; small sealed payloads remain inline in SQLite. Thresholds,
streaming codecs and encryption are implementation concerns, not modeled here.
One object ID identifies one immutable candidate; this model does not introduce
content deduplication or permit an ID to be reused after collection.

## Proposed protocol

The publication order is:

    commit pending identity -> stream sealed staging file -> fsync file
      -> publish final name -> fsync directory -> commit reference + retention
                                                        -> return recorded result

The object directory is persistent, private to the server, and separate from
guest-writable workspaces. Staging and final names live in the same pre-created,
durable directory on one filesystem. An opaque publication ID determines both
names; it must not contain a node-local path in the durable history. The reference
also carries format/key identifiers, length and integrity identity. The model
represents that descriptor by `Body(o)`, a fixed sequence of opaque chunks.

1. **Reserve before writing.** Commit the stable publication ID and immutable
   request identity with a pending-publication pin. Discover that record after a
   lost acknowledgement or restart. A pending record protects partial and fully
   published files equally; absence of a committed reference is not GC eligibility.
2. **Stage and publish.** Stream sealed bytes to the staging file. Complete and
   validate the envelope, sync file contents, close writable handles, publish the
   final name without replacing an existing object, then sync the parent directory.
   On retry, inspect and validate existing staging/final contents under the same
   identity. Never infer completeness or a resumable prefix from length alone:
   reset and rewrite invalid staging bytes while retaining the pending pin.
   An existing final file is never blindly overwritten.
3. **Commit the reference.** In one SQLite transaction, conditionally verify that
   the publication remains pending and is not being collected; insert its history
   event/reference and retention roots; consume the pending pin. A failed or
   uncertain commit does not authorize file deletion. Reread the durable state:
   bound means reuse the original reference, pending means retry the same logical
   commit, abandoned/retired/deleting means refuse the late write.
4. **Read under a pin.** Register a reader pin under the same per-object exclusion
   gate used to claim collection, before opening the file. Hold it through all
   reads, copying, cancellation and draining. Retirement of a SQL reference may
   overlap a reader, but physical collection waits for every reader pin.
5. **Retire and collect.** Recoverable-run needs and response-retention needs are
   distinct. Only their actual release permits reference retirement. A pending
   publication can be abandoned only after excluding/draining its writer. Claim
   deletion durably before unlinking; that claim prevents new references/readers.
   Unlink, sync the parent directory, then record collection complete. Restart
   resumes any deletion claim, including an unlink whose name removal was not
   durable. Retain identity/tombstone evidence; no object-ID reuse is modeled.

SQLite transactions and filesystem operations are separate atomic actions.
`CommitReference` leaves its local transaction uncertain; `ForgetTransaction`
before commit is rollback, and after commit is a lost/reconciled acknowledgement.
A process can crash between any actions. Reservation and deletion commits also
stand alone: their durable state is rediscovered without relying on a reply.

The per-object exclusion gate is a required implementation mechanism. It covers
filesystem mutation versus abandonment/collection, and reader registration versus
the deletion claim. SQL state predicates are checked when the write commits,
not merely when it is prepared. This model assumes the gate's serialization and
worker draining; it does not verify a Rust lock or async cancellation mechanism.

## State and persistence assumptions

| State | Meaning |
| --- | --- |
| `db` | Committed pending/bound/abandoned/retired publication records, independent GC claims, and run/response retention needs. |
| `cache` | Kernel-visible names, file contents and dirty-data state. Partial staging and a published name are distinct. |
| `disk` | Persisted namespace and contents, advanced independently by file sync, directory sync, or background writeback. |
| `local` | Discovery, sync acknowledgements, uncertain SQL transactions, reader pins and workspace materialization; lost on either crash. |
| `machine` | Process availability and finite crash count. |
| `audit`, `witnessStage`, `witnessed` | Observers for invalid commits, commit counts and reachability; never authorize protocol actions. |

`CrashMode = "process"` retains the kernel filesystem cache. `"power"` drops
unsynced state and reconstructs the visible namespace from `disk`. Background
writeback can persist names and dirty data independently, even while the process
is down. Unsynced data may retain any mixture of written chunks and holes/stale
chunks, including full length with invalid contents. Zero represents a chunk
that differs from the intended candidate; it is not limited to literal zero-fill.
This deliberately conservative writeback model stops once file sync completes.
Sync acknowledgement flags are separate from persisted state:
recovery verifies and repeats the necessary barriers instead of guessing which
ones completed. A completed deletion also requires an acknowledged directory sync.

After power loss, `ResetStaging` validates the pending staging file and truncates
invalid data before rebuilding it. A second crash may expose the old unsynced
contents again; recovery validates again. `ValidPrefix` abstracts a trustworthy
comparison with the pinned candidate, not a length check. The implementation must
either establish that comparison or discard unverified staging and regenerate it;
authenticated streaming/nonce handling is not proved by this predicate. This is
ordinary crash recovery, distinct from later corruption of already synced bytes.
Linux documents that pre-sync file data need not survive consistently.
[Linux ext4 journaling documentation](https://www.kernel.org/doc/html/latest/filesystems/ext4/journal.html).

The model assumes SQLite commits survive the selected failure class. The current
[database connection](../../crates/submilli-server/src/database.rs) selects WAL
and `synchronous=FULL`. SQLite documents durability differences between WAL FULL
and NORMAL; the latter may lose committed transactions on power failure, so it
would not satisfy this model's power-loss premise.
[SQLite synchronous documentation](https://www.sqlite.org/pragma.html#pragma_synchronous).

For the filesystem, file sync and directory sync establish different guarantees:
syncing file data alone does not establish persistence of its containing name.
[Linux fsync documentation](https://man7.org/linux/man-pages/man2/fsync.2.html).
Runtime rename atomicity is useful for publication but does not make the SQL
commit atomic with the rename. Rename may replace an existing destination; the
adapter must prevent replacement under its identity/exclusion protocol.
[Linux rename documentation](https://man7.org/linux/man-pages/man2/rename.2.html).

These are **adapter assumptions**, not a proof of any OS/filesystem/hardware:

- Successful persistence barriers are honored, including required platform and
  device flushes. Parent directories already exist durably. Disk loss, corruption
  of already synced data, lying devices and rollback to mismatched backups are
  excluded; damaged unsynced staging data is explicitly included.
- This namespace abstraction recovers either an earlier persisted name or the
  new persisted name around an unsynced rename. It does not model torn filesystem
  metadata, simultaneous staging/final aliases or cross-directory/cross-volume
  publication. A backend exposing those states needs a refined recovery protocol
  and checks. POSIX runtime rename atomicity alone does not establish this premise.
- Exactly one live process owns the store. Filesystem actions are serialized per
  object. No other process, guest, mutable hard link or writable open handle can
  change published contents. Workspace materialization copies into a distinct
  writable inode; the model abstracts the copy under an open reader pin.
- Crashes kill all local readers and I/O. A cancelled task in a live process must
  drain before releasing its pin. Neither a task timeout nor a node heartbeat
  establishes that draining occurred.
- `Body(o)` is a fixed, validated candidate supplied by the producer. Progress
  after partial staging assumes those bytes can be supplied again. This does not
  prove that an HTTP download can be repeated with the same response. Capturing
  an external outcome and recovering interrupted downloads belongs to SUB-1840;
  an unavailable candidate can remain pending/blocked or be explicitly abandoned.

Missing/corrupt **committed** objects must fail closed, without substituting a
fresh download. Such storage corruption is outside the checked failure model;
validation and error handling are explicit implementation test obligations.
Sealing, quotas, digest collision resistance, reference serialization, backups,
key retention and the mapping from real syscalls to these actions also need tests.

## Properties and checks

| Property | Guarantee checked in the finite model |
| --- | --- |
| `ReferencedObjectsDurable` | Every committed reference has complete, correct, visible and durable bytes/name and cannot be collected. |
| `PublishedBytesImmutable` | Published names, including unreferenced ones, never expose partial or changed bytes. |
| `PendingPublicationProtected` | Collection cannot claim a pending publication. |
| `RequiredReferenceRetained` | Run/response needs cannot outlive their SQL reference. |
| `ReadersProtected` | A pin protects the object from registration through close/drain, including after reference retirement. |
| `CollectionComplete` | A completed collection has durably removed the name. |
| `NoLateReference`, `ReferenceCommittedOnce` | Abandonment/retirement/deletion fences old prepared SQL writes; a logical reference commits once. |
| `PendingCanAdvance` | An online pending publication has a local recovery/publication step, including after staging damage. |
| `PublicationProgress` | Pending work eventually binds or is explicitly abandoned. |
| `CollectionProgress`, `RetentionProgress` | Eligible orphans/references are eventually reclaimed, and releasable retention eventually retires. |

`LiveSpec` assumes finitely many crashes, eventual restart, fair local work and
reader release. Strong fairness for SQL commit excludes endlessly aborted
transactions. Retention progress additionally assumes the run and retention
policy eventually release their needs; it is not a requirement to expire a
recoverable run. There is no fairness assumption on another client submission,
admission of new work, or abandonment. The publication-only progress case
disables abandonment and lifecycle actions, so reaching an abandoned state cannot
stand in for successful publication. Permanent storage failure and never-ending
readers do not have a progress guarantee.

Run with Python 3.9+ and Java 17, from the repository root:

```sh
python3 verification/check.py --model objects
python3 verification/check.py --model objects --suite extended
python3 verification/check.py --model objects --case progress-cleanup
```

The shared [runner](../check.py) verifies the pinned official TLA+ tools
[release/checksum](../toolchain.json), produces exact configurations from
[checks.json](checks.json), and retains model copies, traces, commands, Java
version, completion status and state counts in a temporary evidence directory.
`--java`, `--jar` and `--output` can select existing tools and a new evidence
directory. TLC needs permission to open its local RMI listener. Generated output
is not committed. The [verification workflow](../../.github/workflows/verification.yml)
runs the CI matrix and uploads its evidence alongside the existing models.

The baseline bounds are one object, one reader, two chunks and one crash, with
both abandonment and lifecycle enabled. Separate safety configurations cover
process crashes, two readers, and two objects without a crash. Progress checks
cover publication without retirement/abandonment and the complete lifecycle.
Extended checks cover two power losses and two objects with one power loss.
No symmetry reduction or state constraints suppress interleavings. Deadlock
checking is disabled for legitimate quiescent states; the temporal properties
check the required progress. These are exhaustive **bounded** checks, not a
theorem for arbitrary populations or an implementation proof. TLC fingerprints
also carry a collision risk, reported in its logs.

On 2026-10-15, the pinned release (reporting TLC 2.19, revision `5a47802`)
with OpenJDK 17.0.20.1 completed all eight positive configurations below with
zero states left on the queue. Both progress checks finished temporal-property
checking. All ten negative variants and thirteen reachability checks produced
their expected named invariant counterexamples.

| Configuration | Generated states | Distinct states |
| --- | ---: | ---: |
| `safety-power-loss` | 2,809 | 854 |
| `safety-process-crash` | 3,504 | 911 |
| `safety-two-readers` | 6,683 | 1,538 |
| `safety-two-objects` | 552,447 | 78,961 |
| `progress-publication` | 663 | 207 |
| `progress-cleanup` | 2,809 | 854 |
| `repeated-power-loss` | 4,871 | 1,427 |
| `two-objects-power-loss` | 2,177,037 | 350,310 |

## Counterexamples and implementation obligations

Each deliberately broken variant must fail its named invariant with a trace.
A timeout, parser/runtime error, wrong invariant or an unexpectedly passing
variant fails the verification command. The reachability cases use the correct
protocol and negate `NeverWitness`; a counterexample establishes the selected
path is reachable. The witnesses include lost reservation/reference replies,
partial staging and unsynced rename crashes, reuse after durable publication or
reference commit, rollback/retry, a reader opening after retirement, response
expiry with a run still pinned, orphan collection and recovery of an unsynced
unlink. The two-reader witness releases one reader after retirement while
another still holds its pin. The damaged-staging witness recovers a full-length
invalid staging file, resets it and commits the rebuilt object's reference.

SUB-1846 owns the object-store implementation and these fault tests. SUB-1339
owns the guest-workspace boundary and the later mutable-filesystem tests.

| Fault / model action | Required deterministic fault or restart test |
| --- | --- |
| `early-reference` / `PrepareReference`, `CommitReference` | Pause before publication; SQL must contain no reference/event authorizing consumption. |
| `skip-file-sync` / `SyncFile` | Lose dirty file data while retaining the name; no committed reference may point to incomplete bytes. |
| `skip-directory-sync` / `SyncPublication` | Lose the unsynced rename; file-data sync alone cannot authorize SQL commit. |
| `collect-pending` / `ClaimCollection` | Pause after publication before reference commit; concurrent GC must retain the object and pending pin. |
| `ignore-reader` / `PinReader`, `OpenReader` | Pause between reader registration and open, retire the SQL reference, and run GC; then repeat with two readers and one release. |
| `early-retirement` / `RetireReference` | Expire the response while the run remains recoverable; required bytes must remain. |
| `writable-alias` / `MaterializeWorkspace`, `EditWorkspace` | Edit the guest's materialized download; captured object and replayed bytes must remain unchanged. Include hard links and leaked writable handles. |
| `late-reference` / `Abandon`, `CommitReference` | Prepare SQL, abandon/claim deletion, then attempt the old commit; it must refuse. Also retry after retirement. |
| `unsynced-deletion` / `Unlink`, `SyncDeletion`, `FinishCollection` | Crash after unlink before directory sync; restart must resume deletion and durably remove a resurrected name. |
| `skip-staging-repair` / `ResetStaging` | Retain a staging file's length with holes/stale chunks after power loss. Validation must reset/rebuild it, including a second crash during repair; length alone cannot authorize append, sync or publication. |
| `Reserve`, `Discover`, `Crash`, `Restart` | Kill after reserving, during staging and after publication, without another client call; inspect and recover the same identity. |
| `ForgetTransaction`, `ReadReference` | Inject rollback and lost commit acknowledgement; never duplicate the event/accounting or publish new bytes for a committed identity. |

Real implementation tests must also exercise short writes, disk-full and sync
errors, envelope/integrity mismatch, cancellation with outstanding I/O, bounded
streaming and permissions. A killed process normally leaves the kernel cache
alive; it is not a power-loss test. Power-loss obligations require a controlled
filesystem/storage fault environment or a persistence simulator in addition to
process-restart tests.

## Relation to durable execution and remaining work

The [execution model](../durability/README.md) treats external idempotency and
retained resources as adapter contracts. This model supplies a narrower contract:
a recorded outcome containing an object reference has durable immutable backing,
and retrying its publication uses the same identity. Pending and reader pins
refine the resource-lifetime obligations behind `ResourcesProtected`.
`CommitReference` is the proposed same-transaction boundary for the history event
and reference; SUB-1837 still owns history order, accounting and generation checks.

This is a mapping of obligations, not a machine-checked refinement or composition
of `DurableExecution.tla`. It does not establish idempotent append/move/remove,
the download provider's deduplication contract, or full run replay. The remaining
SUB-1815 filesystem model must provide those resource/effect guarantees before
SUB-1339 is complete.

Multi-node ownership and stale writers remain in SUB-1798/1799. EFS/S3 adapters
must establish their own publication/persistence contract, persistent reader and
backup pins, collection exclusion and fencing. Local POSIX sync/rename rules and
the disappearance of process-local readers cannot simply be carried over to HA.
