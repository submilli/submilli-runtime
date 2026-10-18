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

Start with `src/filesystem/write_bytes.rs`. Each journaled operation has its own
file, state enum, execution loop, and state encoding/decoding. Mutation loops
advance `Initial -> Prepared -> Completed`, with terminal preflight refusals
also represented by `Completed`. Each arm performs its native work, then the
operation saves the next state before continuing. A retry decodes the latest
journal entry for the same run/effect and enters that state again.

`encode_state` maps those variants onto the existing SQL intent/receipt columns;
this refactor does not change the database format. Prepared evidence and completed
results remain immutable. `operation.rs` contains only shared per-call accounting
and journal admission/commit. Shared evidence primitives stay in `filesystem.rs`;
operation-specific steps are inline in their state arms.

Resource calls have their own state machines too. A chunk advances through
`Initial -> Staged -> Completed`; only its fingerprint receipt is persisted.
Reader/listing next calls use `ReadAt -> SaveProgress -> Completed`. They encode
only effect ID and before/after cursor progress, not the returned bytes. Retrying
the same effect decodes its before position and reconstructs the result; the next
effect starts at after. Explicit close has its own loop. File metadata and direct
reads remain observations; their result recording belongs to execution history.

## Interpreter boundary

Embedders set `StoreData.filesystem` to inject the selected backend, using the
same workspace as `StoreData.vfs`. Filesystem host functions retain capability
checks, caller attribution, heap limits and fuel
accounting. Native tree operations receive a budget interface for incremental
work and settlement after the first effect. Readers, writers and directory
iterators hold backend resources. Only explicit writer close publishes output.

Durable mutation and resource calls require canonical UUID operation identities
from the caller. Reusing an identity with different arguments fails. A trace
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
work. A fresh `open` requires exclusive ownership of the roots.
The caller retains the same roots, journal, run ID and effect IDs across
retries. Every journal record binds its request to a fingerprint of the configured
roots, mounts, working directory, access modes and quotas. A changed filesystem configuration fingerprint
fails rather than replaying a result against different paths. Supplying an empty
or unrelated journal is a caller error; there is no separate database-identity
registry to detect it.

The code determines the sequence. There is no persisted compound plan or parent
outcome. A caller retries primitives in the same order with their original IDs:
completed operations return their receipts, and an interrupted operation
reconciles its own intent. Filesystem receipts are independent of future execution
history. If execution history has not yet recorded a completed filesystem call,
retrying that call still returns its filesystem receipt without repeating it.
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
