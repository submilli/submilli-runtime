# Filesystem operation journal

This model checks the filesystem's own operation journal. It is not shared with
future execution effects. The runner path `effects` is retained for existing CI
commands. `FilesystemOperations.tla` maps to the direct operations in `submilli-durable/src/filesystem.rs`
and the server's `durable_filesystem_operations` table.

Rows are keyed by caller-supplied scope and operation ID. Operation/version select
the codec. The immutable request includes digests of the arguments and the
configured filesystem. Request, recovery intent and outcome are independently
sealed with scope/operation/version/field identity. No file input bytes are
stored: requests contain digests; checkpoints retain only required identities and offsets; retries supply matching bytes.

| Transition | Filesystem journal |
| --- | --- |
| Admit | Immutable request; at most one unfinished mutation per scope. |
| Prepare | Store recovery intent once, before public mutation. |
| Complete | Store immutable outcome after file/directory persistence barriers. |
| Reject | Store a definitive refusal only before preparation. |
| Retry | Same identity and arguments return the recorded outcome; changes fail. |

The caller handles resource admission in the same transaction. There are no
workspace, run-state or plan lookups. Records have a 64 KiB plaintext bound and
a scope has at most 10,000 operation rows. Future history budgets and large
outputs remain separate work.

## Verification

The model has two operation slots, two codec identities, two immutable request
values, and two values each for recovery data and outcomes. Requests abstract
both arguments and filesystem configuration binding. Admission, preparation and
completion are separate SQL commits. Crashes may lose acknowledgement without
losing the row. Checks cover immutable identity/request/intent/outcome, one
unfinished operation, refusal before preparation and receipt-before-return.
Negative variants remove guards. Witnesses require changed identities, changed
requests and replay of an outcome unacknowledged at the crash.

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
and operation ID: the filesystem receipt prevents repeating the mutation.
This PR does not implement that bridge or prove end-to-end execution composition.
Neither component requires durable-object storage; outputs can start in SQLite.

Migration 10 is revised within the unmerged filesystem PR. Main schemas upgrade
normally; earlier PR-only migration checksums are intentionally incompatible.
