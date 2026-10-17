# Ordered filesystem recovery

A compound operation persists its immutable ordered plan of operation descriptors and fingerprints
before its first mutation. Each step has a derived stable effect ID, its own
prepared intent, persistence barriers and SQL receipt. Recovery replays the
completed prefix and reconciles the pending step before admitting the next.
Replay supplies matching bytes for unfinished steps; it never re-enumerates
sources or recomputes a Git commit. `pinned` denotes filesystem recovery evidence,
not payload storage. Primitive Apply abstracts eventual matching input supply,
as checked explicitly in the append and namespace models.

This model composes two abstract primitive steps. `Apply`, `Sync` and `Receipt`
remain separate; the primitive models supply identity-based reconciliation.
A preflight refusal on the second step commits a definitive failure after the
first completed step. This is a documented partial-result contract, not rollback.
Replay of success or failure must leave later path reuse untouched.

The bounded checks cover one power crash, progress under eventual storage
availability, immutable plans, durable ordered receipts, evidence retention,
and completed-prefix replay. Negative variants regenerate the plan, skip a
persistence barrier, repeat a finished prefix or collect evidence too early.
Witnesses require recovery after a prefix crash and replay of partial failure.

Prepared Git publications use the `publication` configuration: every failure
leaves the operation pending until it can finish the same immutable plan. No
definitive partial-failure outcome is allowed. Its Git objects and refs, including
the commit ID, must be reconstructed from recorded execution outputs. This is roll-forward recovery;
it does not verify the ordinary Git host function's existing in-memory undo
mechanism, and those host calls are not automatically routed through this API.
Execution history must record the compound outcome before exposing it to a
program. One exclusive run and drained old workers remain assumptions.

```sh
python3 verification/check.py --model filesystem/sequences
```
