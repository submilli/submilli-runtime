# Single-node durable execution protocol

SUB-1797 introduced the bounded model; SUB-1837 aligns it with the native
outcome history now implemented. Public enablement and startup scheduling remain
SUB-1842. Each run retains its initial source, blueprint, variables and limits
once. Recovery reruns that program from the beginning, using committed outcomes
until it reaches new work.

    reconstruct arguments -> dispatch -> native outcome -> commit -> guest conversion
                                                    replay ----^ (skip dispatch)

There is no persistent intent or pending effect row. A call's ID comes from the
run ID and its ordered position. A missing outcome retries that call with the
same ID and reconstructed arguments. The framework does not classify adapters
as idempotent. A crash after an external mutation but before outcome commit can
repeat that mutation. Provider deduplication is an explicit assumption only for
`NoDuplicateMutation`; the matrix also checks non-idempotent providers and
witnesses their duplicate mutation. Filesystem idempotency is verified separately
in [the filesystem models](../filesystem/README.md). HTTP idempotency keys remain
the caller's responsibility; code and Git idempotency are follow-up work.

## Run the checks

Use Python 3.9+ and Java 17:

    python3 -m unittest discover -s verification/tests -p 'test_*.py'
    python3 verification/check.py
    python3 verification/check.py --case progress
    python3 verification/check.py --suite extended

The runner downloads the official TLA+ tools
[1.7.4 release](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4) into
the system temporary directory and checks the SHA-256 pinned in
[toolchain.json](../toolchain.json). Java is supplied by the caller; the runner
does not install it. Use --java /path/to/java and --jar /path/to/tla2tools.jar
for an existing installation; the jar must match the pin.

TLC uses a local RMI listener even for this single-process check. An environment
that prohibits local sockets must permit that listener to run TLC.

--list lists checks without Java. --case can be repeated.
--output /new/empty/directory chooses where evidence goes. By default it goes
to a new temporary directory. Each check retains the exact model, generated TLC
configuration, command and checker output. The summary.json file records
constants, model hashes, exit codes, completion status, elapsed time and state
counts. Do not commit these generated artifacts.

[checks.json](checks.json) is the versioned configuration matrix.
[safety.cfg](safety.cfg) also provides a directly runnable baseline configuration
for editor use. To run it directly from this directory:

    java -XX:+UseParallelGC -Xmx1g -cp /path/to/tla2tools.jar tlc2.TLC \
      -workers 1 -config safety.cfg -metadir /tmp/submilli-tlc-states \
      DurableExecution.tla

The [CI workflow](../../.github/workflows/verification.yml) runs the bounded
matrix for pull requests, main and merge-queue checks, independently of the Rust
suites. It retains model-checking evidence as a workflow artifact. A failed
check stops the runner. Timeout, parse errors, incomplete exploration, and the
wrong counterexample all fail the gate.

## What is modeled

The state is deliberately split:

| State | Meaning |
| --- | --- |
| db | Committed admissions, fingerprints, attempt generations, histories, accounting, results, response retention, session status and resource pins. |
| process | Volatile program position, replayed values, staged database writes and received replies. A crash erases these. |
| flight, finished | Dispatched requests and completed requests, independent of process lifetime. |
| provider | Durable deduplication receipts and outcomes, keyed by logical effect ID. |
| monitor | Observer state for acknowledgement, dispatch, stale-commit and client-response properties. It cannot enable production actions. |
| witnessed | A reachability observer used only to demonstrate a named scenario. |

Database commit and acknowledgement are separate. Prepare stages a write;
Commit atomically changes the database but leaves the staged write untouched.
ForgetWrite before commit represents a transaction abort; afterwards it
represents an acknowledgement or a lost acknowledgement resolved by rereading.
The guest consumes only committed history. Crashing after Commit erases the
caller's knowledge while preserving the database.

Claim commits a generation before Launch starts the interpreter. A local
supervisor can rediscover a claim without a client or notification. Only one
attempt per run per startup is represented. The generation abstracts the
implementation's startup/attempt identity; cancellation advances its fence.
An independently prepared old write can race cancellation at Commit.

