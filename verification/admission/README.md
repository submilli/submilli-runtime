# Run preparation and admission

SUB-1836 introduces persistent identity for HTTP session, one-shot and MCP program
executions. Caller idempotency is optional; it is not what makes a run persistent.
This incremental implementation does not yet record replay inputs or effects.
Every row explicitly has `recovery_mode=unrecorded` and `history_position=0`.
Interrupted work becomes indeterminate; it cannot be automatically launched again.

The existing [durability model](../durability/README.md) remains the contract for
the complete recovery implementation. `RunAdmission.tla` separately checks the
compatibility boundary needed to land its first part without changing safe HTTP
pre-dispatch retries:

1. Commit a prepared run with stable identity, session order and optional key.
2. Prepare the execution environment. A proven pre-dispatch failure may abort
   preparation and release its key, retaining the aborted run's identity.
3. Commit the claim immediately before invoking the runner. Only a confirmed
   claim authorizes dispatch. Reconcile a lost acknowledgement using the same ID.
4. Complete once, or retain indeterminate evidence after an interruption. A
   claimed run's key cannot be released even if the caller never saw its reply.

The key scope remains the authenticated session. Version 1 fingerprints remain
SHA-256 of the submitted code's UTF-8 bytes. No client metadata is interpreted as
an idempotency key. Each unkeyed submission creates a distinct run.

## Mapping and bounds

| Model action/property | Implementation boundary / evidence |
| --- | --- |
| Stage, CommitPreparation, Observe | Shared coordinator preparation and the existing unit of work; concurrent admission and lost-ack tests. |
| Admit, Dispatch, CommitBarrier | Conditional claim before `runner::run`; repeated claim rejected and lost claim acknowledgement reconciled. |
| Release, KeyReserved | `abort_preparation` releases only a never-claimed key; release after claim becomes indeterminate. |
| Complete, NoStaleWrites | Revision/startup-guarded SQL writes; closure fences pending completion. |
| AdmissionUnique, OrderUnique | Scoped unique key and order constraints, transactionally allocated session counters. |
| Crash, Recover | Real child-process exit and database reopen; no synthetic replay history. |
| Expire | Existing response TTL/newest-three policy removes bytes, retaining key and terminal evidence. |

The bounded configuration has two potential runs, one optional caller key, two
code fingerprints and one crash. It distinguishes commit from observation. Three
negative variants permit duplicate keys, release of an admitted key, or stale
completion. Four witnesses require unkeyed dispatch, observation after an
unacknowledged preparation commit, a crash after admission, and dispatch after
concurrent session closure to be reachable.
This is a safety model of compatibility admission, not a progress proof or a
proof of SQL, serialization, storage durability or the interpreter.

The compatibility runner does not enforce the complete model's `EarlierDrained`
scheduler yet. Persistent preparation order provides the input for that later
scheduler; it must not be described as verified durable execution order today.
No artifact pins, terminal history/last-result atomicity, detached recovery,
filesystem protocol or multi-node ownership is implemented by this increment.
Session closure retains run evidence and fences completion, but resource-drain
and recoverable workspace protection remain obligations of the lifecycle and
recovery issues before replay can be enabled.

## Storage and migration

Migration 7 copies all legacy identities and exact response bytes into `runs`.
Imported unfinished reservations become indeterminate immediately. Legacy
execution IDs are extracted only when present as strings in valid JSON;
otherwise they remain absent. The old reservation ID remains the stable primary
identity for imported records. New executions use their execution ID throughout.
A store-local node ID survives restart and preparation order is never recycled.
The old table is removed, so there is only one admission authority.

Unkeyed completions retain metadata without adding response payload storage.
Keyed response storage and its existing failure semantics remain compatible;
SUB-1841 must add the complete terminal-history/result publication barrier, and
SUB-1846 owns the shared external payload store. This migration is forward-only;
roll back binaries only together with a suitable pre-upgrade database backup.

Run this model with `python3 verification/check.py --model admission`.
The existing runner pins TLC and retains evidence outside Git. The protocol CI
workflow runs both admission and full durability configurations.

The compatibility runner may start after concurrent session closure if its claim
was already acknowledged. `Dispatch` deliberately permits that race, and the
`late-dispatch` witness exercises it: closure retains the key and fences terminal
writes even when a launch permit is already in memory. Draining active work
before releasing session resources remains a separate lifecycle obligation.
