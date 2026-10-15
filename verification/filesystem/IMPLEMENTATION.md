# Filesystem recovery adapter

This implements the filesystem portion of SUB-1339. The server's
`DurableFilesystem` combines a sealed SQLite journal with the engine's
`runtime::fs::recovery::Workspace`. It is usable without an interpreter.
Execution history, manifests, effect dispatch, terminal run commits and startup
supervision remain separate work. Existing HTTP/MCP requests and ordinary
filesystem/Git host functions are not automatically routed through this API.

## Supported boundary

The adapter reuses retained files. It never rebuilds a workspace by replaying
completed mutations. Ephemeral roots live at
`<volume_dir>/ephemeral/submilli-vfs-run-<workspace-id>`; the ID binds the admitted
run to the object store/database identity. Interpreter/VFS Drop keeps the root.
Named, per-session and mounted writable volumes keep their ordinary paths;
retirement removes private recovery artifacts, not their user data.

One logical run must exclusively own all affected backing files until pending
operations reconcile, including aliases, ordinary host functions, Git, tools
and external writers. The local gate serializes adapter access and drains
cancelled blocking attempts. It does not exclude other logical runs or external
processes. General locks remain SUB-1295, with TLA+ verification required before
implementation. No HA takeover, leases, stale-owner fencing or network storage
guarantees are claimed.

Storage must preserve successful file and directory synchronization. Directory
sync errors are returned. Models distinguish process death from power loss;
process-kill tests do not establish a device's power-loss contract. Missing or
corrupt roots, pins, parent identities, captures and append prefixes fail closed.

## Journal and persistence ordering

Migration 10 holds workspace, effect, resource and ordered-plan records, separate
from execution history. Paths, layouts, intents and outcomes are sealed.
Captured bytes and directory snapshots live in the immutable object store.
Mutable candidate files never share an inode with an immutable capture.

| Boundary | Implementation | Model action |
| --- | --- | --- |
| Reserve before mkdir | SQLite preparing reservation, with parent identity | Workspace ownership precondition |
| Record retained layout | Sealed layout, then active root identities | Stable workspace precondition |
| Admit immutable request and retention | One SQLite transaction | `Intent` |
| Prepare private evidence | Sync baseline, pin original, copy/validate candidate | Pinning and candidate preparation |
| Store prepared intent | Separate SQLite pending transition | Strengthens the pre-apply guard |
| Publish or reconcile | Capability-relative syscalls with expected identities | Primitive `Apply` actions |
| Persist files and directories | Fallible synchronization barriers | File/directory sync actions |
| Commit result before returning | SQLite done/rejected outcome | `CommitReceipt` |
| Duplicate call | Check request, return outcome without filesystem access | `Replay` |
| Terminal cleanup | Fence, drain, delete, release retention, tombstone | Terminal/drain/collection |

The model's pending intent spans the implementation's preparing request and
prepared intent. Preparation writes only private artifacts. Its definitive
preflight refusals can be recorded. Once public mutation starts, an error leaves
the intent pending; it cannot become a definitive guest failure. A lost SQL
acknowledgement is resolved by loading the committed row.

Registration can stop before mkdir, after creating an empty root, or after
storing its full layout. Cleanup handles each case. Bootstrap cleanup removes
only an empty reserved root. Full-layout cleanup checks backing-root identities
directly and tolerates already deleted cwd, mount placeholders, private subtrees
or ephemeral roots. It does not restore an execution layout before deletion.

## Primitive operations

* Write/download materialization copies a capture into a private candidate,
  verifies length/hash, synchronizes it and publishes its name. Recovery repairs
  uncommitted candidate bytes on the same inode. A completed receipt never
  overwrites a later edit. Ordinary writes have new-file metadata semantics.
* Append pins the original inode and hashes its durable prefix. Each attempt
  opens without `O_APPEND`, seeks to the recorded original length and repairs
  the captured interval. It preserves inode metadata and live-reader visibility.
  Unexpected prefix bytes or length block recovery.
* Same-volume move publishes a private hard link at the destination, syncs that
  parent on every attempt, then unlinks the source and syncs its parent. Both
  names may temporarily identify the same inode. The pin also supports defensive
  recovery of all four combinations of persisted source/destination changes.
