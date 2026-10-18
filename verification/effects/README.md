# Filesystem operation journal

This model checks the filesystem's own operation journal. It is not shared with
future execution effects. The runner path `effects` is retained for existing CI
commands. `FilesystemOperations.tla` maps to the direct operations in `submilli-durable/src/filesystem.rs`
and the server's `durable_filesystem_operations` table.

Rows are keyed by caller-supplied scope and operation ID. Operation/version select
the codec. The immutable request includes digests of the arguments and the
configured filesystem. The request and one optional encoded `state` payload are
sealed separately with scope/operation/version/field identity. No file input bytes
are stored: requests contain digests, and prepared states contain only retry facts.
Each operation owns its enum and codecs. Missing state means `Initial`; unknown
or corrupt saved data returns an error. The current codec version is 4.

`Prepared` contains retry facts only when needed: append stores its original
offset, while mkdir, remove, move and writer close use a marker with no fields.
That marker remains distinct from absent state. `Completed(Ok/Err)` replaces it with the
result. There are no separate recovery/outcome fields or rejection flag. The SQL
`status` column is derived from the state for its one-pending-operation index,
written atomically with it, and validated against decoded state on read. Native
execution dispatches on the enum, never on this index value.

| Transition | Filesystem journal |
| --- | --- |
| Admit | Immutable request; at most one unfinished mutation per scope. |
| Prepare | Save `Prepared` before public mutation, with retry facts only when needed; pending state cannot change. |
| Complete | Replace prepared data with immutable `Completed(Ok(result))` after file/directory barriers. |
| Reject | Save `Completed(Err(reason))` only from Initial. |
| Retry | Same identity and arguments return the recorded outcome; changes fail. |

The caller handles resource admission in the same transaction. There are no
workspace, run-state or plan lookups. Records have a 64 KiB plaintext bound and
a scope has at most 10,000 operation rows. Future history budgets and large
outputs remain separate work.

## Verification

The model has two operation slots, two codec identities, two immutable request
values, two prepared-data values plus a marker without data, and two completed results. Each row
has one `state`, with a variant and value. `prepared` and `completed` are ghost
history for invariants, not extra SQL fields; replacing prepared data at completion
does not weaken the check that pending facts were immutable. Requests abstract
both arguments and filesystem configuration binding. Admission, preparation and
completion are separate SQL commits. Crashes may lose acknowledgement without
losing the row. Checks cover immutable identity/request/intent/outcome, one
unfinished operation, refusal before preparation and receipt-before-return.
It also checks that state never resets, success follows preparation, and the
derived status always matches the state. Fifteen cases include negative variants
that forget saved state, corrupt the derived index or complete too early, as well
as the existing guard-removal controls. Witnesses require changed identities, changed
requests, completion from a prepared marker without data, and replay of an outcome
unacknowledged at the crash.

```sh
python3 verification/check.py --model effects
```

This bounded model does not prove Rust, encryption, SQL isolation or filesystem
barriers. Primitive filesystem models establish the barrier ordering. The
caller-sequence model checks that code can replay receipts and finish a pending
operation without a stored plan, including a crash after filesystem completion
but before an outer completion record. SQL tests cover field authentication,
size bounds, rollback and reopen; filesystem tests exercise explicit retries.

## Future execution history

SUB-1837 will own ordered execution history, observation outputs, accounting,
run state and resumption. It may wrap filesystem calls in separate execution
effects. If its completion is missing, it must retry the same filesystem scope
and operation ID: journaled operations return the completed state's result.
Plain byte writes have no journal and repeat until the framework records their
completion; [their model](../filesystem/writes/README.md) covers that separate contract.
This PR does not implement that bridge or prove end-to-end execution composition.
Neither component requires durable-object storage; outputs can start in SQLite.

Migration 10 is revised within the unmerged filesystem PR. Main schemas upgrade
normally; earlier PR-only migration checksums are intentionally incompatible.
