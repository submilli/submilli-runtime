---
title: "Resource limits"
description: "Reference for the limits that bound one program's run: memory, time, fuel, stack, files, model tokens, and session state, where each is set on submilli-server, and the fixed limits inside the standard library."
slug: resource-limits
sidebar:
  order: 13
---

Every program runs in a fresh WebAssembly instance: its own memory, its own
view of the filesystem, and nothing else. It reaches the outside only through
the standard library, which checks the blueprint on every call. It also runs
inside limits that bound how much it can use. Some of the programs an agent
writes will be wrong, a loop that never stops or a list that never stops
growing, and the limits make such a program fail on its own. It doesn't take
the server down, crowd out other sessions, or run up your model provider's
bill.

The instances of one server share its process. There is no process or
container boundary between programs, so the limits are what keeps one
session's program from starving another's, and a fault in the runtime itself
would reach every session.

Most limits are the operator's, set for the whole server. The blueprint adds
one of its own, a size limit on the program's files, and can narrow others
call by call with filters. It can't raise an operator's limit.

## At a glance

| Limit | Default | Set by | When a program passes it |
| --- | --- | --- | --- |
| Memory | 50 MB | `max_execution_memory`, MB | The run ends: `memory exhausted` |
| Time | none | `max_execution_time`, seconds | The run ends: `timeout exceeded` |
| Fuel | 10¹² | `max_execution_fuel` | The run ends: `fuel exhausted` |
| Stack | 512 KB | `max_execution_stack`, KB, at most 16,384 | The run ends: `call stack exhausted` |
| Filesystem size | none | the blueprint's `vfs.size_limit`; a named volume's `size_limit` in the server config | `QuotaExceededError` |
| Model tokens, one run | 1,000,000 | `max_execution_llm_tokens` | `QuotaExceededError` |
| Model tokens, all runs | 20,000,000 | `max_llm_tokens` | `QuotaExceededError` |
| Prompts in flight | 4 | `max_llm_concurrency` | Further prompts wait |
| Session state, one session | 16 MB / 1,024 keys | fixed | `QuotaExceededError` |
| Session state, all sessions | 1,024 MB | `max_session_state_memory`, MB | `QuotaExceededError` |

"The run ends" means the program can't catch it: the caller gets the error
in place of a result. Memory, time and fuel each have a `kind` of their own
(`memory_exhausted`, `timeout`, `fuel_exhausted`), which tells the limit from
a fault in the program. A `QuotaExceededError` is an ordinary error the program can
catch with `try` and act on. Sizes in this chapter are binary, as
the settings count them: a KB is 1,024 bytes and an MB is 1,024 KB.