* Remove retains the original inode before unlinking. Completed retries return
  their saved result even after path reuse.
* Empty-directory mkdir/rmdir follow their separate model. A pending new mkdir
  may recognize only an empty directory under exclusive ownership. No child
  mutation is admitted before its receipt.

Quota counts unique physical inodes, including retained versions and candidates.
Growth is admitted before writing; shared counters reconcile after partial or
completed work and terminal cleanup. Same-process reopen shares existing volume
counters. Retained originals can consume space until terminal cleanup.
Bounds include 10,000 traversed entries, depth 64, 1,000 steps per plan, 64 KiB
protocol records and 4 MiB directory snapshots. Oversized plans fail before
public mutation. File payloads use the configured object/filesystem limits.

## Resources and compound operations

Readers durably pin an inode before returning a logical resource. A
`ReaderPosition` identifies that inode and the first unread byte independently
of pathname and OS descriptor. Restoration preserves replacement semantics while
observing append to the original inode. Native access guards delay retirement.

Execution history must commit read observations and cursor advancement together.
Replay returns recorded bytes, never a fresh read. A buffered text adapter must
also save decoder/BOM/EOF state and account for prefetched unread bytes. Existing
guest handle IDs and uncheckpointed native BufReaders are not resume points.

Durable writers record immutable chunks under stable IDs. Duplicate chunks do
not append twice. Close captures the ordered concatenation and publishes it
through the write protocol under a stable effect ID. Pause keeps every chunk;
an unfinished close blocks new mutations until its outcome is durable.
Permanent capture-size and effect-slot limits are checked before closing fences
the workspace. Its aggregate object quota is reserved before close admission;
the capture ID includes the chunk fingerprint so an interrupted reservation does
not prevent later chunks. Directory captures reserve their actual encoded size,
bounded by the snapshot and object limits; a listing that cannot fit records a
definitive refusal.

Directory iterators use a captured sorted listing and an offset, even if the
directory changes later. Their object producer is journaled before reservation,
so interrupted captures can be retired without leaking permanent object pins.

`execute_plan` persists an ordered immutable mutation list and pins its captures
before the first step. Each step has a derived ID, intent and receipt. Plans
reserve capacity for all steps and reject reused effect IDs before admission.
Recursive copy/remove and cross-volume moves use create/write/remove steps. The caller
supplies captured source versions and admits the whole plan's capabilities.
Recovery never re-enumerates sources. A definitive refusal may leave a committed
prefix; its recorded failure prevents replay from repeating that prefix.

`publish_git_plan` accepts prepared Git files and refs, with the same captured
commit/result. It rolls forward: after admission, failures leave the publication
pending instead of reporting a partially applied Git error. Git-scoped mutations
exclude private staging names, reject parent aliases and preserve executable
flags. The caller orders objects before refs. The ordinary Git host function's
volatile undo list is not used; the future bridge must submit its prepared
publication and must not regenerate the commit after a crash.

Explicitly unsupported: disposable TempDirs, aliased writable volume roots,
arbitrary hard-link aliases, symlink/special-file mutations, raw directory
rename, native locks and uncheckpointed buffered guest handles. Cross-volume
move uses a captured plan. A recorded lock acquisition is never a current lock.

## Verification and integration

The [append](README.md), [namespace](namespace/README.md),
[directory](directories/README.md), [resource](resources/README.md),
[sequence](sequences/README.md) and [object](../objects/README.md) models check
separate bounded protocols. Composition assumes immutable captures and
idempotent primitive steps, not an end-to-end proof of the Rust runtime.

Rust tests exercise partial append, corrupt candidates, move states, lost pins,
path reuse, private aliases, quota, persistence failures, reader cursors, writer
pause/close, changed listings, compound prefixes and the exact Git commit after
interrupted publication. Child-process kills exercise SQL request/intent/receipt
and workspace initialization/retirement with a fresh database/store reopen.

Before enabling guest durability, the execution bridge must supply stable IDs,
immutable captures, full capability/fuel admission, recorded observations and
logical checkpoints. It must settle work after effects, commit execution results
before exposing them, finish filesystem recovery before new live work and retire
only after terminal authority and draining. Ordinary session expiry and boot
scratch sweeping preserve SQL-retained workspaces until retirement completes.
