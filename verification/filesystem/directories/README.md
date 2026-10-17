# Empty-directory recovery

This model covers one admitted `mkdir` or empty `rmdir`, under exclusive access
until its SQL receipt commits. A new directory has no children until that
receipt. If a crash loses an unacknowledged `mkdir`, recovery creates it again;
its physical inode number need not survive that failed attempt. If it exists,
recovery checks that it is a plain empty directory. `rmdir` records the original
identity and accepts only that identity or absence. Neither operation follows
a final symlink. Existing-directory `mkdir` is an observation of that identity.

File contents are absent in this model. It separates the visible name, persisted
name, new-directory synchronization, parent synchronization and SQL receipt.
The initial parent/namespace are durable. The filesystem must honor successful
directory synchronization; errors cannot be ignored. A pending I/O error stays
pending, rather than becoming a definitive failed outcome after a mutation.
Completed calls replay from SQL even when a later operation reused the path.

No directory handles, concurrent writers or tree rename are proved here. A
recursive operation follows the caller code and each child receipt, using
these empty-directory operations and the separately verified file primitives.
Progress assumes eventual successful storage operations and finite crashes.

Run `python3 verification/check.py --model filesystem/directories`. Negative
variants expose early receipts, stale path retries and uncertain errors. The
witnesses require publication before receipt, a lost receipt acknowledgement,
and replay after path reuse. Rust tests must additionally check identity/type
validation, nonempty targets, symlinks, sync failures and process restart.
