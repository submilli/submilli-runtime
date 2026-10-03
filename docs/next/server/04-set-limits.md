---
title: "Set limits"
description: "How to set the server's limits: bound CPU work with fuel, keep a time limit as the backstop for your callers, size memory and the container, bound recursion, cap model spending and session state, and read what a program sees when it passes one."
slug: next/server/set-limits
pagefind: false
# Written for SUB-1269 (https://linear.app/submilli/issue/SUB-1269, host
# functions charge fuel) as fixed: the loop's fuel is measured, and the
# host-side figures under "What a budget buys" are estimates against the
# charging scheme SUB-1269 proposes, to be replaced by SUB-1270's
# measurements. The `1B`-style counts (SUB-1272) and the `--report` output
# and server log line (SUB-1271) are real, on main 10da5c4.
sidebar:
  order: 4
  hidden: true
---

Some of the programs an agent writes will be wrong: a loop that never
stops, a list that never stops growing, a batch that asks a model a
million questions. Every program runs in a fresh WebAssembly instance
inside the server process, with no process or container boundary between
one and the next, so the limits are what keeps such a program from taking
the server down, starving another session, or running up your model
provider's bill. They are the operator's; a blueprint can't raise them.

This guide shows you how to set the server's limits: bound CPU work with
fuel, keep a time limit as the backstop for your callers, size memory and
the container that holds it, bound recursion, cap model spending and
session state, and read what a program sees when it passes one. Refer to
[Errors and limits](/docs/next/reference/errors-and-limits) for every
limit, its default, and the fixed limits inside the standard library.

## Choose what to set

| Limit | Default | Setting | When a program passes it |
| --- | --- | --- | --- |
| Fuel | 10¹² | `max_execution_fuel` | The run ends: `fuel exhausted` |
| Time | none | `max_execution_time`, seconds | The run ends: `timeout exceeded` |
| Memory | 50 MB | `max_execution_memory`, MB | The run ends: `memory exhausted` |
| Stack | 512 KB | `max_execution_stack`, KB, at most 16,384 | The run ends: `call stack exhausted` |
| Model tokens, one run | 1,000,000 | `max_execution_llm_tokens` | `QuotaExceededError` |
| Model tokens, all runs | 20,000,000 | `max_llm_tokens` | `QuotaExceededError` |
| Prompts in flight | 4 | `max_llm_concurrency` | Further prompts wait |
| Session state, all sessions | 1,024 MB | `max_session_state_memory`, MB | `QuotaExceededError` |

"The run ends" means the program can't catch it: the caller gets the
error in place of a result, with a `kind` (`fuel_exhausted`, `timeout`,
`memory_exhausted`) that tells the limit from a fault in the program. A
`QuotaExceededError` is an ordinary error the program can catch with
`try` and act on.

The examples below set the limits in the config file from [Run the
server](/docs/next/server/run-the-server); the server reads them at
startup.

## Bound CPU work with fuel

Fuel counts work. The program's own instructions cost about one unit
each, and a standard library function charges for what it does on the
program's behalf: about one unit per character of a string it reads or
writes, per byte of JSON or regex input, per element of an array or
`Map` it touches, per byte of an HTTP body it handles. Waiting costs
nothing: a program blocked on an HTTP request, a model call, or an MCP
tool spends no fuel while it waits, and that is most of what an agent's
programs do. A server holds hundreds of such executions at once at no
CPU cost, so fuel, not time, is the limit to set first: it bounds the
CPU a program may burn, it stops the same program at the same point
however busy the server is, and it never cuts off a program for being
slow at waiting.

### What a budget buys

Measured with `submilli run` on a laptop core, a loop of a million
simple iterations costs 38,000,026 fuel and runs in about 110
milliseconds:

```typescript title="million.ts"
function main(): number {
    let acc = 0;
    for (let i = 0; i < 1000000; i++) {
        acc = (acc + i * 7) % 1000003;
    }
    return acc;
}
```

So a billion fuel
is a couple of seconds of pure computation, and the default, a trillion,
is on the order of half an hour of CPU: a backstop, not a budget. For
the work agents' programs do:

| Work | Fuel, roughly |
| --- | --- |
| A loop of a million simple iterations | 40 million |
| `toUpperCase`, `split`, `replaceAll` over 1 MB of text | 1 million |
| Parsing 1 MB of JSON | 1 million |
| A regex over 1 MB of text | 1 million |
| Building a `Map` of 100,000 entries | a few million |
| Reading and parsing a 1 MB HTTP response | 2 million |

A budget of ten billion, `10B`, lets a program read and process a few
hundred megabytes, or compute for twenty seconds, before it stops; set it
lower when your programs are small and a runaway loop should stop within
a second or two. Counts take a `K`, `M`, `B`, or `T` suffix:

```yaml title="server.yaml (fragment)"
max_execution_fuel: 1B
```

Under that budget, a loop that never stops ends with `fuel exhausted`,
and the program can't catch it:

```typescript title="loop.ts"
function main(): void {
    let n = 0;
    while (true) {
        n = (n + 1) % 1000;
    }
}
```

```text
error: fuel exhausted
  at main (<execute>:4:23)  [fuel exhausted]
3 |     while (true) {
4 |         n = (n + 1) % 1000;
  |                       ^
5 |     }
```

### Measure a program

