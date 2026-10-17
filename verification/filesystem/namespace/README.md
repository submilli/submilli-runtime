# Regular-file namespace recovery

This SUB-1815 slice checks staged replacement, same-filesystem regular-file
move, and regular-file removal. It complements the [append model](../README.md)
and [staged writer protocol](../writers/README.md). It does not verify
directory trees, cross-volume moves, Git transactions or open-resource replay.

## Protocol

1. Commit an immutable intent identifying the original file, expected source
   and destination versions, operation, and replacement input fingerprint when
   applicable. No public namespace mutation precedes the intent.
2. Retain the original inode under a private recovery name on the same
   filesystem. Creating that name and synchronizing its parent directory are
   distinct steps. Do not mutate the source until this name is durable.
3. For replacement, prepare a separate mutable candidate from replay-supplied
   bytes, validate it and synchronize its bytes before publishing its name.
   Recovery may rewrite damaged uncommitted candidate bytes.
4. Apply or recognize the original namespace mutation by resource identity.
   Synchronize each affected parent directory separately. Commit the SQL
   outcome only after all required barriers succeed, then expose guest success.
5. If the outcome already exists, replay it without touching the namespace.
   Later path reuse must not authorize the old operation to remove or replace
   a newer entry. Release recovery evidence only after terminal work drains.

The recovery name is a mutable-file retention mechanism, not an immutable
input archive. A hard link can retain a
regular inode locally, but guest access to recovery names and aliases must be
excluded. Directory hard links are not a solution for directory-tree recovery.

## Persistence and reconciliation

`names` and `diskNames` distinguish visible and persisted namespace bindings.
The two paths may have different parents. `Apply` publishes a private hard
link at the destination; `SyncDirectory("destination")` persists it before
`RemoveSource` unlinks the source. Both public names can point to the original
between those steps, including after a process crash. Source synchronization
and SQL receipt follow. Replacement and remove each have one public step.

Recovery checks recorded identities before touching either name:

| Recovered move state | Adapter action |
| --- | --- |
| Original source; original destination | Publish the retained original at the destination, sync, then unlink the source. |
| Original source; destination already names original inode | Sync destination, then unlink the duplicate source name. |
| Source absent; destination already names original inode | Recognize publication and repeat parent synchronization. |
| Source absent; original destination remains | Republish the retained original. This is defensive recovery, unreachable under the modeled destination-before-source persistence ordering. |

Private staging links use a stable effect-specific name and never change
public bindings before publication. The model abstracts their creation behind
the retained original; Rust tests cover those extra syscalls. Unexpected
versions fail closed instead of becoming a new request.

Replacement data has separate cached, persisted and synchronized states.
Dirty candidate writeback may persist incomplete or corrupt data. A successful
file sync prevents later dirty writeback from undoing acknowledged bytes.
Publication, parent sync and SQL receipt remain different actions. A SQL
receipt can survive a crash before its acknowledgement is consumed.

## Bounds and assumptions

One exclusive logical run performs one operation, followed by abstracted later
path replacements and duplicate replay. There are two public paths and one
retained original. The positive checks cover one process/power crash and two
power crashes in the extended configurations. Storage failures may stutter;
progress assumes eventual storage availability, finite crashes, eventual
restart and weak fairness of recovery actions.

The original bytes and initial namespace already exist durably. Candidate
input bytes are volatile. Crash clears `inputAvailable`; `SupplyInput` abstracts
deterministic replay providing the same bytes again. Progress requires that
supply eventually succeeds. No input payload is stored by this model. SQL intent and
outcome commits are atomic. Recovery has drained the previous attempt and
excludes every external writer/alias. No stale-node fencing is established.
The replacement of paths after the first receipt is an already-durable later
operation; this model tests old-effect replay against it.

This slice retains only the source inode. A runtime with an open reader on an
overwritten destination must separately retain that destination version, and
resource reconstruction must be checked before claiming transparent handle
replay. Preflight errors and application-visible rollback/error outcomes also
need separate history rules; this model covers an admitted mutation that
eventually completes successfully.

## Checks and implementation obligations

```sh
python3 verification/check.py --model filesystem/namespace
python3 verification/check.py --model filesystem/namespace --suite extended
```

Safety checks require durable receipts, recoverable original files, retained
evidence, no stale mutation of reused paths and no guest success before SQL
completion. Progress checks cover replace, move and remove. Negative variants
omit backup, file or directory synchronization, collect evidence too early, or
reapply an old operation to reused paths. Witnesses require partial namespace
persistence, publication without a receipt, lost receipt acknowledgements and
replay after path reuse.

SUB-1339 must map each action to actual capability-relative operations, capture
metadata semantics, reserve quota for retained/staged files, and inject failures
before and after every syscall and SQL commit. Kill/restart tests must cover
all four move states above, missing/corrupt recovery evidence, incomplete
replacement data, cancellation while a worker still owns files, directory
sync failure, and cleanup retries. A passing finite model does not establish
these Rust or filesystem guarantees by itself.
