---
title: "Connect Mastra"
description: "Run the research agent on Mastra: its programs executed on the server as the signed-in user, then one real conversation watched end to end."
slug: tutorials/connect-mastra
sidebar:
  order: 3
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "bcebe2fc7c4a150d35c0eaf21be510263d5533f650a7847cb68646821ca2ab0a"
  confirmedAt: "2026-10-05T10:59:51.482Z"
---

In this tutorial we will run the research agent on Mastra, with its programs
executed on the server as the signed-in user (`u_ada` in the examples). You need the server and the `research`
blueprint from [Connect a harness](/docs/tutorials/connect-a-harness),
with `SUBMILLI_SERVER_TOKEN` still exported, Node.js 20 or later, and a
key from your model provider for the conversation. The agent file
names Claude. For Google or OpenAI, the `model` argument takes
`google/gemini-3.8-flash` or `openai/gpt-4o-mini` instead, with that
provider's key in the environment.

## Start the project

In `harnesses`, make a directory for this harness and install the
dependencies. The files use top-level `await`, hence `type=module`. The
full project is
[`examples/harnesses/mastra/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/mastra).

```sh
mkdir mastra && cd mastra
npm init -y
npm pkg set type=module
npm install @mastra/core @mastra/mcp
npm install --save-dev tsx typescript @types/node
```

## The agent

Save this as `agent.ts`. It reads the brief from `../prompt.txt`:

```typescript title="agent.ts"
// A Mastra agent that runs its programs on submilli-server, over MCP.

import { readFileSync } from "node:fs";
import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

type Model = ConstructorParameters<typeof Agent>[0]["model"];

// The agent's brief, kept beside the blueprint.
const INSTRUCTIONS = readFileSync(new URL("../prompt.txt", import.meta.url), "utf8");

export async function answer(
  question: string,
  userId: string,
  model: Model = "anthropic/claude-sonnet-5",
): Promise<string> {
  const agent = new Agent({
    id: "researcher",
    name: "Researcher",
    instructions: INSTRUCTIONS,
    model,
  });

  // One client per user: the binding is fixed when the client connects.
  const submilli = new MCPClient({
    id: `submilli-${userId}`,
    servers: {
      submilli: {
        url: new URL(`${SUBMILLI_SERVER}/mcp/${BLUEPRINT}`),
        requestInit: {
          headers: {
            Authorization: `Bearer ${serverToken()}`,
            "submilli-variables": `userId=${userId}`,
          },
        },
      },
    },
  });

  try {
    const toolsets = await submilli.listToolsets();
    // Mastra logs a refused connection and carries on with no tools.
    if (toolsets.submilli === undefined) throw new Error("submilli-server refused the connection");

    const result = await agent.generate(question, { toolsets, maxSteps: 20 });
    return result.text;
  } finally {
    await submilli.disconnect();
  }
}

/** The API token this application was given for the server. */
function serverToken(): string {
  const token = process.env.SUBMILLI_SERVER_TOKEN;
  if (!token) throw new Error("SUBMILLI_SERVER_TOKEN is not set: export the token the server was started with");
  return token;
}

if (import.meta.filename === process.argv[1]) {
  // In a real application the user comes from the signed-in session.
  console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada"));
}
```

Notice where the tools go. They go to `generate`, as `toolsets`, per
request, and not to the agent when it is constructed. An agent is usually built
once and shared, and tools given to it would carry one user's binding
into another's conversation.

Keep the check on `toolsets.submilli`. When the server refuses the
connection, Mastra logs the error and returns no tools, and the model
would answer without them. `maxSteps` bounds the loop. The agent stops
after twenty rounds of tool calls whether or not it has an answer.

## One conversation

Now the real thing:

```sh
ANTHROPIC_API_KEY=... npx tsx agent.ts
```

This is one real run, with Claude Sonnet 5 as the model. The programs
are the model's own, unedited. Another run, or another model, writes
different ones, so watch for the shape. The application asks, on behalf
of `u_ada`:

```text
What is new in the latest stable release of Rust? Save a note with your sources.
```

The model starts where the brief tells it to, with the files tool on
`/notes`, which is empty, and a program that asks the server for
today's date:

```typescript
function main(): string {
  return Temporal.Now.plainDateISO().toString();
}
```

```text
2026-10-03
```

Then it reads the documentation of `@submilli/jina` and runs two small
programs. One searches, and the other reads the release post it found.
This is the first:

```typescript
import jina from "@submilli/jina";

function main(): string {
  const results = jina.searchJson("Rust 1. release notes blog.rust-lang.org 2025");
  const lines: string[] = [];
  for (const r of results) {
    lines.push(r.title + " — " + r.url + " — " + r.description);
  }
  return lines.join("\n---\n");
}
```

Its first draft of the note doesn't compile:

```text
error: `+` not defined for `string` and `number`
  --> <execute>:61:10
   |
60 |   fs.writeText("/notes/rust-latest-release.md", content);
61 |   return "Saved note to /notes/rust-latest-release.md (" + content.length + " chars)";
   |          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
62 | }
   |
help: `+` does not coerce; wrap the number with `String(...)` before concatenating
```

A compile error is a result like any other. The model wraps the number
in `String(...)`, as the diagnostic says, and the note is written:

```text
Saved note to /notes/rust-latest-release.md (2695 chars)
```

Then it answers the user:

```text
## Rust 1.99.0 — the latest stable release (shipped 2026‑10‑01)
…
Saved a note at `/notes/rust-latest-release.md` with these details and sources for future sessions.
```

Notice a few things. The model can't know today's date, so it asked
the server before searching, and the date lets it tell the newest
release from an old announcement that ranks well. The programs never
named the user. They wrote to `/notes`, and the session's binding
decided that `/notes` is `u_ada`'s. And the note is a file on the
server's volume, there for the next conversation `u_ada` opens, on this
harness or another. The run took nine tool calls, within the twenty
steps the agent allows.

## With your coding agent

With the [skill](/docs/install#the-skill) installed, your coding
assistant does this for an application you already have. The project
was a small Mastra app with no Submilli in it, made of an agent and an
HTTP handler that takes the signed-in user from an `x-user-id` header set by
the company's login proxy.

```text
Connect this app's Mastra agent to the research blueprint on my local Submilli server, so it does its research by running programs there, as the signed-in user.
```

The assistant adds `@mastra/mcp` and writes a `research` function that
follows this page. It opens a new `MCPClient` for each request, puts the
user in the `submilli-variables` header, passes the tools to `generate`
as `toolsets`, checks that the Submilli toolset loaded, and calls
`disconnect` in a `finally`. The handler answers 502 when the tools are
missing. The assistant noticed unprompted that the user id ends up in
the blueprint's path filters, so it accepts ids of letters, digits, and
`_ . @ -` alone. Then it says it guessed that format and asks what your
ids look like.

It tests with a real MCP client and no model. The eight tools load and
the user's own directory can be read. Another user's directory, the
volume's root, and a look-alike directory that starts with the user's id
are refused, and a connection with no user gets no tools. It reports that
it did not run a model. Run afterwards, the app answered the
question above in about a minute and saved its note under `/u_ada`.

You have the research agent running on Mastra, each program it writes
executed on the server as the signed-in user, and the binding proved on the index
before any model was involved. Project:
[`examples/harnesses/mastra/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/mastra).