A server setting has the three forms every server setting has. In the
[config file](/docs/server#configure-it) it is `max_execution_fuel`, as a flag
`--max-execution-fuel`, and in the environment
`SUBMILLI_MAX_EXECUTION_FUEL`. A flag beats a variable, and a variable beats
the file.

Fuel and model-token counts accept decimal `K`, `M`, `B`, and `T` suffixes
(thousand, million, billion, trillion) in the file, flags, and environment.
For example, `max_execution_fuel: 10B`, `max_llm_tokens: 20M`, and
`max_execution_llm_tokens: 1M`. Suffixes are case-insensitive and immediately
follow a whole number; fractions and scientific notation are not accepted.
Underscores may separate digits: `10_000_000_000` is also valid. On these
count settings, `B` means billion; byte sizes keep their own units.

## Memory

Each run is its own WebAssembly instance, and `max_execution_memory` caps the
memory that instance can use.

An allocation that would pass the limit ends the run with `memory exhausted`
(`kind: memory_exhausted`), and the program can't catch it. The server
reports:

```text
memory exhausted: GC heap out of memory: no capacity for allocation of 2000048 bytes
```

Memory the server holds for the program counts toward the same limit, and
ends the run the same way: open file handles and compiled regular
expressions.

Files needn't pass through memory. `http.download` writes a response
straight to a file, `fs.writer` writes one a line at a time, and `fs.lines`
and `fs.bytes` read one a piece at a time, so a program can handle files much
larger than its memory limit. A whole-file read, `fs.readText` or `fs.read`,
returns `null` for a file over 50 MB.

To size a container, budget `max_execution_memory` times the number of
programs you expect to run at once, plus `max_session_state_memory` for
sessions' state.

## Time

`max_execution_time` stops a run that takes too long. It is in seconds, off by
default, and starts counting when the program starts: the top-level statements
of the packages it imports run first, then its own, then `main`. The run ends
with `timeout exceeded` (`kind: timeout`), and the program can't catch it.

A call the program is waiting on finishes first, so a run can go past the
limit by as long as that call takes:

| Call | Gives up after |
| --- | --- |
| An HTTP request | 30 seconds |
| `http.download` | 60 seconds, or its `timeout` option |
| An MCP tool call | 60 seconds |
| A Git operation | 60 seconds |
| A model call | 10 minutes |

## Fuel and stack

Fuel counts work, roughly one unit per WebAssembly instruction. A run that
uses it all ends with `fuel exhausted` (`kind: fuel_exhausted`). The default,
a trillion, is a backstop; set `max_execution_time` to bound how long a caller
waits. Lower `max_execution_fuel` when you want a runaway loop stopped at the
same point every time, however busy the server is.

The stack bounds how deep calls go. The default 512 KB holds about 2,000
levels of recursion; deeper ends the run with `call stack exhausted`. Raise
`max_execution_stack`, up to 16 MB, for programs that recurse deeply by
design. A larger stack also uses more of the server's memory, outside
`max_execution_memory`.

## Filesystem

A blueprint with an `ephemeral` or `per_session` filesystem can cap how much
space its files take:

```yaml title="blueprint.yaml (fragment)"
vfs:
  mode: per_session
  size_limit: 100MB
```

`size_limit` takes a byte count or a size such as `500KB`, `100MB`, or `1GB`.
Under `per_session` it covers all the session's files, not each program's.

A write that would pass the limit is refused with a `QuotaExceededError` the program
can catch, and deleting files frees the space again. `fs.info().sizeLimit`
tells the program its limit, and is `-1` when there is none. A [named
volume](/docs/server#volumes) takes no `size_limit` in the blueprint: the
operator sets one where the server declares it, and that one limit covers
every session and blueprint using the volume. `fs.info().mounts` reports each
mounted volume's limit. Git writes and `http.download` count against the same
quota; `fs.remove` frees space for later writes. An unmeasured filesystem is
treated as full and refuses writes with `QuotaExceededError`.

```text
QuotaExceededError: fs.writeText /notes/big.md: the filesystem's size limit of 1024 bytes would be exceeded: 0 bytes are in use and this needs 2000 more
```

## Model spending

A program's `submilli:llm` calls spend tokens against three limits. Before a
prompt is sent, its size and the output reserved for it are counted against
the run's budget, `max_execution_llm_tokens`, and against the server's,
`max_llm_tokens`. A prompt that wouldn't fit is refused before it is sent,
with a `QuotaExceededError` naming the budget, so it is never billed. The server's
budget is your ceiling on the provider credential across every running
program.

The reserved output is 64,000 tokens per prompt unless the blueprint's model
sets `output_reserve`. It is also sent as the request's output cap:

```yaml title="blueprint.yaml (fragment)"
llm:
  models:
    claude-haiku-4-5:
      provider: anthropic
      output_reserve: 4000
```

`max_llm_concurrency` bounds how many of one `llm.batch`'s prompts are in
flight at once; the rest wait their turn. A batch takes at most 128 prompts,
and a prompt at most 256 KB. Those per-request bounds throw `RangeError`.

## Session state

`submilli:session` keeps keys and values for the life of a session. Each
session may hold 16 MB, in at most 1,024 keys, each value at most 1 MB and
each key at most 256 characters. Across every open session,
`max_session_state_memory` bounds the total, so a server with many sessions
can't be filled by them. A `set` that would pass either is refused with a
`QuotaExceededError`, and nothing another session holds is evicted to make room.
Individual key and value size caps remain `RangeError`.

A session nobody has used for its blueprint's `idle_timeout`, 24 hours by
default, is closed, and its state and `per_session` files are deleted.

## Inside the standard library

These limits are fixed. A program that meets one gets an error it can catch:

| Where | Limit | When passed |
| --- | --- | --- |
| HTTP response (`http.get` and the other verbs) | 50 MB | `RangeError`: the message suggests `http.download` |
| `http.download` | 50 MB, or the program's `maxBytes` option | `RangeError` |
| Redirects | 10 | Error |
| `fs.read`, `fs.readText` | 50 MB (`fs.maxReadSize()`) | Returns `null` |
| A string built by `repeat`, `padStart`, `join`, and the like | 32M characters | `RangeError` |
| A `Uint8Array` | 1 GB | `RangeError` |
| `JSON.parse` nesting | 128 levels | `SyntaxError` |
| Object nesting, when compared, hashed, or stringified | 128 levels | `RangeError` |
| A regular expression | 1 MB compiled; matching is linear, with no backreferences or lookaround | `SyntaxError` for a pattern too large |
| MCP server discovery | 10 seconds | The server is left out, with a warning |

`http.download`'s `maxBytes` has no upper bound of its own, because the body
goes to a file and not to memory: the file size limit and the disk bound it.
A blueprint can bound it per call with a filter:

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: http.download
    filter: host == "files.example.com" and max_bytes <= 104857600
    action: allow
```

## Git

`submilli:git` reads and changes a repository where it is, so a repository's
size counts against the volume's `size_limit`, not against memory:

- **Memory.** Git's work counts against `max_execution_memory`, but it holds
  what one operation touches, not the repository: the paths it lists, and one
  file at a time. Under the default 50 MB, Git takes a file of up to about
  8 MB, a diff of up to about 4 MB, and tens of thousands of paths. A file,
  diff or listing larger than that fails with an error that says to raise
  `max_execution_memory`. So does a repository whose packs hold more than
  about 50,000 objects under the default.
- **Other limits.** Directories nest at most 64 levels. A new branch or remote name
  is at most 250 bytes. A history page returns at most 1,000 commits, 50 by
  default.
- **Packs.** Git uses a repository's packs with the indexes native Git wrote
  for them. A pack without a valid index is refused; run `git index-pack` on
  it.
- **Time.** Each operation has 60 seconds, including the waits for another
  operation on the same repository and for one of the four Git workers a
  server shares among all its programs.
- **Fetch.** A fetch may bring at most half of what the `size_limit` leaves,
  or 4 GB without one. It downloads the history it needs without first
  telling the remote what is already present, so it may transfer objects the
  repository has.

An operation that passes a limit fails with an error that names it, and the
repository is left as it was. A change is staged in a `.git-submilli-…`
directory beside `.git` and moved into place at the end; the next change
removes one a stopped server left behind. If the server stops while it is
moving files, Git refuses the repository until it is recovered: restore the
repository, from a backup or by cloning it again, then remove the directory.

Operations on one repository take turns within a server, so only one server
should work on a repository: two servers sharing its volume could undo each
other's changes.

## With a coding agent

A coding agent with the [Submilli skill](/docs/skill) knows these limits and
where each one is set. These prompts were run with Claude Code in a project
holding a research agent's blueprint, a `per_session` filesystem with
`@submilli/jina`, and the server's `server.yaml`.

**Cap the agent's files.**

```text
Cap the files this agent keeps in its session at 100 MB.
```

The agent adds `size_limit: 100MB` to the blueprint's `vfs` block and tests
it with `submilli run`: a program that grows a file a megabyte at a time is
refused at the limit with a `QuotaExceededError` it can catch. It points out that
`100MB` is 104,857,600 bytes, in case you meant the decimal figure. It also
says what it didn't test: two programs in one session, which needs a server,
and Jina's downloads, which would mean real calls to Jina.

**Stop runaway programs.**

```text
Programs our agent writes sometimes loop forever, and our API call to the Submilli server times out after 30 seconds. What should we change? server.yaml is the server's config.
```

The agent explains that the server has no time limit by default, so a stuck
program runs until its fuel is gone, long after your client gave up. It adds
`max_execution_time: 25` to `server.yaml`, a few seconds under your client's
timeout, and says the server needs a restart to apply it. It then warns about
the limit's gap: a call already waiting isn't cut off. The blueprint reaches
Jina, whose requests time out after 30 seconds, so a request started at
second 24 can end a run near second 54. It suggests treating `timeout
exceeded` and your own timeout as the same result for the agent.

Next: [deploying](/docs/deploying).