The finite guest program alternates observations and external calls. Two opaque
observation/outcome values stand in for serialized values or catchable errors.
Random, UUID and clock reads share this observation protocol. A call's arguments
contain its pinned input fingerprint, logical position and all preceding values,
so incorrect replay changes the request. Each distinct call has its own ID.
Pure computation, codecs, interpreter scheduling and arbitrary guest control
flow are abstracted.

Each committed observation or external outcome records one logical unit. Replay
restores that spend; it does not grant a new logical budget. An uncommitted retry
can incur physical cost again. A short budget
blocks the run and keeps its resources. Granting more fuel is outside this
model. Compatible = FALSE represents a run whose pinned artifacts cannot be
executed, and also blocks it.

One session contains up to two admitted runs in the CI configurations. They
must execute in admission order, including draining earlier in-flight work.
Cancellation prevents fresh dispatch and fences staged commits, but cannot
retract a request already sent. Cleanup waits for both interpreter work and
in-flight requests to drain. This is a conservative resource-retention policy.
The model does not prove that cancellation undoes remote effects.

Requests from different startups may overlap. Within one startup, a retry waits
for the previous transport request to finish and retires its reply before reusing
that request identity. Timeout retries that overlap within the same startup are
outside these bounds; an implementation that permits them must distinguish and
track every outstanding request and extend the model before relying on its
cleanup result. The stable provider key remains identical across these requests.

Response expiry removes availability of the retained response, while its key,
fingerprint and terminal evidence remain. Retention eligibility is abstract:
the 24-hour/newest-three policy is not a clock/counting model here. A legacy
indeterminate reservation has no invented history and is never recovered.
Session expiry cannot erase a recoverable or draining run. Physical deletion,
backup/restore and storage corruption are outside this model.

## Properties and bounds

| Property | Checked guarantee |
| --- | --- |
| AdmissionUnique, KeyReserved, ClientResponseMatches | One key admits one run; conflicts refuse; responses match the original request; expiry cannot reopen admission. |
| HistoryConsistent, EffectHistoryWellFormed, AppendOnly | Ordered immutable history, atomic sequence/accounting advancement and outcomes backed by the intended effects. |
| ReplayMatches, StableEffectIdentity, ProviderArgumentsMatch | Replay preserves observations; retries preserve IDs and arguments. |
| NoDuplicateMutation | Under provider deduplication, repeated requests cannot create two logical mutations. |
| CommitBarriers, TerminalConsistent | Outcome before consumption, terminal result before acknowledgement, and atomic publication of the session's last result. |
| NoStaleWrites, SessionOrder, ResourcesProtected | Fenced commits, ordered execution and retention of resources needed by recoverable/draining work. |
| RequestSetsDisjoint | A transport request cannot be both outstanding and completed; retry retires its previous completion. |
| LegacyNotRecovered | No recovery of a reservation without a replay manifest/history. |
| Progress | Admitted eligible work advances under the separately declared liveness assumptions. |
| CleanupProgress, ReleaseProgress | Terminal work drains, and lifecycle cleanup eventually releases its resource pins. |

The standard safety configuration has one run, three logical calls, two
possible values and one crash. Separate CI configurations cover two runs/two
calls/one crash with cancellation and retention, a short budget, incompatible
inputs, and a legacy reservation. Progress configurations cover one run/two
calls/one crash, with and without lifecycle actions, and two runs/one observation/
one crash for session progress. The optional extended configuration has four
calls and two crashes, exercising repeated external calls in one run. Exact
bounds are in checks.json and each emitted configuration.

Safety checks require complete exploration of those finite state spaces.
AppendOnly is an action property checked across transitions. These are bounded
model checks, not a proof for arbitrary bounds. TLC also uses state fingerprints;
its collision estimate appears in the retained output. No symmetry or arbitrary
state constraint is used to hide failure paths.

The liveness specification adds weak fairness for recovery and continuously
enabled local work and cleanup, and strong fairness for writes/replies that can be
repeatedly aborted or lost. Remote completion is fair per run/call/attempt.
Together with the finite crash bound, these express eventual recovery and
eventual storage/provider availability. They do not guarantee progress through
permanent outages, revoked authority, incompatible inputs or an expired provider
deduplication window. Liveness does not assume another client submission.
Deadlock detection is disabled because completed, blocked and quiescent states
are valid; the progress properties check the work that should still advance.
The two-run progress profile supplies enough fuel and compatible artifacts:
an earlier blocked run intentionally prevents later runs in that session.

