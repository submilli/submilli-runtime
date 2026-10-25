# Effect-history object publication

This bounded model covers SUB-1846's single database owner on local/block
storage. It models one history position, two fresh object identities and at most
one process crash. It assumes root initialization has completed its persisted
directory barriers. Failed initialization retries are covered by the Rust tests.
Byte length is abstracted to a fixed value. It does not model
cryptography, allocation, SQLite internals, multi-node sharing or guest files.

| Model action/state | Implementation |
| --- | --- |
| Reserve / pending row | Fenced SQL publication identity before file creation |
| StartIO through SyncDirectory | Owned worker writes staging, syncs file, renames and syncs directory |
| writer and worker | Complete append read guard and its clone retained by cancelled native work |
| Commit | SQL outcome reference, history position/bytes and pending-to-bound transition in one transaction |
| Acknowledge | Caller observes commit success |
| Read / CloseRead | Read guard retained through authenticated object decoding |
| Retire | Terminal run loses outcome references and marks objects deleting |
| Claim / Delete / SyncDelete | Exclusive collection, unlink, directory sync, then row deletion |
| Crash | SQL and synced names survive; unsynced publication can disappear and unsynced deletion can reappear |

The safety check requires committed references to have durable bytes, atomic
history accounting, protection of active workers and readers, and no history
after retirement. The liveness check requires eventual deletion after retirement
under weak fairness for reader release and successful deletion/sync. Permanent
I/O failure is outside that progress assumption. The runtime retries failures
without dropping their SQL rows.

Four deliberately broken variants must violate their named safety invariant.
Three reachability checks witness cancellation with a live worker, replay of a
commit whose acknowledgement was lost before a crash, and orphan collection
after a crash. Reachability checks alone are not safety proofs.

Run `python3 verification/check.py --model history-objects`. CI runs the same
manifest and retains configurations, traces and summaries as artifacts. Rust
`adapters::history::tests` additionally kills a subprocess at the implementation's
publication/commit/collection boundaries. Those tests observe process crashes,
not loss of the operating system's page cache. The model explores loss of
unsynced publication and resurrection of unsynced deletion. Real disk/controller
power-loss behavior still depends on honoring file and directory sync.
