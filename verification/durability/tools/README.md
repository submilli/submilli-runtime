# Application-tool callback protocol

This models the SUB-1695 protocol implemented by the server tool coordinator.
It extends the durability verification suite at the HTTP callback handoff.
[DurableExecution](../README.md) checks the generic effect journal and
[RunRecovery](../recovery/README.md) checks exclusive execution ownership and
restart scheduling. This model assumes those guarantees and expands their
application-tool seam. It is not a proof of their end-to-end composition.

Waiting Wasm instances stay in memory, including instances rebuilt at startup.
A call ID encodes the run and logical effect identity. Replay skips completed
effects and reconstructs the first unanswered call. There is no durable pending
table or separate answer receipt. The ordinary effect commit is the acceptance
boundary; the client can retry an unacknowledged answer after a crash.

## Requested checks

| Scenario | Property and reachable example |
| --- | --- |
| Duplicate resume does not store multiple results | `OneResultPerCall`; duplicates before and after commit, including conflicting values |
| Timeout followed by resume does not store the submitted result | `TimeoutNotReplaced`; late resume before timeout commit and after a committed timeout survives restart |
| Execute with the same idempotency key returns the tool again | `OneRunPerKey`, `ExecuteReturnsCurrentState`; repeated execute and a lost execute response |
| Recovery after restart reconstructs the wait | `StableCallIdentity`, `ReplayMatches`, `RecoveryProgress`; reconstruct the same ID and arguments |
| Resume after recovery works | `RunProgress`; answer a reconstructed wait, retry an answer lost before commit, and retry after commit but before acknowledgement |

Other cases send an older call's answer while the next call waits, wrong-run and
unknown-effect IDs, and conflicting duplicate values. Completed calls follow
the existing first-committed-answer policy. Terminal runs return their retained
result or expiry response without consulting tool history. Cleanup can delete
that history before a terminal retry.

## State and actions

| State | Lifetime |
| --- | --- |
| `db` | Durable identity for one execute key, run status, completed effects, and final result |
| `memory` | Server process: replayed values, pending call, uncommitted answer, HTTP waiters, and timeout selection |
| `client` | Observed calls and computed answers, retained across server crashes |
| `online`, `crashes` | Server availability and bounded crash count |
| `audit`, `witnessed` | Ghost evidence for invariants and examples; never used to recover execution or enable protocol actions |

`Execute` admits one key once, then observes the existing run. Its `delivered`
parameter allows a response to be lost before the client learns the pending call.
`Recover`, `Consume`, and `Publish` reconstruct the execution without a connected
HTTP request. Arguments include preceding outcomes so replay mistakes are visible.

`ClientAnswer` can run while the server is down. `Submit` represents HTTP arrival,
`Deliver` gives the answer to the waiting provider, `Commit` records the ordinary
effect outcome, and `Reply` acknowledges it. Crashes can occur between those
actions. Concurrent identical requests are coalesced in a set: multiplicity
cannot change the protocol, but `Submit` can recur before commit. Conflicting
requests remain distinct.

`Reply` abstracts acceptance independently of how long the HTTP handler waits
for the next suspension or completion. Active-run acceptance requires a committed
tool outcome. `SubmissionResolvedBeforeReply` also forbids answering an unresolved
submission while replay is still reconstructing its call, including a premature
`running` response. A terminal reply observes the stored result; it does not claim the
newly submitted answer was applied.

`Timeout` competes with `Deliver` to choose an outcome in memory. `Commit` records
it through the same journal. Late answers cannot replace a selected timeout in
the live process or a committed timeout after restart. As today, subsequent tool
calls after a committed timeout fail immediately. The finite guest consumes
these errors and continues to completion.

`Disconnect` removes HTTP waiters without stopping execution or removing a
delivered answer. `Crash` clears all server memory, including received but
uncommitted answers. `Restart` brings the server online; the supervisor still
has to replay. `Complete` stores the final result, `RetireHistory` deletes effect
history, and `ExpireResult` removes the final payload while keeping the identity.

## Code mapping