To see what a program costs, run it locally with `--report`, which
prints the run's usage to standard error after the result:

```sh
submilli run --report million.ts
```

```text
42
fuel: 38,000,026   memory peak: 0.1 MB   wall: 115 ms (compile 4 ms, run 110 ms)
```

The report comes after a failed run too, with the fuel the run had spent
when it ended. The memory figure is what the program held at its peak,
counted as the limit counts it, not what the process used.

Run the programs your agent produces, or a package's tests, and set the
budget a few times above the largest honest figure. A server logs the
same figures once per execution, with the blueprint, the session, and
how the run ended, so a limit that fires shows up in the log without
the client's help:

```text
INFO submilli_server::execute: execution finished blueprint="support" session="1be1de26-d368-4f8d-864d-2e5496ff962b" fuel=38000026 memory_peak=65536 wall_ms=155 outcome="ok"
```

## Keep a time limit as the backstop

There is no time limit by default, because time is a weak limit: it
counts waiting as well as working, and a program that waits a minute on
a slow API has done nothing wrong. Set one anyway, a few seconds under
your application's own timeout, so that a program stuck on a call that
never returns ends on the server rather than only in your client:

```yaml title="server.yaml (fragment)"
max_execution_time: 25
```

The clock starts when the program starts: the top-level statements of the
packages it imports run first, then its own, then `main`. A program that
passes the limit ends with `timeout exceeded`, and the program can't
catch it. The same `loop.ts` under a two-second limit, for the
example's sake:

```text
error: timeout exceeded
  at main (<execute>:4:23)  [timeout exceeded]
3 |     while (true) {
4 |         n = (n + 1) % 1000;
  |                       ^
5 |     }
```

A call the program is waiting on finishes first, so a run can go past the
limit by as long as that call takes. Treat `timeout exceeded` and your
own timeout as the same result for the agent:

| Call | Gives up after |
| --- | --- |
| An HTTP request | 30 seconds |
| `http.download` | 60 seconds, or its `timeout` option |
| An MCP tool call | 60 seconds |
| A Git operation | 60 seconds |
| A model call | 10 minutes |

## Size memory and the container

`max_execution_memory` caps the memory one instance can use, 50 MB by
default. An allocation that would pass the limit ends the run, and the
program can't catch it:

```typescript title="grow.ts"
function main(): void {
    const parts: string[] = [];
    while (true) {
        parts.push("x".repeat(1000000));
    }
}
```

```text
memory exhausted: GC heap out of memory: no capacity for allocation of 2000044 bytes
```

Memory the server holds for the program counts toward the same limit,
and ends the run the same way: open file handles and compiled regular
expressions. Files needn't pass through memory: `http.download` writes a
response straight to a file, `fs.writer` writes one a line at a time, and
`fs.lines` and `fs.bytes` read one a piece at a time, so a program can
handle files much larger than its limit.

The limit counts memory a program holds, not memory the process uses on
the way: one execution near its limit has been measured at more than 12
times that in process memory, released when it finishes. So size a
container as the Helm chart does, at 128 MiB plus 16 times
`max_execution_memory`, which covers one such execution at a time;
several at once can use more, so if your programs handle large data,
measure your own peak. Add `max_session_state_memory` for the sessions'
state.

## Bound recursion

The stack bounds how deep calls go. The default 512 KB holds about 2,000
levels of recursion; deeper ends the run:

```typescript title="deep.ts"
function depth(n: number): number {
    return depth(n + 1) + 1;
}

function main(): number {
    return depth(0);
}
```

```text
error: call stack exhausted
  at depth (<execute>:2:22)  [stack overflow]
1 | function depth(n: number): number {
2 |     return depth(n + 1) + 1;
  |                      ^
3 | }
  at depth (<execute>:2:22)  [caller]
…
```

Raise `max_execution_stack`, up to 16 MB, for programs that recurse
deeply by design. A larger stack also uses more of the server's memory,
outside `max_execution_memory`.

## Cap model spending

A program's `submilli:llm` calls spend tokens against two budgets and
wait on a concurrency cap. Before a prompt is sent, its size and the
output reserved for it are counted against the run's budget,
`max_execution_llm_tokens`, and against the server's, `max_llm_tokens`.
A prompt that wouldn't fit is refused before it is sent, with a
`QuotaExceededError` naming the budget, so it is never billed. The server's
budget is your ceiling on the provider credential across every running
program; the run's keeps one program from spending it all.
`max_llm_concurrency` bounds how many of one batch's prompts are in
flight at once; the rest wait their turn.

The reserved output is 64,000 tokens per prompt unless the blueprint's
model sets `output_reserve`, which [Allow model
calls](/docs/next/blueprints/allow-model-calls) covers.

## Bound session state and files

`submilli:session` keeps keys and values for the life of a session, 16 MB
per session. Across every open session, `max_session_state_memory` bounds
the total, so a server with many sessions can't be filled by them. A
`set` that would pass either is refused with a `QuotaExceededError`, and
nothing another session holds is evicted to make room.

The size of a session's files is the blueprint's limit, not the server's:
`vfs.size_limit`, which [Keep files and
state](/docs/next/blueprints/keep-files-and-state) covers. A named
volume's limit is the server's, set where the volume is declared, as
[Mount a shared volume](/docs/next/server/mount-a-shared-volume) shows.

