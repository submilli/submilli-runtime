# Effect-history object publication

This bounded model covers SUB-1846's single database owner on local/block
storage, with an already persisted volume root. It models one run and history
position, two fresh object identities and at most one process crash. Byte length
is abstracted to a fixed value. It does not model cryptography, path validation,
allocation, SQLite internals, multi-node sharing or guest files.

| Model action/state | Implementation |
| --- | --- |
| BeginAppend / used | Validate the running attempt before writing; `used` tracks fresh names in the model, not SQL metadata |
| StartIO through SyncDirectory | Owned worker prepares session/run directories, writes staging, syncs file, renames and syncs directory |
| writer and worker | Complete append read guard and its clone retained by cancelled native work |
| Commit | SQL outcome reference and history position/bytes in one transaction |
| Acknowledge | Caller observes commit success |
| Read / CloseRead | Read guard retained through authenticated object decoding |
| Terminate | Existing run state prevents new appends and recovery attempts |
| Delete / SyncDelete | Exclusive removal of the run directory and empty session directory, followed by directory barriers |
| deleting | Exclusive cleanup operation, not persisted metadata |
| Retire | Delete outcomes and mark existing run resources retired in one SQL transaction |
| Crash | SQL and synced names survive; unsynced publication can disappear and unsynced deletion can reappear |

The safety check requires recoverable references to have durable bytes, atomic
history accounting, protection of active workers and readers, and no history or
files after retirement. A terminal run can lose its files before its outcomes are
removed: terminal runs are never replayed. Uncommitted files remain with their
owning run until terminal retirement. The liveness check requires eventual
retirement after the terminal run's writers finish, under weak fairness for reader
release and successful deletion/sync. Permanent I/O failure is outside that
progress assumption. Failed cleanup leaves the run's existing retirement flag
unset, so recovery and session cleanup retry it.

Four deliberately broken variants must violate their named safety invariant.
Three reachability checks witness cancellation with a live worker, replay of a
commit whose acknowledgement was lost before a crash, and orphan cleanup after
a crash. Reachability checks alone are not safety proofs.

Run `python3 verification/check.py --model history-objects`. CI runs the same
manifest and retains configurations, traces and summaries as artifacts. Rust
`adapters::history::tests` additionally kills a subprocess at the implementation's
publication/commit/retirement boundaries. Those tests observe process crashes,
not loss of the operating system's page cache. The model explores loss of
unsynced publication and resurrection of unsynced deletion. Real disk/controller
power-loss behavior still depends on honoring file and directory sync.
