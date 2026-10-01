---
title: Execution model
description: How code execution moves tool orchestration out of the model loop.
slug: concepts/execution-model
sidebar:
  order: 1
prev: false
next: false
---

An agent investigating failed payments needs to fetch records, identify affected
accounts, and find relevant support tickets. With sequential tool calls, each
result goes back to the model before it chooses the next request. With code
execution, the model writes a program to coordinate that work and receives a
focused report.

The introduction above follows this one task. Read its
[complete transcript](/docs/videos/code-execution-introduction/) or browse the
[video library](/docs/videos/).

## What changes when the agent writes a program

The example moves from four model rounds to two: one to produce the program and
one to interpret its result. The program handles intermediate records and tool
coordination. This is an illustrative sequence, not a benchmark or a guarantee
that every task needs two rounds. Errors, retries, and additional reasoning may
require more.

Code execution does not decide which actions the agent should be allowed to take.
The runtime and its configuration still need to enforce those boundaries. Read
[how Submilli works](/docs/how-submilli-works/) for the execution mechanism and
[blueprints](/docs/blueprints/) for the rules you configure.

## Continue learning

- [Introduction](/docs/introduction/): why Submilli and where it fits.
- [Quickstart](/docs/quickstart/): install the CLI and run your first program.
