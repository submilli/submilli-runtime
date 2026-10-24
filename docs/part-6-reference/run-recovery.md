---
title: "Run recovery"
description: "Configuration, commands, lifecycle states, and limits for durable session execution."
slug: reference/run-recovery
sidebar:
  order: 15
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "9d891ed7fa2c0dbe711a8e1118e23d1aba5f51a07a4d30bdea349e92692a1560"
  confirmedAt: "2026-10-21T15:20:00Z"
---

## Configuration

| YAML key | Default | Constraint |
| --- | --- | --- |
| `durable_execution` | `false` | Requires persistent SQLite storage and an encryption key |
| `max_durable_runs` | `4` | Positive maximum of concurrently executing sessions |

Database, key, and volume settings are listed in
[server settings](/docs/reference/server-settings).

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

## Supported scope

| Area | Contract |
| --- | --- |
| Transports | Persistent HTTP and MCP session executions; one-shot execution remains ordinary |
| Filesystem modes | `per_session`, named, or disabled; ephemeral is refused. An enabled filesystem needs a writable root or mount for private recovery files |
| HTTP idempotency key | Retries observe the original run, including a stuck run |
| MCP request identity | No automatic idempotency key; another call may create another run |
| Disconnect | Accepted execution continues independently of its waiter |
| Long-running request | May return run metadata after the bounded wait |
| Storage ownership | One server process per SQLite database |
| File writers | Exclusive access to backing files, including other sessions and external tools |
| Packages | Package contents must remain unchanged while runs are unfinished |
| External effects, including HTTP | Assumed idempotent. Calls without a committed outcome may execute again |
| General Git/code idempotency | Deferred |
| Durable application-tool waits | Refused; deferred |
| Multi-node recovery | Unsupported |
