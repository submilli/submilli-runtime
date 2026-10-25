# Captured file updates

`UpdateRecovery.tla` models SUB-1857's portable replacement protocol, including
its composition with execution history. Run the CI matrix with:

```sh
python3 verification/check.py --model filesystem/updates
```

The native text transformation runs only while no preparation is durable.
It produces final bytes and the native result. The filesystem writes those bytes
once to a private candidate, preserves permissions, syncs the file and its
private directory, and commits the prepared identity together with the encrypted
native result. It then atomically publishes the candidate, syncs the affected
directories, commits a receipt, and returns the saved result to execution history.
Only after history commits may Wasm receive the result.

The model distinguishes volatile and durable file state. Crashes may occur at
every transition, including immediately after SQL commits whose acknowledgement
was lost. Cancellation can leave an owned worker running; restart must drain it.
Completed effects return their saved output without touching later file contents.
Noops and rejected text matches bypass candidate publication and use outer
history recording directly.

| Model action | Implementation |
| --- | --- |
| Compute | `stdlib/code/mod.rs`: edit, insert_at, apply_patch |
| Stage / SyncFile / SyncPrivateDirectory | `filesystem/replacement.rs`: prepare_file_update |
| Capture | `filesystem/update_file.rs`: candidate plus result transaction |
| Publish / SyncPublicDirectory | `filesystem/replacement.rs`: publish_file_update |
| Receipt | `filesystem/update_file.rs`: UpdatePublication::complete |
| History / Deliver | interpreter `runtime/effects.rs` |
| Cancel / Restart | owned filesystem gate, blocking worker, finish_owned |

Safety checks cover captured-byte/result stability, atomic publication, durable
completion, delivery ordering, request identity, and preservation of later writes.
The matrix includes edit, insert, patch, noop, and rejected outcomes; repeated
crashes; progress under weak fairness; and reachable publication, history, and
cancellation gaps. Deliberately broken variants omit each sync barrier, lose the
result, recompute after capture, ignore identity, repeat a completed effect, or
expose a result before history. Each must produce its specified counterexample.

This is a finite protocol check, not a proof of Rust, storage hardware, the text
algorithms, or permission/quota implementation. Assumptions: one filesystem owner,
atomic same-volume rename, successful sync barriers persist their scope, atomic
SQLite transactions, stable request identity, and eventual successful recovery.
It does not claim HA coordination or survival of filesystem corruption. Native
checks exercise process kills, saved-result replay, metadata, missing/corrupt
recovery data, and the actual host APIs. Corrupt/missing prepared candidates fail
closed; recovery never reconstructs them from the public file.

Linux reflinks/range-copy optimization is tracked in SUB-2038. Lower-memory text
transformation/diff algorithms are tracked in SUB-2039. Neither changes this
protocol; macOS-specific optimization is outside the planned scope.
