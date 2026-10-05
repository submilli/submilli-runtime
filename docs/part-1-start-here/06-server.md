---
title: "The server"
description: "What submilli-server is and why it is built the way it is: an isolated WebAssembly instance per run instead of a microVM, and what it does with a program."
slug: server
sidebar:
  order: 6
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "01385368bc24aa7aaaa42d1e0f231a0f0dbe71ee966988fe4722afda97ed859d"
  confirmedAt: "2026-10-05T10:59:51.494Z"
---

`submilli-server` is the process that runs the agent's programs. It is one
long-lived service that holds your blueprints, packages, and secrets. For
each run it builds the environment a blueprint describes, runs the program
inside, and answers every check from that blueprint.

Consider the alternative, a large machine running a microVM per agent with
Firecracker or the like. Each microVM takes half a gigabyte to a gigabyte
of memory, so a machine with 64 GB holds a few dozen. You keep them warm
between invocations, because starting one costs about 100 ms you don't
want to pay on every turn, and you pay for the time they are up, most of
which an agent spends waiting on an HTTP call. Memory sets your density,
and the machine idles on your bill.

Submilli runs each program as a WebAssembly instance inside the server
process. An instance costs a few megabytes, and no CPU while it
waits on a request. Agent work is mostly waiting, so one server carries hundreds of runs at once and thousands of sessions between runs.
WebAssembly isolates one program from another, and the server puts
limits around it, on memory and on compute among others. It is a microVM's
isolation at a fraction of the cost.

In the quickstart you started the server, registered a blueprint, and sent
it two programs. This chapter is what happened in between.

## What happens to a program

<!-- video:works -->

Your application, or your agent framework, sends the server three things:
the program's source, the name of a blueprint, and values for the
blueprint's variables. The server then does four things.

1. **Compile.** The server parses and type-checks the source and compiles it
   to WebAssembly, a binary format designed for running code in isolation.
   A program with a type error never starts. Generated programs are compiled
   on every request. Packages are compiled once, when you install them.
2. **Create an instance.** Each run gets its own instance and memory, the
   environment the blueprint describes, and sees nothing left by an earlier
   run, apart from the file area or session state a blueprint can grant.
   Creating one takes less than a millisecond.
3. **Run `main`.** The program runs inside the server process. Whenever it
   calls an operation that touches the outside world, the runtime consults
   the blueprint first.
4. **Return.** The value `main` returns is the result. A successful run
   returns only that value. The logs stay on the server for the framework
   to fetch with another tool call. A failed run returns the error and the
   logs.

## Limits

As you would size a microVM or put a container in a cgroup, the server lets
you bound each instance, for whatever reason you have: to keep one runaway
program from starving the others, to cap what a run can cost, or because a
loop that never stops is one of the programs an agent will write. Around an
instance you can set:

- **Memory** it may allocate.
- **Fuel**, the instructions it may execute. Fuel is a CPU quota counted in
  work done rather than in time, so the same program stops at the same point
  however busy the server is.
- **Time**, for callers that have a timeout of their own.
- **Call depth**, for recursion.
- **Files**, the size of what a session may keep.
- **Network**. By default, the server refuses connections to private
  addresses, loopback, the private ranges, and link-local, which covers the
  cloud metadata endpoint that hands out credentials, whatever a blueprint
  allows.

A run that passes a limit stops with an error the model can read. The
limits belong to the operator. A blueprint can cap its own files and narrow
with filters, but it can't raise them.

## When a program fails

Every run shares the server process, so a run that fails must end alone.
Two things make sure it does.

First, WebAssembly contains the failure. A program's memory belongs to its
instance, and the program can't address anything outside it. When a run
does something it can't continue from, such as allocating past its memory
limit, recursing past its stack, burning its fuel, or any other WebAssembly
trap, the instance stops and that run ends with an error. The process and
every other run carry on. Four programs sent to one server, one after
another:

| The program | What came back |
| --- | --- |
| Allocates a million megabyte strings | `memory exhausted` |
| Recurses without end | `call stack exhausted` |
| Loops without end | `fuel exhausted` |
| Returns a string | `still serving` |

Second, the server doesn't panic. Submilli's runtime is written so that no
program can crash it. The parser, the compiler, the runtime, and every
function a program can call return an error instead of panicking, even for
a state that "can't happen", and input size and nesting are bounded before
they can exhaust the process. A program that breaks one of the runtime's
own assumptions gets an error, and the server keeps running. Every change to the
runtime is held to this rule, and the path a program takes through
the server has been reviewed against it.

## MCP

The server exposes MCP, the protocol agent frameworks use to call tools,
with one endpoint per blueprint: `/mcp/<blueprint>`. Point your harness at
it and the agent gets Submilli as a set of tools: run a program, look up
packages, read the session's files. An application that would rather build
those tools itself, as the quickstart's did, uses the HTTP API instead.

## Availability

A session lives on one server. Its record and files are on that server's
disk, so the server needs no database beside it. Run
several servers for capacity, and route each session to the server that
opened it. A restart keeps every session. While a server is down, its
sessions wait for it, and the others keep serving.

Submilli Enterprise is fault tolerant. To learn more, email us at
[hello@submilli.ai](mailto:hello@submilli.ai).

## In production

The server is the part of Submilli you run in production, and the part
your application or harness connects to. It runs beside your application:
as a process on the same machine, as a container with Docker Compose, as a
pod in your Kubernetes cluster with the Helm chart, or on a container host
such as Render.

Next: [your application](/docs/application), which opens the session
and hands the agent its tools.