## Counterexamples and reachability

Deliberately broken variants must violate a specific invariant:

| Fault | Expected counterexample | Implementation obligation |
| --- | --- | --- |
| Acknowledge before terminal commit | CommitBarriers | SUB-1322 must not return durable success before confirmed terminal persistence. |
| Free the key when its response expires | AdmissionUnique | Preserve SUB-708 tombstones during SUB-1322 migration and retention. |
| Replace a replayed value after restart | ReplayMatches | SUB-1322 reuses each recorded random/UUID/time result and outcome. |
| Consume a native reply before its outcome commit | CommitBarriers | Never expose an uncommitted native outcome to the guest. |
| Skip commit authorization after cancellation | NoStaleWrites | SUB-1322 checks generation and lifecycle at the SQL commit boundary. |
| Put the startup generation in a provider key | StableEffectIdentity | SUB-1322 retains the original logical effect key across recovery. |

Negative checks select the named invariant so an earlier, different failure
does not mask the intended counterexample. They must return TLC's safety
violation exit code and a trace naming that invariant. Successful exploration
of a broken variant is a failure of this test.

Reachability checks use the correct protocol and deliberately assert
NeverWitness. Its counterexample must show the selected path: crash after
dispatch, remote success, outcome or terminal commit; retry while an old request
remains in flight; consumption of history present before the recovered attempt
launched; conflicting/expired retries; a refused
legacy reservation; or cancellation with a prepared write. A completed search
without that witness fails. These checks prevent passing by accidentally
excluding the scenario under discussion.
The outcome-crash witness requires an external outcome, distinct from a random,
UUID or time observation.

## Mapping to implementation

The interpreter's [native boundary](../../crates/interpreter/src/runtime/effects.rs)
records Rust values and typed ordinary errors before Wasm conversion. Its
[history implementation](../../crates/submilli-durable/src/history.rs) owns the
replay cursor and stable IDs. The server supplies asynchronous SQL through the
unit-of-work port.

| Model action | Implementation boundary | Fault coverage / remaining work |
| --- | --- | --- |
| Admit, Duplicate | Existing `PrepareRun` stores the immutable initial program with the optional admission key. | Existing admission/tombstone tests and recorded-run integration. |
| Claim, Launch, Restart | Internal `runner::durable` and `reopen_run` reconstruct the program under a new attempt. | Explicit reopen tested; autonomous scheduling and transport enablement remain SUB-1842. |
| Prepare, Commit, ForgetWrite | `append_history` inserts the outcome and advances position, bytes, logical fuel and run revision in one transaction, fenced by owner/attempt/state. | SQL rollback/reopen, stale attempt and mismatch tests. Lost acknowledgement is model-checked; SQL acknowledgement loss is not fault-injected. |
| Dispatch, RemoteComplete, Receive | `effects::execute` calls the backend only for missing history, passing the stable EffectId. | Missing outcomes retry regardless of adapter idempotency. No persistent intent. |
| Consume | Operation, codec version and argument fingerprint must match; native outcomes are decoded before guest conversion. | Saved observation and filesystem-resource replay, malformed data and memory bounds. |
| PrepareTerminal, Commit, Ack | `publish` verifies all history was consumed and uses `ResolveRun` for atomic terminal response and last-result publication. | Restart before terminal publication and stale-attempt refusal. Public handlers remain unchanged. |
| Cancel, Drain, Release, ExpireSession | Existing cancellation/drain rules remain; caller must drain an old attempt before reopening its mounts. | Full recovery/cleanup orchestration remains SUB-1842. |
| ExpireResponse, legacy initialization | Existing response retention preserves keys. Legacy runs without a manifest remain indeterminate. | Migration and legacy admission tests; no invented histories. |

