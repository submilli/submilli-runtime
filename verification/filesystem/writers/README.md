# Staged writer output

`WriterRecovery.tla` checks one writer with two chunks and a bounded crash.
The private file is output-in-progress. SQLite holds chunk IDs/fingerprints and
close state; it holds no copy of input bytes. Replay supplies each call's bytes
from earlier recorded execution outputs, abstracted by `Payload`.

`Stage` writes at the acknowledged prefix length, replacing an unacknowledged
tail. `Sync` persists that output. Only then can `Receipt` record a chunk.
Duplicates validate the fingerprint and skip writing; mismatches are refused.
Close composes the independently checked namespace-publication protocol. Its
abstract atomic transition represents completed file/directory barriers plus
receipt, not a claim that rename and SQL are one transaction. Completed close
replay leaves a reused pathname untouched.

Safety checks cover acknowledged output durability, exact bytes, bounded tails,
and no republication. Negative variants bypass synchronization, argument
validation, fixed offsets and completed-close replay. Witnesses require recovery
of an unacknowledged tail and refusal of mismatched replay. Liveness assumes
correct replay eventually supplies bytes, finite crashes and fair storage work;
repeated incompatible replay attempts have no progress guarantee.

Rust tests cover a process killed after staging but before the chunk receipt,
close publication boundaries, wrong duplicate bytes, corrupt staging, and a
completed close followed by pathname reuse. No object storage is involved.

Run `python3 verification/check.py --model filesystem/writers`.
