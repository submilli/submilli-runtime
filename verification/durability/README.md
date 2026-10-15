# Single-node durable execution protocol

SUB-1797 checks the proposed
single-node protocol before its implementation in
SUB-1322. The model is executable
with TLC. It does not change the runtime or enable durable execution.

An accepted run belongs to one persistent logical node. Its code/input identity,
admission key, history and budgets survive process restart. Recovery reruns the
same program, supplies recorded observations and outcomes, then continues live.
For an external call, the order is:

    commit intent -> dispatch -> provider completes -> commit outcome -> consume
                                                                  |
                                            replay returns this outcome

The provider can complete while our process is down. Retrying the same effect
ID and immutable arguments resolves the same logical operation and outcome.
That is an explicit provider assumption. Filesystem operations must establish
it separately in SUB-1815, with
implementation and real crash tests in
SUB-1339.

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

Each fresh observation or external intent charges one logical unit. Replay and
retry do not grant a new budget or charge that effect again. A short budget
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
| CommitBarriers, TerminalConsistent | Intent before dispatch, outcome before consumption, terminal result before acknowledgement, and atomic publication of the session's last result. |
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
| Skip commit authorization after cancellation | NoStaleWrites | SUB-1322 checks generation and lifecycle at the SQL commit boundary. |
| Put the startup generation in a provider key | NoDuplicateMutation | SUB-1322 retains the original logical effect key across recovery. |

Negative checks select the named invariant so an earlier, different failure
does not mask the intended counterexample. They must return TLC's safety
violation exit code and a trace naming that invariant. Successful exploration
of a broken variant is a failure of this test.

Reachability checks use the correct protocol and deliberately assert
NeverWitness. Its counterexample must show the selected path: crash after
intent, remote success, outcome or terminal commit; retry while an old request
remains in flight; consumption of history present before the recovered attempt
launched; conflicting/expired retries; a refused
legacy reservation; or cancellation with a prepared write. A completed search
without that witness fails. These checks prevent passing by accidentally
excluding the scenario under discussion.
The outcome-crash witness requires an external outcome, distinct from a random,
UUID or time observation.

## Mapping to implementation

These are proposed boundaries, not existing durable-run code. The shipped
[idempotency use cases](../../crates/submilli-server/src/application/idempotency.rs)
and [unit of work](../../crates/submilli-server/src/application/unit_of_work.rs)
provide the current admission/transaction foundation. The existing recovery
marks older unfinished reservations indeterminate; it does not replay programs.

| Model action | Planned application/storage boundary | Required implementation fault test |
| --- | --- | --- |
| Admit, Duplicate | SUB-1322 unifies admission, immutable manifest and keyed run identity in one transaction. | Concurrent same/different fingerprint requests; lost admission commit acknowledgement; SUB-708 tombstones. |
| Claim, Launch, Restart | SUB-1322 supervisor claims local-origin runs using startup/attempt generations and persisted session order. | Restart with no client; crash after claim before launch; a second process must not acquire the store. |
| Prepare, Commit, ForgetWrite | SUB-1322 commits an event, sequence and logical accounting with lifecycle/generation guards in one unit of work. | Abort and lost acknowledgement at each history boundary; cancellation races a queued commit. |
| Dispatch, RemoteComplete, Receive | SUB-1322 effect adapter uses the durable intent's original key and immutable request. | Provider succeeds without delivering a reply; old and recovery requests overlap; duplicate replies. |
| Consume | SUB-1322 validates replay position/arguments and reconstructs guest values from committed outcomes. | Crash after recording a random/UUID/time value or external outcome but before guest consumption. |
| PrepareTerminal, Commit, Ack | SUB-1322/SUB-1331 atomically commit terminal evidence, exact response and last-result reference. | Crash or lost acknowledgement after completion; repeated observation returns the original response. |
| Cancel, Drain, Release, ExpireSession | SUB-1322/SUB-1432 coordinate cancellation, resource pins and session cleanup. | Restart after session idle expiry with recoverable work; cleanup races in-flight work. |
| ExpireResponse, legacy initialization | SUB-1322 preserves SUB-708 retention and legacy indeterminate keys. | Expiry never enables another execution; absent legacy manifests are never synthesized. |

Filesystem bytes, namespace changes and fsync are not modeled as atomic SQLite
effects here; SUB-1815 owns that protocol. Its independently runnable
[immutable-object publication model](../objects/README.md) checks the storage
contract for large payloads and captured downloads. Mutable workspace operations
and a formal composition with this execution model remain separate work.
Atomic database-local effects such as
KV need the same event/sequence transaction contract, but their concrete mutation
semantics and schema remain implementation work in SUB-989/SUB-1322.

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
