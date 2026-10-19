# Mutable filesystem recovery

Recovery actions in these models represent a caller retrying the same operation
after a crash. They do not model a startup scanner or autonomous recovery worker.
Liveness assumes the caller eventually retries; this PR provides idempotent
filesystem operations, not execution scheduling or program resumption.

This directory contains bounded byte-write, append, namespace, directory, reader, listing, writer and
caller-sequence models. The filesystem has no durable-object dependency;
input payloads are supplied by execution replay, while the filesystem retains
only its own mutation evidence. See [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Append protocol

Before writing, bind the effect identity to a digest of the path, input and
filesystem configuration, then save `Prepared { offset }` with the original
byte length. Preparation checks the size and quota and synchronizes the existing
file and parent directory. The append state stores no resolved path or directory
identity. Replay supplies the same path and bytes after restart; a changed
request fingerprint is refused. Each attempt resolves the supplied path through
the same mounts and permission checks. The append writes the payload
at the original length plus the chunk's offset. Recovery starts at the original
offset and rewrites the same range, even when some or all of it already landed.
It never opens the path with append-at-current-EOF semantics on retry.

Synchronize the original file before committing the successful SQL receipt.
Only that receipt permits guest success or the next operation. A retry after
receipt commit returns the recorded outcome without touching the filesystem.
Consequently, a later operation may replace the pathname without an old retry
modifying the replacement. Recorded reads also replay their recorded bytes.

The model distinguishes the saved offset, cached bytes, persisted bytes, a volatile chunk cursor,
acknowledged synchronization, durable SQL state and journal retention.
A process crash keeps cached bytes; power loss restores persisted bytes. Dirty
writeback may persist truncated or damaged appended bytes, while preserving the
previously synchronized prefix. A successful sync prevents later writeback from
undoing those acknowledged bytes. SQL commit may succeed without an observed
acknowledgement.

## Assumptions and unproved boundaries

- Exactly one run accesses the backing filesystem. No conflicting external
  writer, alias, file tool or Git operation runs during pending recovery. Old
  local attempts drain before recovery; this model starts after that drain.
- The file remains at its path while the append is pending; there is no private
  original-file link or old-content hash. Input bytes are volatile and erased on crash. Progress requires deterministic execution replay
  to eventually resupply the same bytes. Fingerprint matching abstracts digest
  collision resistance; this is not an end-to-end execution composition proof.
- The original prefix and containing parent are durable. For a missing target,
  save offset zero, create the file, write the fixed range, synchronize its bytes
  and parent, then save completion. The InitiallyExists configurations check both
  existing files and new files, including a persisted name with incomplete bytes.
- The original length and target path are validated before a new intent.
  Fixed offsets must use checked arithmetic, and the reservation must cover the
  complete appended range. A shorter original prefix is an error; no concurrent modification is supported.
- Successful file synchronization and SQLite commit survive the modeled power
  failure. This is a storage contract, not a claim about every filesystem or
  physical device.
- Replacement is a later, independently durable operation. Its internal
  publication protocol is abstracted here. `NewPathUntouched` checks the old
  append's replay behavior, not the replacement operation's own crash safety.
- The bounded history has one append and one replacement, two payload bytes,
  and one crash (two in the extended check). No leases, stale nodes, concurrent
  writers, recursive trees or Git steps are modeled.

SUB-1295 must establish exclusion with its own TLA+ verification before lock
implementation. Multi-node composition must recheck both exclusion and resource
fencing; a database generation cannot fence a filesystem write.

## Properties and counterexamples

| Property | Meaning |
| --- | --- |
| `ReceiptIsDurable` | A receipt is committed only with durable expected bytes, resource path and recovery evidence. |
| `AppendExactlyOnce` | Completion leaves exactly the original bytes plus one payload. |
| `NewPathUntouched` | Old-effect replay does not restore or modify a later replacement. |
| `ReplayReadStable` | Replay returns the recorded read even after path reuse. |
| `JournalRetained` | Pending/replayable work retains its offset and receipt row. |
| `SuccessAfterReceipt` | Guest consumption follows the SQL receipt. |
| `OriginalPrefixPreserved` | Partial writes and recovery preserve the original prefix. |
| `OriginalOffsetRetained` | Pending retries keep the original saved offset, even when the current file has grown. |
| `Progress` | With finite crashes, eventual restart/storage availability and fair scheduling, completion and eligible journal retirement occur. |

Negative variants deliberately append at the current EOF, refresh the saved offset after a partial write, commit before sync,
retry by a reused path, forget the journal early, omit new-file parent synchronization, or re-read a changed file. Each must
violate its designated property. Witness checks require reachable recovery
after partial writes, sync-before-receipt crashes, lost receipt acknowledgements
and pathname reuse.

## Reproduction and implementation obligations

Use the shared pinned runner:

```sh
python3 verification/check.py --model filesystem
python3 verification/check.py --model filesystem --suite extended
```

The runner keeps copied models, generated configurations, state counts, traces,
Java version and module hashes outside Git. A timeout is not a completed check.

SUB-1339's adapter tests must reproduce partial append at each chunk boundary,
full-length damaged append data, a process kill after file sync but before the
receipt, a lost receipt acknowledgement, and path replacement before duplicate
replay. Reopen real storage and assert exact bytes, stable logical handles,
unchanged replacement files, and cleanup only after work drains. The adapter
must also cover admission failures, source/capability attribution, resource
quotas, file/directory persistence errors and post-effect fuel settlement; those
mechanisms are not established by this finite append model. The separate
[quota model](quotas/README.md) checks public-byte accounting, free private
artifacts and counter reconstruction across crashes.

## Extracted implementation boundary

The implementation lives in `crates/submilli-durable/src/filesystem.rs`, `readers.rs`, `writers.rs` and `io.rs`. The server implements the durable
unit of work and maps records to SQLite. The [unit-of-work model](../unit-of-work/README.md)
checks the atomic admission-and-retention assumption at this boundary. The journal uses filesystem-owned rows and authenticates their scope and
configuration binding. Mutation checkpoints contain only the facts each operation
needs under exclusive ownership; obsolete original-file retention is absent.

Plain byte writes now use a [separate protocol](writes/README.md): repeat atomic
replacement while the outer effect is pending, then let execution history skip
the completed effect. No local write journal or state machine is required. The
namespace replacement model continues to cover writer close.

## Native outcome history composition

SUB-1837 supplies each filesystem call with a stable run/sequence EffectId. A
committed native outcome skips the filesystem call; an absent outcome invokes
it again with reconstructed arguments and the same ID. There is no outer durable
intent row. Filesystem journals and reader/writer/listing cursors remain independent
of `durable_outcomes`. Restored externrefs contain persistent resource IDs; live
file handles are reopened lazily. Closing/finalizing a local handle does not erase
progress needed by a paused run. The execution-history integration test interrupts
24 resource boundaries and checks replay against the same SQLite database.

These models establish filesystem idempotency under their single-writer assumption.
The execution model establishes outcome-before-consumption separately. Neither is
a formal refinement proof of Rust or a proof of multi-node composition.
