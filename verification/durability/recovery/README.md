# Single-node run recovery

This model composes run admission, restart scheduling, effect replay, and session
lifetime. `DurableExecution` and the filesystem models verify the native-effect
protocols themselves. This model treats a retry-safe adapter's progress and its
outer recorded outcome as separate commits.

The bounds are three runs, two sessions, two simultaneous attempts, one restart,
two native outcomes per run, and one explicit resume per run. Runs 1 and 2 share
a session; run 3 can progress independently. SQLite has one exclusive process
owner, and filesystem writers do not overlap across sessions or external tools.
Multi-node leases, filesystem locks, application-tool waits, code changes, and
package retention are outside this model.

| Model action | Implementation boundary |
| --- | --- |
| `Admit` | `application::sessions::recovery::admit` commits the program and run identity |
| `Claim`, `Launch` | `recovery::claim` followed by supervisor task creation; rediscovery in the same startup is not a launch permit |
| `Execute`, `CommitOutcome`, `Consume` | Filesystem journal progress, immutable native outcome, and guest replay cursor |
| `Stick`, `Drain` | Interrupted execution retains resources; the runner drains native work before the supervisor records stuck |
| `Resume` | Explicit revision-checked transition from stuck to queued |
| `Complete` | Terminal response and session last-result reference committed together after consuming history |
| `Close` | Session closure fences attempts before resource cleanup drains live users |
| `Retire` | Terminal, drained work may release private files and history |
| `Crash`, `Restart` | Volatile ownership disappears; the new startup can reclaim queued/running work, never stuck work |

The safety invariants check single launch, explicit-only resume, session ordering,
retained resources, bounded adapter/history progress, and complete history before
terminal publication. Four deliberately broken variants must violate their
named invariants. The progress case is a **reachability witness**, not a liveness
proof: another session can complete while the first is stuck. This does not
prove real-world scheduling fairness, disk durability, implementation equivalence,
or recovery from arbitrary filesystem corruption.

Run with Java 17 or newer:

```sh
python3 verification/check.py --model durability/recovery
```

Rust tests exercise the persistence boundaries and process restarts separately.
The model's atomic commits assume SQLite's durable transaction contract; a lost
acknowledgement is reconciled by reading the committed identity/attempt rather
than dispatching a second copy.
