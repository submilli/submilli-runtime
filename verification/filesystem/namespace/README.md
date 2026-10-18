# Regular-file namespace retry

Plain `write_bytes` is covered separately by [WriteRecovery](../writes/README.md).

This model checks journaled replacement for writer close, same-volume regular-file moves, and
regular-file removal under exclusive ownership. The caller resolves a pending
operation before admitting another mutation of its paths. Restart drains the
old attempt before retry. SUB-1295 will enforce exclusion with separately
verified locks; this model does not establish multi-node fencing.

## Operation state machines

Each operation advances from Initial to Prepared to Completed. The model
abstracts request admission and preflight validation into Intent; the separate
operation-journal model checks those SQL transitions. Native writes, directory
synchronization and receipts remain separate crashable actions.

`Prepared` is a marker with no operation-specific fields. Replay supplies the
paths again; the journal's immutable request digest rejects changed arguments.
Neither resolved entries nor parent/candidate inode identities are saved. The
replacement candidate has a deterministic name derived from the close effect ID;
native retry compares its current identity with the destination to recognize
publication. The model's name/version variables describe live and persisted
filesystem contents, not extra SQL recovery fields.

- **Replace:** prepare the new bytes in a private candidate, synchronize them,
  publish the candidate at the destination, synchronize its parent, then save
  completion. The old destination is neither backed up nor hashed. Replay
  supplies replacement input again; damaged unfinished candidate bytes can be
  repaired. Rust retains the candidate name while publishing another link to
  that same file, so a retry at full quota does not allocate another copy.
- **Move:** keep the source at its public name, publish a link to it at the
  destination, synchronize the destination parent, remove the source, then
  synchronize the source parent. No separate original/destination backup is
  needed. The destination must be durable before removing the source.
- **Remove:** record the prepared deletion while the file exists, remove it,
  synchronize its parent, and save completion. An absent path on prepared retry
  means this deletion already happened. The removed bytes need no backup.

Completed retries return the saved outcome without examining or changing the
filesystem. The same owner may subsequently reuse either pathname; those later
files must remain untouched by retries of the completed effect.

## Move states

| State after restart | Retry |
| --- | --- |
| Source exists; destination has its old contents | Publish the source at the destination, sync destination, remove source, sync source. |
| Source and destination name the same file | Sync destination, remove source, sync source. |
| Source absent; destination names the moved file | Repeat parent synchronization and save completion. |

The source cannot disappear before the destination is durable under this
ordering. A negative variant deliberately removes it too soon, allowing a power
crash to lose the only copy. Source-absent/old-destination is outside the correct
protocol, rather than a reason to retain an extra backup for every operation.

## Persistence and verification

Visible names and persisted names are separate. Directory entries can reach
storage independently. Candidate bytes have cached, persisted and synchronized
states; dirty writeback can be partial or corrupt. Successful file/directory
synchronization and SQL commits survive the modeled power failure. The initial
source bytes and namespace are durable. These are explicit storage assumptions,
not guarantees for arbitrary filesystems or hardware.

The semantic invariants require durable completed results, surviving move data,
no writes to later reused paths, and no success before the SQL receipt. They do
not require preserving bytes that a replacement or deletion intentionally removes.
The journal remains available while replay is possible. Negative controls cover
early source removal, omitted file/directory sync, forgotten receipts and
reapplying completed work. Witnesses cover intermediate move states, publication
before receipt, lost SQL acknowledgements and later path reuse.

Progress assumes finite crashes, eventual storage availability, matching replay
inputs and fair retries. Input bytes are not stored in SQL. The model does not
cover directory-tree moves, cross-volume moves, open-reader snapshots, external
writers or automatic startup recovery. Rust tests cover the additional private
publication syscall, capability paths, quotas, cancellation and failure injection.

```sh
python3 verification/check.py --model filesystem/namespace
python3 verification/check.py --model filesystem/namespace --suite extended
```
