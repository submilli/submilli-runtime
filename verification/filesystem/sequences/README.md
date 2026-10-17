# Caller-code replay

The caller's program executes two primitives in order using stable identities.
There is no persisted plan, step list, aggregate filesystem outcome or recovery
dispatcher. On restart the caller starts its code again. A completed primitive
returns its receipt; a pending primitive reconciles its intent before returning.
The next call follows from the code's control flow.

`cursor` is volatile execution position, reset by a crash. `intent`, `receipts`
and `refused` are filesystem journal state. `applied` and `persisted` distinguish
cached effects from synchronized filesystem state. `Apply`, `Sync`, `Receipt`
and `Return` remain separate; primitive models supply identity-based reconciliation
and matching replay input. A preflight refusal on the second operation is a
persistent result, leaving the completed first operation intact, without rollback.

`outer` abstracts a future caller's independent completion record. A crash after
both filesystem receipts but before that record leaves it pending. Replaying
calls still uses the filesystem receipts and cannot repeat those mutations.
This illustrates the retry contract; it does not implement or prove the complete
execution-history integration, stable ID assignment or nondeterministic replay.

Checks cover one power crash, ordered durable receipts, retained evidence,
receipt-before-outer-completion, no repeated completed mutations, and progress
under finite crashes and fair caller retries/storage actions. Negative variants
skip synchronization, repeat a completed operation or discard evidence early.
Witnesses require a crash after a completed prefix, replay of a persisted refusal,
and the gap between filesystem receipts and outer completion. Evidence retention
is a caller obligation; this component does not implement lifecycle cleanup.
There is no prepared Git publication protocol in this model or adapter.

```sh
python3 verification/check.py --model filesystem/sequences
```
