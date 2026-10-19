# Durable filesystem services

The interpreter defines its filesystem traits in `runtime/filesystem.rs` and
supplies `DefaultFilesystem` in `runtime/default_filesystem.rs`, including its
reader, writer and directory implementations. This crate implements the same
interface and owns filesystem intents, receipts, resource retention, retry
reconciliation. It has no dependency on the server or SQLx.

The server supplies the `UnitOfWork` implementation through a `UnitFactory`
callback. A unit reads its own writes, holds transaction exclusion, and commits
all staged records together. Dropping it rolls back. Input bytes are never
written to the journal; replay supplies them again. An uncertain commit is
resolved by opening a fresh unit and looking up the same immutable identity.
No unit remains open during filesystem mutation.

The SQLite adapter maps typed records to filesystem-specific tables. Serialization and
associated-data formats belong here. The journal stores fingerprints and recovery evidence, with recorded
outputs inline in SQLite. Native paths, file descriptors and database connections are not
persistence contracts.

## Reading an operation

Start with `src/filesystem/write_bytes.rs`: it creates a fresh temporary file at
a deterministic run/effect path, writes and syncs it, renames it over the target,
updates public disk usage and syncs both directories. It has no journal, state
enum or input fingerprint. Retrying an unfinished effect repeats those steps
with identical bytes. The execution framework must skip completed effects,
keep arguments and configuration unchanged, and durably record completion before
starting another effect. A direct call always writes, even with an old effect ID.
The framework integration is future work; the adapter does not enforce that ordering.

Next read `append_bytes.rs`. Each journaled operation has its own file, state enum, execution loop, and state encoding/decoding. Mutation loops
advance `Initial -> Prepared -> Completed`, with terminal preflight refusals
also represented by `Completed`. Each arm performs its native work, then the
operation saves the next state before continuing. A retry decodes the latest
journal entry for the same run/effect and enters that state again.

Each operation's `encode_state` serializes its enum into one optional `state`
payload. `None` decodes as `Initial`; malformed saved bytes fail. `Prepared`
contains only facts that cannot be recovered from the arguments and filesystem,
and `Completed` replaces it with the result. There are no
separate recovery/outcome payloads or rejection flag. A derived SQL `status`
maintains the one-pending-operation index; reads verify it matches the decoded
state. Journal format version 4 rejects previous unmerged formats. Prepared
facts cannot change while pending, and completed states remain immutable. `operation.rs` contains only shared per-call accounting
and journal admission/commit. Writer-close replacement helpers live in `filesystem/replacement.rs`;
operation-specific disk steps stay alongside their state machines. Workers
apply the public size delta immediately after each successful native mutation.
Only fresh filesystem setup measures public usage; private output is uncharged.

Resource calls have their own state machines too. A chunk advances through
`Initial -> Staged -> Completed`; only its fingerprint receipt is persisted.
Reader/listing next calls use `ReadAt -> SaveProgress -> Completed`. They encode
only effect ID and before/after cursor progress, not the returned bytes. Retrying
the same effect decodes its before position and reconstructs the result; the next
effect starts at after. Explicit close has its own loop. File metadata and direct
reads remain observations; their result recording belongs to execution history.

Mutation preparation retains only what retry needs. Append uses `Files::size`
to obtain and synchronize the original length, saves `Prepared { offset }`, and
rewrites the same range on retry; a missing file starts at zero. Other mutations
save `Prepared {}` with no fields. Paths come from the replayed call or writer
row, and each pending attempt resolves them again. Writer close retains its new
candidate at a deterministic effect-derived location, never a backup/hash of the
old destination; its journal stores no candidate inode. Remove infers the type
from an existing target and accepts absence after preparation. Move keeps its
source name until the destination is durable, then removes the source. Later operations
can reuse paths after completion, so every completed journaled retry still returns its
receipt without touching those paths.

## Interpreter boundary

Embedders set `StoreData.filesystem` to inject the selected backend, using the
same workspace as `StoreData.vfs`. Filesystem host functions retain capability
checks, caller attribution, heap limits and fuel
accounting. Native tree operations receive a budget interface for incremental
work and settlement after the first effect. Readers, writers and directory
iterators hold backend resources. Only explicit writer close publishes output.

Durable mutation and resource calls require canonical UUID operation identities
from the caller. Journaled calls reject identity reuse with different arguments;
byte writes rely on the framework to validate their identity and input. A trace
identifier is not an operation identity. The embedding runtime still needs the
execution-history sequencer before it can assign identities to successive guest
calls; a fixed effect ID is useful only for an isolated operation and its retry.
Ordinary server and CLI runs continue to use the native backend.

