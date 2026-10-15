# Logical filesystem resources

This bounded composition models one reader, one recorded read, a durable append
to its original inode, a durable pathname replacement, restart and terminal
cleanup. Append and replacement are supplied by their separate primitive models.
The reader's private inode retention must be synced before recording its open.
Native descriptor numbers never appear in the history contract.

A physical read produces a candidate result. Committing the read observation and
its logical cursor is one transaction in execution history. Recovery reconstructs
the original inode at the recorded byte position. Replaying an observation returns
its recorded value, even after the path has been replaced. Reading *new* data
from the restored original inode still observes an append to that inode. Buffered
line readers additionally preserve decoding flags and the first unread byte;
using the native descriptor's prefetched offset is invalid.

The filesystem API supplies retained readers and explicit positions. The later
observation-replay issue owns committing observations/cursors and returning
recorded guest results; this model states that integration obligation rather
than claiming the filesystem API alone makes interpreter handles replayable.
An immutable directory listing uses the same observation/cursor rule without
a live directory descriptor. Buffered writers use retained immutable chunks;
close is a separately identified publication effect.

Run `python3 verification/check.py --model filesystem/resources`. Negative
variants omit pin durability, restore from the current path, reset the cursor,
read fresh bytes during replay, or collect before terminal draining. The witness
requires crash after replacement, restoration and replay. Single-run exclusion,
atomic SQL history and correct immutable-object retention are assumptions. No
multi-node owner takeover or general lock reacquisition is proved here.