| Model action | Rust implementation |
| --- | --- |
| `Publish` | `RunEntry::suspend` |
| `Deliver` | `RunEntry::resume_durable` |
| `Commit` | `ToolHistory::complete`, followed by `RunEntry::committed` |
| `Consume` | `ToolHistory::begin` with a recorded outcome |
| `Reply` | The `handlers::runs::resume_durable` observation loop |
| `Timeout` | The provider timeout and `RunEntry::time_out` |
| `Crash`, shutdown | The surviving process observes `ToolAttempt` cleanup into `Unavailable` on shutdown. A process crash loses the entire registry without running destructors |
| `Execute` | `durable_retry` and `durable_runs::wait` |

`ExecuteReturnsCurrentState`, `TerminalReturnsStoredResult`,
`AcknowledgedCommitted`, and `StableCallIdentity` are monitor-style checks of
values built or observed by the protocol actions. `SubmissionResolvedBeforeReply`
also monitors the response decision. The other protocol safety properties are
emergent constraints across transitions. `TypeOK` checks state
shape, and `NeverWitness` is a reachability monitor rather than a safety claim.

## Negative controls

Each variant must violate its named invariant with a trace. Parser errors,
timeouts, and other invariant violations do not pass.

| Variant | Broken rule | Expected invariant |
| --- | --- | --- |
| `duplicate-result` | Append another outcome for a completed call | `OneResultPerCall` |
| `late-result` | Replace a selected timeout with an answer | `TimeoutNotReplaced` |
| `duplicate-run` | Admit a second run for the execute key | `OneRunPerKey` |
| `retry-running` | Hide a pending tool on repeated execute | `ExecuteReturnsCurrentState` |
| `provider-counter` | Use a provider invocation counter that resets at restart and skips completed calls | `StableCallIdentity` |
| `wrong-call` | Deliver without matching the pending call ID | `CorrectDelivery` |
| `early-ack` | Acknowledge before effect commit | `AcknowledgedCommitted` |
| `replay-ack` | Reply before replay resolves the submitted call | `SubmissionResolvedBeforeReply` |
| `skip-replay` | Reconstruct with a different committed value | `ReplayMatches` |
| `terminal-needs-history` | Require tool history to return a terminal result | `TerminalReturnsStoredResult` |

Witness checks intentionally violate `NeverWitness`, showing each named scenario
is reachable. They complement exhaustive safety checks; they are not progress
proofs.

## Progress and limits

`RecoverySpec` assumes eventual restart and fair local replay/commit scheduling.
`RecoveryProgress` requires a newly admitted or interrupted run to reach a wait
or a terminal state without requiring client activity.

`LiveSpec` additionally assumes the client eventually observes a pending call,
computes an answer, and retries it. Strong fairness for submission and delivery
prevents requests being lost or starved forever. `RunProgress` requires the finite
program to finish. Timeouts are disabled in the client-progress case so they
cannot substitute for answer delivery. No progress is promised during permanent
outage or infinite message loss.

Bounds cover one idempotency key, one or two sequential tools, two distinct
answer values, and one crash in routine cases. The extended suite adds two
crashes with one tool and one crash with two tools and timeouts enabled. Values abstract bounded serialized results or failures.

The model does not establish when a wall-clock deadline expires. A crash before
timeout commit loses that in-memory selection, as it loses an uncommitted answer.
The implementation restarts the timeout for an unresolved wait reconstructed
after recovery. Preserving the original deadline would require a recorded time
observation and is deferred. A committed timeout always survives.

Client-process crashes, exactly-once client-side effects, arbitrary guest control
flow, cancellation/session closure, capacity scheduling, credentials, artifact
pinning, codec limits, and SQLite durability are outside this focused model.
Existing models cover several server guarantees separately. The Rust tests in `crates/submilli-server/src/recovery/tests/tool_callbacks.rs`
exercise HTTP retries and process kills before and after effect commit; the
coordinator tests hold commits open and inject commit failures. These complement
the model without proving the out-of-scope guarantees.

## Run

Use the pinned runner with Java 17 or newer:

```sh
python3 verification/check.py --model durability/tools
python3 verification/check.py --model durability/tools --suite extended
```

`--case NAME` selects a focused check. Logs, generated configurations and traces
stay in the printed temporary evidence directory, outside Git.
