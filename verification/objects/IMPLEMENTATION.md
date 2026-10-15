# Local immutable object storage

SUB-1846 implements the object-publication protocol in `submilli-server`.
It supplies storage for later history, download and terminal-result adapters;
it does not enable interpreter replay or mutable filesystem recovery.

The public embedding API is `objects::DurableObjects`. The server can own it
through `ServerConfig::durable_objects`, or this YAML configuration:

```yaml
durable_objects:
  directory: /data/objects
  inline_bytes: 65536
  maximum_object_bytes: 4294967296
  maximum_owner_bytes: 8589934592
  maximum_store_bytes: 68719476736
```

The feature is opt-in. The directory must already exist, including when using
the default `<server-root>/objects` path. A configured missing mount fails
startup. Server startup also requires persistent SQLite and the secret-store
encryption key. The private object directory cannot overlap guest volumes.
Its process lock survives cancelled callers until all owned I/O finishes.
A synchronized private identity file pairs the directory with one database;
opening a different directory against that database fails. A directory holding
payloads cannot be adopted by a replacement empty database. Initialization
publishes the identity before committing its SQL counterpart; a rollback can
only leave an empty store to adopt with a freshly minted identity. Adoption
invalidates any previous database's pairing, including a database retaining
only collected-object tombstones. Prepared acknowledgements also carry an
in-process issuer identity, and binding verifies the consumer transaction's
durable store identity.

## Publication and consumption

Reserve an opaque ID and immutable request fingerprint before supplying bytes.
Retries reuse that ID and the original request. `publish` returns a
`PreparedObject` only after file and directory persistence barriers. A prepared
object is still pending: it is not an authoritative consumer reference.

The application `ObjectPublications::bind_in` operation participates in the
consumer's existing unit of work. History must bind the object in the same
transaction as its event, sequence and accounting. The standalone `bind` API
commits an object-row reference for consumers which use that row directly.
Do not use standalone binding followed by a separate history insert as a
replacement for the history transaction.
`DurableObjects::bind_payload_in` supplies the same boundary for inline/object
payloads used by server-side consumers.

Payload preparation buffers at most the configured threshold plus one byte
before choosing inline storage or encrypted object staging. The threshold is
serialized plaintext bytes; equality stays inline. File downloads must select
`always_object`, even for an empty response. Inline values are sealed before
their representation can be serialized. Their consumer still owns transaction
ordering, inline-data quotas and retention. Object quotas count the sealed
representation, reserve the configured transfer maximum before writing, and
shrink to the actual size on binding. Retired objects stay charged until
collection is complete.

Objects use 64 KiB authenticated frames and an authenticated completion frame.
Associated data binds the object ID, envelope instance, frame index and length.
The completion frame authenticates the length, chunk count and plaintext hash.
An invalid pending staging envelope is discarded and regenerated with fresh
encryption randomness; it is never resumed based on its length. An existing
final object is validated and reused, never replaced. The authenticated SQL
reservation establishes the configured key before pending staging inspection,
so corruption of an uncommitted key-ID header can be repaired. SQL key failures
and corrupt committed objects fail closed. This adapter uses one retained key;
key rotation requires coordinated migration of SQL and object envelopes.

Serialized payload references use a versioned `V1` representation. Inline
envelopes retain the SQL sealer's format byte and key ID; object envelopes retain
the `SMOBJ001` version and key ID. A future backend must preserve these encoding
and key identities, or explicitly migrate them while retaining the same logical
object identity and plaintext hash. Unknown versions fail closed.

The producer must preserve its candidate bytes on retry. An incomplete HTTP
download without a validated remote version cannot assume a new response is
the same candidate. Its external-effect adapter must resolve or abandon that
publication under SUB-1840's contract. Captured download materialization into
guest storage belongs to SUB-1339 and must create a distinct writable inode.

## Mapping to the model

| Model action | Implementation boundary |
| --- | --- |
| `Reserve` | SQLite transaction stores pending object identity, request fingerprint and reservation. |
| `WriteChunk`, `ResetStaging` | Bounded authenticated encoding; only invalid pending staging is truncated and rewritten. |
| `SyncFile` | `File::sync_all`, including repetition when recovering a complete candidate. |
| `Publish`, `SyncPublication` | Same-directory rename under exclusive ownership, then directory `sync_all`. |
| `CommitReference` | `bind_in`, object revision/state predicate and consumer transaction commit. |
| `PinReader`, `OpenReader` | Per-object access guard acquired before querying/opening; retained by blocking I/O. |
| `RetireReference` | Transactional release of distinct run/response/workspace/backup retention roots. |
| `ClaimCollection` | Durable deletion claim while holding the same access guard. |
| `Unlink`, `SyncDeletion`, `FinishCollection` | Remove staging/final names, sync directory, then commit `gone`. |
| `Discover`, `Restart` | Bounded SQL scan at server boot; complete pending publications and deletion claims are rediscovered. |

The local adapter serializes readers as well as writers per object. This is a
restriction of the model's multiple-reader behavior. SQL retirement may happen
while a reader runs; physical collection waits for the guard. Blocking tasks
own a guard clone, so dropping their async caller cannot release protection.
Object identities and tombstones are retained after collection.

The model's platform assumptions still apply: one process owns the store, no
external writer changes the directory, and successful persistence barriers are
honored. Directory-sync failures are errors, never ignored. Unsupported
platforms/filesystems do not gain a durability guarantee merely because they
can create a file. The adapter is not an EFS/S3 or multi-node implementation.

## Verification

Focused Rust checks:

```sh
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 cargo test -p submilli-server --lib objects
```

The tests exercise rollback and lost consumer acknowledgement, stale binding,
retention and collection, full-length damaged staging, missing/corrupt captures,
key failures, threshold boundaries, encrypted storage, bounded short reads and
caller cancellation while blocking I/O remains active.

The process harness kills a child at reservation, partial streaming, file sync,
rename, directory sync, reference commit, deletion claim, unlink, deletion sync
and completion. Every point runs once with the surviving kernel cache and once
with controlled loss of permitted unsynced state. The latter deliberately
damages staging, loses a staging name, rolls back a rename, or resurrects an
unlinked name. These injections complement the TLA+ persistence model; they do
not certify physical devices or simulate an entire operating system.
