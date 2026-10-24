# Single-node run recovery

This model composes run admission, restart scheduling, effect replay and
session lifetime as the supervisor in `crates/submilli-server/src/recovery.rs`
implements them. `DurableExecution` and the filesystem models verify the
native-effect protocols themselves. Here a run's outer history and its adapter
progress are two counters with separate commits, and each run needs two native
outcomes to complete.

Invariants are predicates over the state variables. Each negative variant
disables one rule (`redispatch` two) and must violate its named invariant.
`ResumeIsExplicit` and `HistoryFenced` compare against ghost bookkeeping that
only the protocol's own actions write.

## State

| Variable | Kind | Implementation |
| --- | --- | --- |
| `state`, `attempt`, `owner` | SQLite | `runs.state`, `runs.attempt`, `runs.owner_generation` |
| `history`, `progress` | SQLite | `runs.history_position` with `durable_outcomes`; filesystem journal receipts |
| `closed`, `pinned` | SQLite | session status; `resources_retired = 0` |
| `boot`, `online`, `live`, `consumed` | process | the startup generation, whether the process is up, the supervisor's `active` tasks, each attempt's replay cursor |
| `resumes` | bound | one explicit resume per run; the implementation keeps no count |
| `launched`, `unresumed`, `frozen`, `progressBy` | ghost | whether the run's latest claim was launched, runs stuck and not resumed, history at cancellation, the attempt that made adapter progress |
| `witness` | witness latch | set by the path a witness case names |

`live` holds `<<run, attempt>>` pairs, so two attempts of one run are
expressible.

## Actions

| Action | Implementation boundary |
| --- | --- |
| `Admit` | `application::sessions::recovery::admit` commits the program and the queued run in session admission order |
| `Claim` | `recovery::claim` with `Run::claimable_by`: predecessor and session checks, then a new attempt owned by this startup. A run already running under this startup is never claimed again; one running under an earlier startup is reclaimed |
| `Launch` | the task spawned right after the claim commits takes the session's resource permit in `admit_resource_work` before any native work. A crash between claim and launch is the `claimed` process-kill checkpoint. An attempt is launched once; a task that ended is never started again |
| `MarkStuck` | `stop_attempt` after a task ends without completing, and the schedule-time rule for a running row owned by this startup with no task |
| `Execute`, `CommitOutcome`, `Consume` | native work, `append_history` fenced by attempt and running state, the guest's replay cursor |
| `Complete` | `ResolveRun::execute_recorded`: terminal response and session last result in one fenced transaction after the history was consumed |
| `Exit` | the task returns, for any reason, after draining its native work and releasing its permit |
| `Resume` | `recovery::resume`: revision-checked, the only path from stuck to queued |
| `Close` | `CloseSession` cancels queued, running and stuck runs and advances their attempt in the closing transaction |
| `Retire` | `recovery::retire` for a terminal run with no live task: the scheduler skips a session with an active task, and session cleanup waits for the resource-scope write lock before retiring |
| `Crash`, `Restart` | the process dies with its tasks; a new startup has a fresh generation and changes no run state |

`EarlierDone` mirrors `has_run_predecessor`: queued, running and stuck
predecessors block a claim. Indeterminate predecessors allow later work to
proceed. Any predecessor whose task has not drained still blocks the session.

## Invariants

| Invariant | Guarantee | Negative control |
| --- | --- | --- |
| `OneLiveAttemptPerRun` | one interpreter per run | `redispatch`: claim a running run again and launch it beside its live task |
| `NoSupersededLiveAttempt` | a live task is the current attempt unless the run was cancelled under it | `duplicate-claim`: claim a run already running under this startup |
| `LiveAttemptsBelongToThisBoot` | no task survives a restart, no task without a claim by this startup | `stale-launch`: launch a row claimed by an earlier startup |
| `StuckIsInert` | stuck is recorded only after the task is gone | `stuck-while-live`: `stop_attempt` without waiting for the task |
| `ResumeIsExplicit` | a run that entered stuck stays stuck or is cancelled until an explicit resume | `auto-resume`: a restart requeues stuck runs |
| `OrderedExecution` | a running or live run has only finished, drained predecessors in its session | `overtake`: claim without the predecessor check |
| `ResourcesProtected` | private files and history stay for retained or live runs | `early-retire`: retire while the task is live |
| `HistoryFenced` | nothing commits to history after cancellation | `stale-commit`: `append_history` without the attempt and state fence |
| `WithinCapacity`, `ProgressBounded`, `TerminalCommitted` | at most `Capacity` live tasks; adapter progress at most one effect ahead of history; completion needs the whole history | none; the `safety-capacity-one` case makes the capacity guard bind |
| `LegacyPreserved` | with `Legacy`, run 1 stays indeterminate with no attempts, live tasks or retained resources | none; the `legacy-successor-completed` witness shows run 2 can complete in the same session |

## Witnesses

Each witness case asserts `NoWitness` and must fail, proving the path exists:

| Witness | Path |
| --- | --- |
| `reclaim-after-crash` | a run claimed by startup 1 with committed history is reclaimed by startup 2 |
| `retry-commits-prior-progress` | adapter progress landed under one attempt and the outer outcome commits under a later one, the filesystem-receipt composition |
| `fenced-task-drains` | a cancelled run's task is still live after `Close` and drains |
| `unrelated-session-progress` | run 3 completes while run 1 is stuck |
| `legacy-successor-completed` | run 2 completes behind an indeterminate run 1 |

## Bounds and limits

Three runs in two sessions, two native outcomes per run, three claims per run
and one resume per run. The CI cases allow one crash and two live tasks, with
one case at capacity one so the capacity guard binds. The `extended` suite
allows two crashes and four claims per run. The `safety` and
`safety-capacity-one` cases each explore about 13 million distinct states in
about four minutes on one worker; the other CI cases finish in seconds. These
are bounded checks of the scheduler and lifecycle composition, not a liveness
proof: there is no fairness and no progress property, only the reachability
witnesses above.

Not modeled: the scheduler's page cursor and its reset on wakeup (SUB-2029),
sessions that share a writable named volume (SUB-2030), the filesystem
fingerprint check at launch and retirement (SUB-2028), the `durability_disabled`
transition from queued to stuck, lost commit acknowledgements inside one
transaction (`DurableExecution` and the unit-of-work model cover those),
multi-node ownership, and disk durability. A `MarkStuck` may fire between
`Claim` and `Launch`; the implementation reaches that state only through a lost
claim acknowledgement.

Run with Java 17 or newer:

```sh
python3 verification/check.py --model durability/recovery
python3 verification/check.py --model durability/recovery --suite extended
```

Rust tests exercise the persistence boundaries and process restarts
separately, including a claim that fails before it commits and one that
commits without a task.
