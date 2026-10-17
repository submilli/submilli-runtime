# Mutable filesystem recovery

This directory contains bounded append, namespace, directory, reader, writer and
compound-operation models. The filesystem has no durable-object dependency;
input payloads are supplied by execution replay, while the filesystem retains
only its own mutation evidence. See [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Append protocol

Before writing, persist the effect identity, input fingerprint,
original resource identity and original byte length in the intent transaction.
That transaction records recovery metadata, not input bytes. Replay supplies the
same bytes after restart; a changed fingerprint is refused. The append writes the payload
at the original length plus the chunk's offset. Recovery starts at the original
offset and rewrites the same range, even when some or all of it already landed.
It never opens the path with append-at-current-EOF semantics on retry.

Synchronize the original file before committing the successful SQL receipt.
Only that receipt permits guest success or the next operation. A retry after
receipt commit returns the recorded outcome without touching the filesystem.
Consequently, a later operation may replace the pathname without an old retry
modifying the replacement. Recorded reads also replay their recorded bytes.

The model distinguishes cached bytes, persisted bytes, a volatile chunk cursor,
acknowledged synchronization, durable SQL state and retained recovery evidence.
A process crash keeps cached bytes; power loss restores persisted bytes. Dirty
writeback may persist truncated or damaged appended bytes, while preserving the
previously synchronized prefix. A successful sync prevents later writeback from
undoing those acknowledged bytes. SQL commit may succeed without an observed
acknowledgement.

## Assumptions and unproved boundaries

- Exactly one run accesses the backing filesystem. No conflicting external
  writer, alias, file tool or Git operation runs during pending recovery. Old
  local attempts drain before recovery; this model starts after that drain.
- The original inode remains durably recoverable while pinned. Input bytes are
  volatile and erased on crash. Progress requires deterministic execution replay
  to eventually resupply the same bytes. Fingerprint matching abstracts digest
  collision resistance; this is not an end-to-end execution composition proof.
- The file and containing path already exist durably. Creating a new append
  target requires a separately verified namespace-publication protocol.
- The original length and resource identity are validated before a new intent.
  Fixed offsets must use checked arithmetic, and the reservation must cover the
  complete appended range. Unexpected identity/length changes fail explicitly.
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
| `RecoveryEvidenceRetained` | Pending/replayable work retains its resource evidence. |
| `SuccessAfterReceipt` | Guest consumption follows the SQL receipt. |
| `OriginalPrefixPreserved` | Partial writes and recovery preserve the original prefix. |
| `Progress` | With finite crashes, eventual restart/storage availability and fair scheduling, completion and eligible cleanup occur. |

Negative variants deliberately append at the current EOF, commit before sync,
retry by a reused path, clean up early, or re-read a changed file. Each must
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
mechanisms are not established by this finite model.

## Extracted implementation boundary

The implementation lives in `crates/submilli-durable/src/recovery/`, `filesystems/` and `journal/`. The server implements the durable
unit of work and maps records to SQLite. The [unit-of-work model](../unit-of-work/README.md)
checks the atomic admission-and-retention assumption at this boundary. Existing
protocol actions, record versions and encryption associated data are unchanged.