`durable_outcomes` stores sealed, versioned inline native results, one row per
run/sequence. Initial arguments are never copied into these rows: only their
fingerprint is retained. Native result encoding is bounded to 64 MiB and total
run history to 1 GiB. Initial program encoding is limited to 16 MiB, decoded
literal collections to 128 MiB, and recorded blueprints to 16 regex operators
(across all permission filters). This conservative policy restriction bounds
compiler multiplicity separately; 128 MiB is not a compiled-regex heap claim.
The normal policy parser retains its per-pattern compilation limits. Other limits and available tenant memory can refuse earlier.
`execute` returns the native Rust value directly using Serde bounds. It validates
encoded size, lengths and nesting before decoding, but does not estimate or charge
the transient decoded value's heap through guest conversion. SQL/envelope buffers,
encoded payloads, ordinary provider allocations and guest allocations retain their
existing limits. This deliberately does not claim a total-memory bound for arbitrary
Serde deserializers. The native codec is version 2 and the runtime compatibility
marker is version 3. Older internal recordings are refused because resource encoding
and the set of recorded operations must match; console no longer occupies a history
position.

HTTP downloads retain metadata and the destination reference, never the downloaded
file body. Large-object storage is a later optimization.

The internal path requires a matching runtime compatibility marker, saved limits,
and unchanged package contents. Package pinning/retention is SUB-1881. Each outcome retains
logical execution time, excluding paused tool waits. Replay restores this spend
and has a separate reconstruction allowance equal to the original timeout.
Downtime does not consume execution time; an uncommitted tail can be retried. Current authority is checked on live calls. Native
fuel and LLM/embedding spend are restored on replay; guest computation and result
conversion run again against the original logical budget.

The native boundary covers UUID, random bytes, Math.random, Temporal.Now,
variables/secrets, filesystem operations and resources, HTTP/downloads, session
KV/finalizers, LLM/embedding, MCP/tools/agents/skills, explicit security checks,
and code/Git operations. Argument conversion and guest callbacks precede the
boundary; pure operations have no history row. Console logging also stays outside
the boundary: it writes normally on every attempt, including replay, and can
publish duplicate console events. Output is retained with the finished run;
incremental console persistence is deferred to SUB-1885.

Reader, writer and listing values all require a run ID and serialize identity/options
directly. Process-local handle state lives alongside these types in the filesystem
interface and is skipped during serialization. The default backend receives a run ID
but cannot restore resources without their local handles; recording requires a backend
that explicitly supports resource replay. Resources hold no filesystem or unit-of-work
reference; operations receive the resource through the active filesystem backend.
Deserialization itself performs no I/O. The default backend retains its existing
local handles; durable handle caching remains SUB-1883. Host return adapters own
Wasm result slots and translate ordinary errors at the host boundary. Result
conversion uses normal fuel checks after the native outcome is committed. If it
runs out of fuel, replay retrieves the saved outcome without dispatching the
operation again; conversion can succeed when the run has enough fuel. Ordinary
execution without recording uses the same conversion checks but has no saved
outcome to replay. Native work completed before the history commit still settles
its fuel costs.

Filesystem bytes, namespace changes and fsync are not modeled as atomic SQLite
effects here; SUB-1815 owns that protocol. The [filesystem models](../filesystem/README.md)
check recovery using replay-supplied input bytes and filesystem-owned evidence.
The abstract request includes preceding values to make replay divergence visible;
it specifies immutable argument identity, not a requirement to serialize those
input payloads again. Initial inputs are retained once and observations/outcomes
are recorded; filesystem intents contain fingerprints and recovery metadata.
Large-payload object placement is deferred and is not a prerequisite for SQLite
history or filesystem recovery. Formal composition with this execution model
remains separate work.
KV and other providers currently execute through the same native boundary; their
mutation and outcome commits are separate. Therefore the same missing-outcome
retry assumption applies. The framework does not claim exactly-once provider
mutation or atomic composition of those two transactions.

Keep this mapping current when protocol boundaries change. Update the model,
run affected checks and review the corresponding fault tests. Real SQLite
connection/process tests, filesystem tests and implementation review remain
necessary. Passing TLC does not verify Rust, SQL isolation, disk durability,
cryptography or complete interpreter determinism.

Later SUB-1798 models ownership,
leases and fencing, and SUB-1799
checks composition across owners. Human/admin approvals, durable harness
callbacks, extra-fuel grants and changed-code replay are separate work.

TLC command-line behavior and checking limitations are documented in the
[official TLC guide](https://docs.tlapl.us/using:tlc:start).

### Admission implementation

SUB-1836 adds run-backed preparation and compatibility admission for every
server program entry point. The [admission mapping](../admission/README.md)
describes its separate bounded model, migration, optional-key behavior and the
remaining gates before the complete recovery protocol can be enabled.
