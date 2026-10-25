# Download admission and effect completion

SUB-1871 uses **Checked → effect committed**. The filesystem stores one immutable
admission record, binding the effect identity, destination, request fingerprint
and filesystem configuration after overwrite/no-clobber validation. The journal's
`done` index means admission is complete; it does not claim publication occurred.
There is no Published state, publication receipt, or saved response metadata before
rename. HTTP does not see the journal or native filesystem handles.

A missing effect outcome retries the HTTP request with fresh disposable staging,
even if the previous attempt finished transferring or published its file. Checked
admission skips the existence refusal on retry. Publication replaces the destination
atomically, synchronizes the file and both affected directories, then returns the
response metadata for effect-history commit. Retries may receive different bytes;
the recorded metadata must describe the response actually published by that attempt.
A committed outcome skips both network and filesystem work, preserving later changes.

`DownloadRecovery.tla` composes admission, filesystem publication, and framework
history. Each attempt number stands for a potentially different response. It includes
independent file/directory persistence, fresh inode creation, bounded crashes, history
commit, and later workspace mutations. A crash can retain both rename names; recovery
must unlink its private name before creating a fresh inode, never truncate an alias
of the public file. The model also covers cancellation abstractly as restart after
owned work drains. Rust tests separately cover disposable staging and worker ownership.

| Model action | Implementation boundary |
| --- | --- |
| Admit / Reject | `Filesystem::begin_download`; initial checks and immutable Checked commit. Lost acknowledgements reread this exact record. |
| CreateFresh / Write | Filesystem-owned `Download` writer; one bounded stage per serialized attempt, excluded from public quota. |
| SyncFile / Rename | Durable download publication; public quota checked before rename and accounted immediately after it. |
| SyncParent / SyncTempDirectory | Destination and private directory synchronization before publication returns. |
| SaveCompletion | `effects::execute` and `History::complete`; response metadata only. Lost acknowledgements reread authoritative history. |
| Restart | Same effect ID, fresh HTTP response unless history already committed. |
| LaterEffect | Later workspace mutation after completion; committed replay must preserve it. |

Checks include initial no-clobber refusal, overwrite admission, multiple crashes,
eventual completion, publication/history gaps and rename aliases. Negative variants
omit admission, repeat no-clobber on retry, reuse staging inodes, omit synchronization,
record mismatched response metadata, replay completed effects, or advance before
history commits.

Assumptions: one owner of writable roots; serialized effects; old workers drain
before retries; no later guest operation before effect completion. There is no claim
of exactly one HTTP request, identical uncommitted responses, multi-node fencing,
or protection from an external concurrent writer. Physical storage and SQLite
behavior still require implementation tests, not just TLC.

```sh
python3 verification/check.py --model filesystem/downloads
```
