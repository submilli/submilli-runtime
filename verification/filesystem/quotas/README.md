# Public-file disk quota

The durable filesystem charges disk quota for public file contents. Candidate
files, staged writer output and retained recovery links are infrastructure costs.
They still consume fuel for their copying, hashing and other host work.

`QuotaAccounting.tla` checks the accounting boundary separately from the
filesystem persistence protocols. It models replacement (including writer
close), incremental append, remove, and same-volume move. Directories have no
file-content delta. A move temporarily exposes two names for one inode, counted
once. Preparing private output never changes usage.

An operation checks its eventual positive growth against the current counter
before modifying public files. Each successful native public change applies
only its actual size delta, before another fallible step or releasing the
mutation gate. A partial append is already public, so its written prefix counts
even when the attempt fails. A retry checks the current filesystem; it never
blindly reapplies a delta from the journal. The framework skips completed byte writes; other completed receipts replay
without accounting. A pending byte write publishes again and measures the
current destination, so repeating a published replacement applies zero delta. Writer chunks are private; quota admission happens at close.

The model separates the native mutation from its immediate counter update, so a
process or machine crash can occur in that gap. Fresh open reconstructs usage
from surviving public files, excluding recovery directories and deduplicating
inodes. Same-process reopen shares the existing counter and waits for outstanding
native work; it does not discard live reservations or rescan. The implementation
does not resume pending operations during open.

## Checks and assumptions

`PublicUsage` equates the counter with public contents whenever the gate could
admit further work. The intermediate accounting phase is exclusively held.
`QuotaAdmission` prevents positive growth beyond the configured limit while
allowing same-size replacement and shrinking an already-over-limit filesystem.
Liveness cases finish or refuse after bounded crashes under weak fairness.

Negative controls charge private output, replay a delta twice, skip the restart
measurement, or omit admission; each must produce the designated counterexample.
Witnesses require reachable free private output and a crash between mutation and
accounting followed by reopen.

Each instance represents one physically disjoint writable volume. Setup rejects
identical or ancestor/descendant backing roots; separate guest mount names alone
do not establish that separation. Server regression tests cover this prerequisite.

This bounded model assumes exclusive root ownership, serialized mutation,
drained cancelled workers, truthful native write lengths and atomic individual
syscalls. The separate byte-write, append and namespace models establish persistence
ordering; this model permits rollback or survival of unfinished changes and
does not re-prove that ordering. It does not model physical device capacity,
multi-node writers, arbitrary external file edits, CPU metering or memory limits.

## Implementation mapping

- `io.rs`: `Volume::check_growth`, `record_size_change`, initial public-file
  measurement and the shared quota registry.
- `filesystem/write_bytes.rs`: fresh-file retry, current destination size and
  immediate size delta; no journal or quota charge for temporary bytes.
- `filesystem/replacement.rs` (writer close): admission using destination size; apply the delta
  immediately after publishing the new file. Retained candidates stay free.
- `filesystem/append_bytes.rs`: fixed-offset retries; account only bytes that
  extend the current public file, including a partial write before an I/O error.
- `filesystem/remove_path.rs` and `move_path.rs`: release removed/replaced bytes
  only after the corresponding public mutation succeeds.
- Server process tests kill workers between the public syscall and accounting,
  reopen SQLite and the roots, and retry each original effect twice.

Run with the shared pinned runner:

```sh
python3 verification/check.py --model filesystem/quotas
```
