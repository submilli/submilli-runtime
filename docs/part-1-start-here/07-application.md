---
title: "Your application"
description: "How the application or agent framework connects: it opens a session under a blueprint, binds the variables, and hands the agent its tools; what the agent gets back."
slug: application
sidebar:
  order: 7
---

Your application, or the agent framework it uses, is the **harness**: the
code that runs the agent's loop. It sends the conversation to a model,
carries out the tool calls the model asks for, and sends the results back.
It may be Mastra, LangChain, or a loop you wrote yourself. Submilli replaces
none of it. The harness keeps the model and the loop, and gains one thing:
a tool that takes a program the model wrote and runs it under your
blueprint.

The quickstart's application was that tool in miniature: forty lines that
sent a program to the server with a blueprint's name and a customer id, and
printed the result. A real harness does the same for a model instead of a
file, and does it once per conversation.

There are two ways to connect. Most harnesses speak MCP, the protocol agent
frameworks use to call tools, and over MCP the server hands the harness its
tools ready-made. A harness that doesn't, or an application that wants full
control, builds the same tools on the server's HTTP API, as the quickstart's
application did.

## Three decisions the model has no part in

Whatever the harness, connecting comes down to three things your
application decides.

**The address names the blueprint.** The MCP endpoint is
`http://127.0.0.1:8128/mcp/<blueprint>`. A harness connected to
`/mcp/quickstart` runs every program under that blueprint. The model can't
choose another, because the blueprint isn't an argument of any tool.

**A header binds the variables.** A blueprint that requires a variable, such
as the quickstart's `customerId`, gets it from the harness when it
connects:

```text
submilli-variables: customerId=cus_northwind
```

The server checks the values against the blueprint before it accepts the
connection, and refuses one that leaves out a required variable. The value
must come from what your application knows, such as the signed-in user.
Never take it from the conversation: anything there could have been written
by the model or by someone instructing it.

**One connection is one session.** The variables are bound for the life of
the connection, which is also the life of the session, and so are the
session's files and state. Open a connection per user and close it when the
conversation ends.

## What the agent gets

Connected, the model gets Submilli as a set of tools. The ones that matter:

- **Execute**: compiles and runs a program.
- **Last run**: returns the last run again, with everything it logged.
- **Search**: finds packages the blueprint allows, by name, description,
  or function.
- **Docs**: returns one package's documentation and declarations.

The harness supplies no system prompt for Submilli. The instructions that
teach a model the language arrive as the description of the execute tool,
with this blueprint's packages and permissions already filled in. What you
supply is the agent's own brief: what it is for, and how to work. This is
the brief the book's research agent runs with, from [Connect a
harness](/docs/tutorials/connect-a-harness):

```text title="prompt.txt"
You are a research assistant. You answer questions by searching the web
and reading pages, and you keep a notebook so the next conversation can
start from what this one learned.

## Work in programs

Do the work by writing and running TypeScript on Submilli. Read `docs`
before using an unfamiliar package or API: the runtime is not Node.js,
has no shell, and has no npm packages. Prefer one coherent program for
related reads, filtering, and summaries, and return the evidence the
answer needs, not whole pages. Keep predictable follow-up steps inside
the same program: a URL or an id one call returns is used by the next
call in code, not in another turn. When a program fails, read the
diagnostic and repair it. A permission denial is final; do not look for
another route to the same effect.

## Resources

- `@submilli/jina`: web search, and reading a page as clean text.
- `submilli:llm`: a model you may call from a program, to summarize a
  long page or rank results without bringing the text back here.
- `submilli:fs`: your notebook, the directory /notes, where every
  program starts. Your first program lists it, reads what is there, and
  gets today's date from `Temporal.Now.plainDateISO()`, before any search. When you are done,
  write what you learned, with its sources, updating an existing note
  rather than replacing what it got right. Use no other file tool for it.

## Answer

Prefer the newest source and check its date against today's before you
call something the latest. Lead with the answer and cite the pages you
used. Distinguish what the
evidence establishes from what you infer and what remains unknown. Never
claim you read or saved something unless a program's result shows it.
```

Four things, and nothing about the language: what the agent is for; how
to work in programs rather than one call at a time, including that a
denial is final; what it may reach, in the words the model will see in
`docs`; and how to answer. The second part is the one that changes how
an agent behaves on Submilli: a model told to fetch, filter, and join in
one program does the work in a few runs instead of a few dozen.

The execute tool answers with three fields:

```json
{ "result": "2 charges, 6150 cents", "console": [], "error": null }
```

`result` is what `main` returned. `console` is empty after a successful run,
to keep logs out of the conversation; the last-run tool returns them when
the model wants them. A program that fails, whether it doesn't compile,
throws, or is denied, is also an ordinary answer, with `error` set and
`console` holding whatever the program logged before it stopped:

```json
{
  "result": null,
  "console": ["2 charges, 6150 cents"],
  "error": {
    "kind": "runtime_error",
    "message": "error: PermissionDeniedError: permission denied: caller=main capability=acme.com/charges.list: policy denied acme.com/charges.list for main. …"
  }
}
```

The model reads the message as it would any tool result, so it can correct
a compile error and try again, and can accept a denial instead of retrying
it.

## Where to go from here

To go deeper into the three components:

- [Blueprints](/docs/blueprints/start-a-blueprint): grant operations,
  declare secrets and variables, allow HTTP, files, Git, and models, add
  MCP servers, install packages.
- [Packages](/docs/packages/start-a-project): start a project, write
  operations, call a service, document, test, publish.
- [Server](/docs/server/run-the-server): run it, register blueprints,
  operate MCP servers, set limits, deploy.

Or jump straight to embedding Submilli:
[connect your harness](/docs/tutorials/connect-a-harness), then
[deploy on Kubernetes](/docs/server/deploy-on-kubernetes).
