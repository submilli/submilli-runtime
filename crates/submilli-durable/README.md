# Durable filesystem services

The interpreter defines its filesystem traits in `runtime/filesystem.rs` and
supplies `DefaultFilesystem` in `runtime/default_filesystem.rs`, including its
reader, writer and directory implementations. This crate implements the same
interface and owns immutable
payload publication, filesystem intents, receipts, resource retention, recovery,
and cleanup. It has no dependency on the server or SQLx.

The server supplies the `UnitOfWork` implementation through a `UnitFactory`
callback. A unit reads its own writes, holds transaction exclusion, and commits
all staged records together. Dropping it rolls back. Callers pin payloads in the
same unit that admits their effect or compound plan. An uncertain commit is
resolved by opening a fresh unit and looking up the same immutable identity.
No unit remains open during filesystem mutation or payload publication.

The SQLite adapter maps typed records to existing tables. Serialization and
associated-data formats belong here. The object and filesystem record formats
are unchanged. Native paths, file descriptors and database connections are not
persistence contracts.

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
calls; a fixed invocation is useful only for an isolated operation and its retry.
Ordinary server and CLI runs continue to use the native backend.

The durable port supports captured writes/appends, primitive directory creation,
remove/move, retained readers, directory snapshots and chunked writers. Recursive
mutations and copies require an explicitly captured `execute_plan`; the generic
port refuses them before mutation.

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

The existing filesystem, object and shared-effect TLA+ models still specify the
protocol. `verification/unit-of-work` checks the new transaction boundary,
including staged reads, rollback and a lost commit acknowledgement. It is a
bounded model of the contract, not a proof of Rust or SQLite. The interpreter's
native filesystem tests, durable protocol tests, in-memory unit contract tests,
SQLite integration tests and process-kill tests check the implementation.
The original single-run/exclusive-filesystem assumption remains in force.
