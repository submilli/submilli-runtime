---
title: "Set limits"
description: "How to set the server's limits: bound CPU work with fuel, keep a time limit as the backstop for your callers, size memory and the container, bound recursion, cap model spending and session state, and read what a program sees when it passes one."
slug: server/set-limits
# Written for SUB-1269 (https://linear.app/submilli/issue/SUB-1269, host
# functions charge fuel) as fixed: the loop's fuel is measured, and the
# host-side figures under "What a budget buys" are estimates against the
# charging scheme SUB-1269 proposes, to be replaced by SUB-1270's
# measurements. The `1B`-style counts (SUB-1272) and the `--report`
# output (SUB-1271) are real, on main 10da5c4. The logfmt server
# log line was recaptured with the release server on main 51ce450b.
sidebar:
  order: 4
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "26481d4b9024130bd7d4a9f4d5083bf1dbfc724318a2b5d647ddb6a5c05f7234"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

Some of the programs an agent writes will be wrong. A loop never stops,
a list keeps growing, or a batch asks a model a million questions. Each
program runs in a separate WebAssembly instance inside the server
process, which ends a failing run without disturbing the others ([The
server](/docs/server#when-a-program-fails)). The limits decide when a run
like that ends, so it can't starve another session or run up your model
provider's bill. The operator sets them, and a Blueprint can't raise them.

This guide shows you how to set the server's limits. [Errors and
limits](/docs/reference/errors-and-limits) lists each limit, its default,
and the fixed limits inside the standard library.

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

"The run ends" means the program can't catch it. The caller gets the
error in place of a result, with a `kind` (`fuel_exhausted`, `timeout`,
`memory_exhausted`) that tells the limit from a fault in the program. A
`QuotaExceededError` is an ordinary error the program can catch with
`try` and act on.

The examples below set the limits in the config file from [Run the
server](/docs/server/run-the-server). The server reads them at startup.

## Bound CPU work with fuel

Fuel counts work. The program's instructions cost about one unit each,
and a standard library function charges for what it does on the
program's behalf. That is about one unit per character of a string it
reads or writes, per byte of JSON or regex input, per element of an
array or `Map` it touches, and per byte of an HTTP body it handles.
Waiting costs nothing. A program blocked on an HTTP request, a model
call, or an MCP tool spends no fuel while it waits, and agents' programs
spend most of their time waiting. A server holds hundreds of such
executions at once at no CPU cost. So set fuel first, before time. Fuel
bounds the CPU a program may burn. It stops the same program at the same
point however busy the server is, and it never cuts off a program for
being slow at waiting.

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

So a billion fuel is a couple of seconds of pure computation. The
default, a trillion, is on the order of half an hour of CPU, which makes
it a backstop more than a budget. For the work agents' programs do:

| Work | Fuel, roughly |
| --- | --- |
| A loop of a million simple iterations | 40 million |
| `toUpperCase`, `split`, `replaceAll` over 1 MB of text | 1 million |
| Parsing 1 MB of JSON | 1 million |
| A regex over 1 MB of text | 1 million |
| Building a `Map` of 100,000 entries | a few million |
| Reading and parsing a 1 MB HTTP response | 2 million |

A budget of ten billion, `10B`, lets a program read and process a few
hundred megabytes, or compute for twenty seconds, before it stops. Set it
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
fuel: 38,000,105 (wasm 38,000,026, host 79)   memory peak: 0.1 MB   wall: 94 ms (compile 5 ms, run 88 ms)
```

The fuel is one budget spent two ways. `wasm` is the program's
instructions, and `host` is what the standard library's functions charge
for the work they do for it. The report also follows a failed run, with
the fuel the run had spent when it ended. The memory figure is what the
program held at its peak, counted as the limit counts it, not what the
process used.

Run the programs your agent produces, or a Package's tests, and set the
budget a few times above the largest honest figure. A server logs the
same figures once per execution, with the Blueprint, the session, and
how the run ended. A limit that fires shows up in the log without the
client's help:

```text
ts=2026-10-03T17:04:47.938Z level=info stream=log target=submilli_server::execute msg="execution finished" blueprint=support session=a9644213-e11b-44fa-bfe8-2fcbe4848fe5 fuel=38000105 wasm_fuel=38000026 host_fuel=79 memory_peak=65536 wall_ms=94 outcome=ok
```

## Keep a time limit as the backstop

There is no time limit by default, because time is a weak limit. It
counts waiting as well as working, and a program that waits a minute on
a slow API has done nothing wrong. Set one anyway, a few seconds under
your application's timeout. Then a program stuck on a call that never
returns ends on the server too, and not only in your client:

```yaml title="server.yaml (fragment)"
max_execution_time: 25
```

The clock starts when the program starts. The top-level statements of the
Packages it imports run first, then the program's, then `main`. A program that
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
application's timeout as the same result for the agent:

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

Memory the server holds for the program, such as open file handles and
compiled regular expressions, counts toward the same limit and ends the
run the same way. Files needn't pass through memory. `http.download`
writes a response straight to a file, `fs.writer` writes one a line at a
time, and `fs.lines` and `fs.bytes` read one a piece at a time. A program
can handle files much larger than its limit.

The limit counts memory a program holds, not memory the process uses on
the way. One execution near its limit has been measured at more than 12
times that in process memory, released when it finishes. So size a
container as the Helm chart does, at 128 MiB plus 16 times
`max_execution_memory`. That covers one such execution at a time.
Several at once can use more, so if your programs handle large data,
measure your peak. Add `max_session_state_memory` for the sessions'
state.

## Bound recursion

The stack bounds how deep calls go. The default 512 KB holds about 2,000
levels of recursion. Going deeper ends the run:

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
`QuotaExceededError` naming the budget, so it is never billed. The
server's budget is your ceiling on the provider credential across all
running programs. The run's budget keeps one program from spending it
all. `max_llm_concurrency` bounds how many of one batch's prompts are in
flight at once, and the rest wait their turn.

The reserved output is 64,000 tokens per prompt unless the Blueprint's
model sets `output_reserve`, which [Allow model
calls](/docs/blueprints/allow-model-calls) covers.

## Bound session state and files

`submilli:session` keeps keys and values for the life of a session, 16 MB
per session. Across all open sessions, `max_session_state_memory` bounds
the total, so a server with many sessions can't be filled by them. A
`set` that would pass either is refused with a `QuotaExceededError`, and
nothing another session holds is evicted to make room.

The Blueprint, not the server, limits the size of a session's files with
`vfs.size_limit`, which [Keep files and
state](/docs/blueprints/keep-files-and-state) covers. A named
volume's limit is the server's, set where the volume is declared, as
[Mount a shared volume](/docs/server/mount-a-shared-volume) shows.

