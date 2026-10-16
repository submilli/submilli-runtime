# Shared effect storage

This is the storage subset of SUB-1837 used by filesystem recovery. It replaces
`durable_filesystem_effects` with `durable_effects` keyed by `(run_id, effect_id)`.
The table has no filesystem-specific columns. `operation` and `version` identify
the adapter codec; `request`, `recovery_payload` and `outcome` are sealed separately
with authenticated run/effect/operation/version/field identity. The existing
sealer includes its format and encryption-key identity.

The shared lifecycle in `crates/submilli-durable/src/effects.rs` operates inside
a caller-owned `UnitOfWork`. The server maps its records through
`crates/submilli-server/src/adapters/unit_of_work/durable_state.rs`. Callers perform admission and
resource checks and pin referenced objects in that same transaction. The current
filesystem caller retains its single-run exclusion, plan/cursor/writer gates and
run-lifecycle checks. The storage helper alone is not permission to dispatch an
effect, supersede a pending compound plan or write from a stale execution attempt.

| Transition | Shared storage | Filesystem caller |
| --- | --- | --- |
| Admit | Immutable request; at most one unfinished effect per run | Pin captured content with request admission |
| Prepare | Write recovery payload once | Record prepared intent before public mutation |
| Complete | Commit immutable outcome after preparation | First finish file/directory persistence barriers |
| Reject | Commit definitive refusal only before preparation | Preflight refusal before public mutation |
| Retry | Same identity/request/payload/outcome reconciles; conflicts fail | Return recorded outcome without filesystem mutation |

The payload format for `filesystem.mutation`, version 1, includes the workspace,
optional compound plan and mutation in the request, a durable filesystem `Intent` as recovery
payload, and a completed/refused outcome. It validates intent-to-request identity
when reading. Large file bytes stay in the object store. Records have a 64 KiB
plaintext bound and a run has at most 10,000 effect rows. These are the existing
filesystem bounds, now enforced by the common storage helper; configurable
history byte/replay budgets remain future work. No truncation is allowed.

## Verification

`SharedEffects.tla` models one run with two effect slots and two operation/version
identities. Two request values independently vary the immutable arguments;
recovery data and outcome bytes each have two abstract values. Admission, preparation and completion are separate atomic SQL commits.
A process crash can lose a completion acknowledgement without losing its row.
Checks cover immutable identity/payload/outcome, one unfinished effect, refusal
before preparation and receipt-before-return. Negative variants remove each of
those guards; witnesses require a conflicting adapter, changed request and replay
of an outcome that was unacknowledged at the crash.
This is a bounded safety model, not a liveness or end-to-end execution proof.

```sh
python3 verification/check.py --model effects
```

Filesystem persistence ordering remains covered by the append, namespace,
directory, resource and sequence models. Their SQL intent/receipt actions map to
fields on the shared row; no second filesystem receipt is required. Their
exclusive-run and durable-storage assumptions are unchanged. Rust tests verify
cross-adapter identity conflicts, field/row/version authentication, byte bounds,
transaction rollback, reopen/lost acknowledgement and the existing filesystem
process-kill/reconciliation cases. TLA+ does not prove the Rust implementation,
encryption, SQL isolation or storage barriers.

## Remaining SUB-1837 work

These rows represent effect state, not the append-only `DurableExecution.tla`
history. That model's `Commit` must additionally check expected sequence, attempt
generation and run state, and atomically append the event, advance committed
history, settle accounting and retain objects. Observation events and terminal
evidence are not implemented here. An observation-only effect need not use this
prepared-mutation lifecycle; its eventual representation requires an explicit
model mapping rather than a fabricated filesystem preparation.

The future history bridge must use these shared rows as the authority for
primitive effect identity and outcome. Linking a filesystem result to ordered
history and exposing it to guest execution remains to be implemented and verified;
this change does not claim `AppendOnly` or `NoStaleWrites` from the complete
execution model. Resource cursors and compound parent outcomes remain separate
protocol state until that bridge supplies logical checkpoints and event ordering.

Migration 10 is being revised within the unmerged filesystem PR. Existing main
schemas upgrade normally; databases created with the earlier PR-only migration
have a different checksum and are intentionally rejected, not silently rewritten.
