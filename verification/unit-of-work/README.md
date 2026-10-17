# Durable unit of work

`submilli-durable` owns the persistence contract and protocol decisions. The
server supplies SQLite units through a factory callback. Each unit uses the
same cancellable transaction driver as session application units. Reads see
staged writes, a consuming commit makes them authoritative, and dropping the
unit discards uncommitted changes.

This model refines the atomic admission assumption of the filesystem and shared
effect models. A transaction stages an effect identity and its argument fingerprint
separately; the protocol commits only after both have been staged. It models
reads within that transaction, drop, process crash, and recovery after a commit
whose acknowledgement was lost. The adapter must provide serializable exclusion
for conflicting writes; the current SQLite transaction driver supplies it.
Multi-node transaction concurrency, leases and fencing are outside this model.

| Model action | Implementation |
| --- | --- |
| Begin | `UnitFactory` and `SqliteTransaction::begin` |
| WriteIntent / Fingerprint | `FilesystemJournal::admit_step` and `save_effect`; both metadata fields are stored in one effect row |
| Read | Direct unit operations, using the held transaction connection |
| Commit | Consuming `UnitOfWork::commit` |
| Drop / Crash | Drop the driver; SQLite transaction rollback; retained filesystem artifacts remain governed by the filesystem models |
| Return after lost acknowledgement | Open a fresh unit and reconcile the same effect identity and immutable request |

Checks cover atomic effect identity plus fingerprint, read-your-writes, rollback, and
commit-before-return. Four deliberately broken variants must violate those
properties. Three witnesses ensure that staged reads, rollback with pending
writes, and reconciliation after a lost acknowledgement are reachable.
This bounded model does not prove the Rust code, SQLite, or storage hardware.
The existing filesystem models still govern prepare → filesystem persistence
barriers → outcome commit. No database transaction spans public filesystem
mutation.

Run with `python3 verification/check.py --model unit-of-work`. SQL contract and
crash tests exercise the implementation separately. Migration 10 stores filesystem metadata and outputs without object tables.
No input payload bytes are required by this transaction model.