This component does not discover pending work or run recovery at server startup.
`open(vfs, run_id, units)` takes the caller's configured filesystem,
including its mounts, and a stable UUID `RunId`. Each logical call takes an
`EffectId`; reader and writer handles use their opening effect's ID. It creates
private recovery artifacts within those roots, not a new public run directory.
`DurableFilesystem` owns its mutation gate and shared quota registry. Use its
`reopen(vfs, run_id, units)` method when existing handles or cancelled workers may
still access the roots: it shares their coordination and waits for outstanding
work. A fresh `open` requires exclusive ownership of the roots. Writable backing roots
must be physically disjoint: setup rejects aliases and ancestor/descendant roots
so one file cannot participate in two independent quota counters. No competing call
may modify a pending operation's paths, including during restart. SUB-1295 will
enforce this contract with separately verified locks.
Disk quota counts public file contents only. Replacement candidates, writer
staging and retained recovery links are free of disk-quota charges; their host
work still costs fuel. A write checks the final size change against the limit
and applies that change when it publishes. Writer chunks are admitted to private
staging without quota charges; close checks the eventual destination size.
Partial public appends count their actual written bytes. Retries apply only
changes they make, so neither lost acknowledgements nor repeated receipts charge
twice. Fresh open measures public files once, excluding recovery artifacts;
same-process reopen keeps the live counter. No operation rescans volumes for quota.
The [quota model](../../verification/filesystem/quotas/README.md) checks these
accounting boundaries and crash gaps.

The caller retains the same roots, journal, run ID and effect IDs across
retries. Every journal record binds its request to a fingerprint of the configured
roots, mounts, working directory, access modes and quotas. A changed filesystem configuration fingerprint
fails rather than replaying a result against different paths. Supplying an empty
or unrelated journal is a caller error; there is no separate database-identity
registry to detect it.

The code determines the sequence. There is no persisted compound plan or parent
outcome. A caller retries primitives in the same order with their original IDs:
journaled operations return completed receipts or reconcile their pending intent.
Byte writes instead repeat from scratch until the framework records completion.
The framework must skip them thereafter, preserving later effects. Other
filesystem receipts are independent of future execution history: retrying those
calls reuses the receipt even before outer completion is recorded.
The later execution layer owns replay, retry scheduling and run completion.

The port supports replay-supplied writes/appends, primitive directory creation,
remove/move, effect-aware readers, directory snapshots and chunked writers. Recursive
mutations and copies require caller orchestration of primitives; the generic
port refuses them before mutation. The caller also owns retention and cleanup:
keep recovery artifacts and receipts while retries or retained resources remain
possible, and drain native workers and handles before disposing of the roots.
There is no run-state lookup, automatic retirement or cleanup sweep here.

Code tools, Git and HTTP downloads retain their existing direct access through
`StoreData.vfs`. Their operations remain available, but are outside this crate's
durability guarantees: they do not record or reconcile effects through the
filesystem backend. Their integration is tracked in
[SUB-1857](https://linear.app/submilli/issue/SUB-1857),
[SUB-1870](https://linear.app/submilli/issue/SUB-1870) and
[SUB-1871](https://linear.app/submilli/issue/SUB-1871), respectively. Read-result
replay, execution supervision and restart of guest programs also remain separate
work. The embedding runtime must drain native work before filesystem recovery.

## Verification

The filesystem and operation-journal TLA+ models specify the protocol, including
volatile replay inputs and staged writer outputs. `verification/unit-of-work` checks the new transaction boundary,
including staged reads, rollback and a lost commit acknowledgement. It is a
bounded model of the contract, not a proof of Rust or SQLite. The interpreter's
native filesystem tests, durable protocol tests, in-memory unit contract tests,
SQLite integration tests and process-kill tests check the implementation.
The original single-run/exclusive-filesystem assumption remains in force.

Readers and listings checkpoint their last effect ID and before/after logical
position before returning an output. The framework records result bytes in its
own history. Handles expose a serializable resource descriptor and restore by
ID without eager I/O. Reader calls validate the configured pathname; they do not
pin old inodes. Failed reads leave progress unchanged, and finalization never
deletes rows needed for resume. Writers restore their ID without resetting staged
output and honor SQL close outcomes, including definitive refusals.

See [the resource protocol and test mapping](../../verification/filesystem/resources/README.md).
The embedding runtime must provide replay-stable effect IDs, skip completed
calls, serialize dependent dispatch and retain unchanged inputs while a read is
pending. The interpreter currently supplies temporary per-call IDs; production
history, restart supervision and durable retirement remain future integration.

Directory opens reserve a conservative 32 MiB plus 8 KiB of tenant working memory
before loading or enumerating their bounded 4 MiB snapshot. The reservation is
refunded after publication. Owned listing operations retain their reservation
and serialization gate until native and SQL work finishes, even if their caller
is cancelled. Restored iterator steps reserve from the actual encoded
snapshot size. Small tenant budgets can therefore refuse a directory open even
when its eventual snapshot would be small; incremental admission is not yet used.
