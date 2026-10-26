---
title: "Run recovery"
description: "Configuration, commands, lifecycle states, and limits for durable session execution."
slug: reference/run-recovery
sidebar:
  order: 15
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "77d1054362b9e1fafb83418a7bbf6d6b522a4e54a47e85e7d3c878c26320fa17"
  confirmedAt: "2026-10-21T15:20:00Z"
---

## Configuration

| YAML key | Default | Constraint |
| --- | --- | --- |
| `durable_execution` | `false` | Requires persistent SQLite storage and an encryption key |
| `durable_objects.inline_threshold` | `64KiB` | Bytes or size string, from `0` through 64 MiB + 64 KiB. Entries larger than this threshold use private object files |
| `max_durable_runs` | `4` | Positive limit for admitting execution work, excluding suspended tool runs |

Database, key, and volume settings are listed in
[server settings](/docs/reference/server-settings).

Durable runs in different sessions can execute together when their writable
named volumes do not overlap. A named root or mount reserves its whole volume,
including other subdirectories, until the execution task has drained. Read-only
references do not reserve the volume. The retained blueprint determines these
references, even if the registered blueprint has changed.

After a restart, a run still marked `running` recovers before queued writers to
the same volume. A stuck run releases the volume for other sessions. Resuming it
after another writer has changed the files can therefore overwrite newer data
at a saved append offset; shared-volume advisory locking is not yet available.

## CLI commands

| Command | Operation |
| --- | --- |
| `submilli server runs list` | List persisted runs |
| `submilli server runs list --status stuck` | Filter by lifecycle status |
| `submilli server runs list --session SESSION_ID` | Filter by session |
| `submilli server runs show RUN_ID` | Read metadata and the retained response |
| `submilli server runs resume RUN_ID` | Read the revision, then request recovery of that stuck run |

All three commands accept `--json` and the standard server connection options.
The HTTP equivalents are [run endpoints](/docs/reference/http-api#get-v1runs).

## Lifecycle

| State | Startup behavior | Explicit resume |
| --- | --- | --- |
| `queued` | Schedule in session admission order | Not accepted |
| `running` from an earlier process | Recover with a new attempt | Not accepted |
| `stuck` | Retain without execution | Accepted with the observed revision |
| `completed`, `cancelled`, `aborted`, `expired` | Retire drained resources | Not accepted |
| `indeterminate` (non-durable runs only) | No automatic execution | Not accepted |

Queued, running, and stuck recorded runs prevent idle session expiry. Explicit
session closure cancels retained work and drains execution before cleanup.
A stuck run prevents later runs in its session from executing; other sessions
remain eligible. Disabling durability leaves unfinished recorded runs stuck.

An indeterminate non-durable run keeps its unknown outcome and idempotency
identity. It does not block later runs or appear in durable recovery and cleanup
work. Session closure preserves its record in run history.

## Retained execution data

| Item | Recovery behavior |
| --- | --- |
| Run ID, source, blueprint, variables | Retained unchanged |
| MCP declarations | Retained; no rediscovery during recovery |
| Filesystem roots and access | Checked against the retained fingerprint |
| Fuel and timeout | Original limits, fresh allowance for each attempt |
| Engine stack and memory settings | Incompatible current settings leave the run stuck |
| Completed native outcomes | Replayed without repeating completed native work |
| Native call without a committed outcome | May execute again |
| Result | Retained for keyed and unkeyed runs under response-retention settings |
| Expired result | Tombstone retained; no re-execution |

## Effect-history storage

| Property | Contract |
| --- | --- |
| Directory | `<volume_dir>/durable-objects/<session-id>/<run-id>/`, private to the server and unavailable through guest filesystems or named volumes |
| Directory names | Session and run IDs must each be a single unchanged path component. Empty names, dot components, separators, and NUL are rejected; directory symlinks are never followed. Names of at most 255 bytes containing only lowercase ASCII letters, digits, hyphens, underscores, or dots are used directly. Other valid IDs use `~` followed by their SHA-256 digest, avoiding case and Unicode aliases |
| Placement | Serialized history entries at or below `inline_threshold` stay in SQLite. Larger entries use encrypted object files with an encrypted SQLite reference. The size includes outcome metadata before encryption |
| Threshold changes | Apply to new entries. Existing inline entries and object references remain readable |
| Encryption | Uses the retained secret-store encryption key, including the existing key identifier and authenticated run/sequence binding |
| Limits | Existing 64 MiB payload limit, 64 KiB entry-metadata allowance, and 1 GiB per-run history limit still apply. Object bytes count toward the run limit. There is no separate object-store capacity limit or guest filesystem quota charge |
| Publication | The file and its directory are synced before the reference and run progress commit together |
| Read failure | A missing, corrupt, truncated, or undecryptable object stops recovery. It does not repeat the recorded operation |
| Retention | Files remain with their run, including interrupted writes without a committed outcome. Terminal retirement removes the run directory and then its SQLite outcomes. Failed deletion is retried before resources are marked retired. An empty session directory is removed |
| Ownership | One database-owning server on local or block storage. The persisted volume root belongs to that database. EFS and multi-node sharing are unsupported |
| Backup and restore | Stop the server and preserve SQLite, the complete volume root, and the encryption key together. Restoring only SQLite or replacing the object directory can make retained runs unrecoverable |
| Disabling durable execution | Retained recorded runs still require their directory and key for retention and cleanup |
| HTTP downloads | Use their existing filesystem and effect behavior. They do not use a separate object API |

```yaml title="server.yaml (fragment)"
durable_objects:
  inline_threshold: 64KiB
```

## Supported scope

| Area | Contract |
| --- | --- |
| Transports | Persistent HTTP and MCP session executions; one-shot execution remains ordinary |
| Filesystem modes | `per_session`, named, or disabled; ephemeral is refused. An enabled filesystem needs a writable root or mount for private recovery files |
| HTTP idempotency key | Retries observe the original run, including a stuck run |
| MCP request identity | No automatic idempotency key; another call may create another run |
| Disconnect | Accepted execution continues independently of its waiter |
| Long-running request | Queued runs and runs without a local attempt return metadata after the 25-second wait. A running tool-enabled program with a local attempt waits for its next suspension or terminal state |
| Object-storage platforms | Unix systems with file and directory synchronization. Native Windows durable execution is unsupported. Ordinary execution remains available |
| Storage ownership | One server process per SQLite database |
| File writers | Durable writers to the same named volume are serialized while their attempts remain active, including while suspended at a tool. Ordinary executions, external tools, and different volume names pointing to the same files still require exclusive access |
| Packages | Package contents must remain unchanged while runs are unfinished |
| External effects, including HTTP | Assumed idempotent. Calls without a committed outcome may execute again |
| General Git/code idempotency | Deferred |
| Durable application-tool waits | Supported inside the active attempt. The answer is accepted when its ordinary effect commits. Recovery replays to the same call and restarts `tool_call_timeout` for an unresolved wait |
| Suspended run capacity | A suspended run stays in memory and counts toward `max_suspended_runs`, but not toward `max_durable_runs`. It retains its session order and writable named volumes until its attempt exits. Resumed attempts continue without acquiring a new execution slot |
| Full suspended-run pool | Interrupts the durable attempt without recording a tool outcome. The run becomes `stuck` with `execution_interrupted` and can be explicitly recovered |
| Multi-node recovery | Unsupported |
