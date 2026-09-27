---
title: "Connecting to your harness"
description: "Connecting an agent framework to submilli-server over MCP, with examples for Mastra, LangChain deepagents, the OpenAI Agents SDK, and the Claude Agent SDK, and building the same tools on the HTTP API."
slug: harness
sidebar:
  order: 10
---

A server is running and a blueprint is registered on it. What remains is the
agent. Your **harness** is the code that runs the agent's loop: it sends the
conversation to a model, carries out the tool calls the model asks for, and
sends the results back. It may be a framework such as Mastra or a loop you
wrote yourself. Submilli replaces none of it. The harness keeps the model and
the loop, and gains one thing: a tool that takes a program the model wrote and
runs it under your blueprint.

There are two ways to connect. Most harnesses speak MCP, the protocol agent
frameworks use to call tools, and over MCP the server hands the harness its
tools ready-made. A harness that doesn't, or an application that wants full
control, builds the same tools on the server's HTTP API.

The chapter builds one agent five times. Read the next three sections, which
apply to every harness, then go to yours:

| Harness | Language | Connects over |
| --- | --- | --- |
| [Mastra](#mastra) | TypeScript | MCP |
| [LangChain deepagents](#langchain-deepagents) | Python | MCP |
| [OpenAI Agents SDK](#openai-agents-sdk) | Python | MCP |
| [Claude Agent SDK](#claude-agent-sdk) | TypeScript | MCP |
| [Vercel AI SDK](#without-mcp-the-http-api) | TypeScript | HTTP |

Every example is a complete project in the repository under
[`examples/harnesses/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses), with a check that runs it without a model.

## The example: a research agent with a notebook

The agent answers questions by searching the web and reading pages, and keeps
notes so that the next conversation can start from what the last one learned.
Each user has a notebook of their own. Three things in the blueprint make
that work:

```yaml title="blueprint.yaml (fragment)"
variables:
  userId:
    required: true

packages:
- '@submilli/jina'

vfs:
  mode: persistent
  volume: notes

default: deny

permissions:
  main:
  - capability: jina.ai/search
    action: allow
  - capability: jina.ai/read
    action: allow
  - capability: fs.read
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
  - capability: fs.write
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
  - capability: fs.list
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
  - capability: fs.stat
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
  - capability: fs.mkdir
    filter: path == "/${vars.userId}" or path glob "/${vars.userId}/*"
    action: allow
```

`@submilli/jina` is the [curated package](/docs/curated-packages) for web
search and page reading. The `notes` volume is a directory on the server that
outlives sessions.

The five file rules carry one filter, which confines a program to the
directory named after the user the session was opened for. The filter has two
halves because a path rule matches exactly what it names: the `glob` covers
everything inside `/u_ada`, and the `==` covers `/u_ada` itself, which a
program asks about when it checks that the directory exists. Anything the
rules don't name falls to `default: deny`, so a program can't remove or move a
file, and can't list the volume's root.

The fragment leaves out the package's own permissions and the secret. The
whole file is
[`blueprint.yaml`](https://github.com/submilli/submilli-runtime/blob/main/examples/harnesses/blueprint.yaml).

The server needs the volume, a secret store for Jina's API key, and the
package. From `examples/harnesses/`:

```sh
mkdir -p "$HOME/submilli-notes"
printf 'volumes:\n  notes: %s\n' "$HOME/submilli-notes" > server.yaml
head -c 32 /dev/urandom | base64 > store.key

submilli-server --config server.yaml --secret-store-key-file store.key &

submilli server packages install submilli/submilli-runtime @submilli/jina
submilli server secret put jina_api_key
submilli server blueprint apply blueprint.yaml
```

`secret put` prompts for the key; Jina issues one at
[jina.ai](https://jina.ai). [Submilli server](/docs/server) explains each of
these commands.

## What an MCP connection fixes: blueprint, variables, session

Whatever the harness, connecting over MCP comes down to three decisions your
application makes and the model has no part in.

**The address names the blueprint.** The MCP endpoint is
`http://127.0.0.1:8128/mcp/<blueprint>`. A harness connected to
`/mcp/research` runs every program under that blueprint. The model can't
choose another, because the blueprint isn't an argument of any tool.

**A header binds the variables.** The `research` blueprint requires a
variable, `userId`. The harness sends it when it connects:

```text
submilli-variables: userId=u_ada
```

Several variables are separated by `;`, so a value can't contain one. The
server checks them against the blueprint before it accepts the connection. It
refuses a variable the blueprint doesn't declare, and refuses a connection
that leaves out a required one with HTTP 400 and
`invalid variables: required variable 'userId' was not supplied`. The value
must come from what your application knows, such as the signed-in user. Never
take it from the conversation: anything there could have been written by the
model or by someone instructing it.

**One connection is one session.** The variables are bound for the life of
the connection, which is also the life of the session. Session state lasts
that long. Files last as long as the blueprint's `vfs` says: here they are on
a volume and outlive the session, while under the default, `ephemeral`, each
program starts with an empty directory. Open a connection per user and close
it when the conversation ends. Closing the client sends the request that ends
the session. A connection that just drops is cleaned up after the blueprint's
`idle_timeout`.

The harness supplies no system prompt for Submilli. The instructions that
teach a model the language arrive as the description of the execute tool, with
this blueprint's packages and permissions already filled in. What the
examples do supply is the agent's own brief: what it is for, and where its
notebook is.

## One conversation, start to finish

This is one real run of the Mastra example, with Gemini 3.8 Flash as the
model. The programs are the model's own, unedited. Another run, or another
model, writes different ones.

The application asks, on behalf of the user `u_ada`:

```text
What is new in the latest stable release of Rust? Save a note with your sources.
```

The model starts by finding out what it can import. It calls the package
search tool, then asks for the documentation of `@submilli/jina` and
`submilli:fs`. Then it looks for notes from an earlier conversation, and
oversteps:

```typescript
import * as fs from "submilli:fs";

export function main(): string {
  const rootEntries: string[] = [];
  for (const entry of fs.list("/", false)) {
    rootEntries.push(entry.name + " (" + entry.kind + ")");
  }
  return rootEntries.join(", ");
}
```

The volume's root holds every user's directory, and the blueprint allows
`u_ada` only her own. The execute tool answers:

```text
error: PermissionDeniedError: permission denied: caller=main capability=fs.list: policy denied fs.list for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
```

The model moves on to the directory it was given, finds it empty, and
searches:

```typescript
import jina from "@submilli/jina";
import * as fs from "submilli:fs";

function main(): string {
    // 1. Check existing notes if any
    let existingNotes = "";
    if (fs.exists("/u_ada/notes")) {
        const iter = fs.list("/u_ada/notes", true);
        const files: string[] = [];
        for (const entry of iter) {
            files.push(entry.path);
        }
        existingNotes = "Existing notes files: " + files.join(", ");
    } else {
        existingNotes = "No existing notes directory.";
    }

    // 2. Search for the latest Rust release
    const results = jina.searchJson("Rust release announcement blog.rust-lang.org 2025 2026");
    const summary: string[] = [existingNotes, "--- Search Results ---"];
    for (const r of results) {
        summary.push(`Title: ${r.title}\nURL: ${r.url}\nDesc: ${r.description}\n`);
    }

    return summary.join("\n\n");
}
```

The result names the Rust blog. Over the next programs the model reads the
blog's index, then the two newest release posts in one program:

```typescript
import jina from "@submilli/jina";

function main(): string {
    const r1981 = jina.read("https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/");
    const r1980 = jina.read("https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/");
    
    return `=== RUST 1.98.1 ===\n${r1981}\n\n=== RUST 1.98.0 ===\n${r1980.slice(0, 4000)}`;
}
```

Its last program writes the note with `fs.writeText`, to
`/u_ada/notes/rust_latest_stable.md`, and the model answers the user:

```text
The latest stable release of Rust is **Rust 1.98.1** (released September 3, 2026),
following the major feature release **Rust 1.98.0** (released August 20, 2026).

A research note with sources has been saved to `/u_ada/notes/rust_latest_stable.md`.
…
### Sources
* [Rust Blog: Announcing Rust 1.98.1](https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/)
* [Rust Blog: Announcing Rust 1.98.0](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/)
```

The note is a file on the server's volume, 3,059 bytes, there for the next
conversation `u_ada` opens.

The run took 19 tool calls: three for discovery and sixteen programs. This
model worked in small steps, reading one thing per program. That is why the
examples allow twenty steps; with eight, the same run stopped before it wrote
the note and returned an empty answer.

## The eight MCP tools and the execute result

Connected, the model sees eight tools.

| Tool | What it does |
| --- | --- |
| `submilli__typescript__execute` | Compiles and runs a program; takes `code` |
| `submilli__typescript__last_run` | Returns the last run again, with everything it logged |
| `submilli__typescript__packages__search` | Finds packages the blueprint allows, by name, description, or function |
| `submilli__typescript__packages__docs` | Returns one package's documentation and type declarations |
| `submilli__typescript__builtins__list` | Lists the built-in types and namespaces |
| `submilli__typescript__builtins__docs` | Returns the declarations of the built-ins named |
| `submilli__files__list` | Lists the files the blueprint's `vfs` holds |
| `submilli__files__read` | Reads one of them, a range of lines at a time |

The execute tool answers with three fields:

```json
{ "result": "Note written successfully to /u_ada/notes/rust_latest_stable.md. Bytes: 3059", "console": [], "error": null }
```

`result` is what `main` returned. `console` is empty after a successful run,
to keep logs out of the conversation; the last-run tool returns them when the
model wants them. A program that fails, whether it doesn't compile, throws, or
is denied, is also an ordinary answer, with `error` set and `console` holding
whatever the program logged before it stopped:

```json
{
  "result": null,
  "console": [],
  "error": {
    "kind": "runtime_error",
    "message": "error: PermissionDeniedError: permission denied: caller=main capability=fs.list: policy denied fs.list for main. …"
  }
}
```

The model reads the message as it would any tool result, so it can correct a
compile error and try again, and can accept a denial instead of retrying it,
as it did in the run above.

## Mastra

```sh
npm install @mastra/core @mastra/mcp
```

```typescript title="agent.ts"
// A Mastra agent that runs its programs on submilli-server, over MCP.

import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

type Model = ConstructorParameters<typeof Agent>[0]["model"];

function instructions(userId: string): string {
  return [
    "You are a research assistant. Search the web and read pages by writing programs for Submilli.",
    "Do the whole job in one program where you can, and return only what you need to answer.",
    `Keep a note of what you learn, with its sources, under /${userId}/notes.`,
    "Read your earlier notes before you search again.",
  ].join(" ");
}

export async function answer(
  question: string,
  userId: string,
  model: Model = "google/gemini-3.8-flash",
): Promise<string> {
  const agent = new Agent({
    id: "researcher",
    name: "Researcher",
    instructions: instructions(userId),
    model,
  });

  // One client per user: the binding is fixed when the client connects.
  const submilli = new MCPClient({
    id: `submilli-${userId}`,
    servers: {
      submilli: {
        url: new URL(`${SUBMILLI_SERVER}/mcp/${BLUEPRINT}`),
        requestInit: { headers: { "submilli-variables": `userId=${userId}` } },
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

if (import.meta.filename === process.argv[1]) {
  // In a real application the user comes from the signed-in session.
  console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada"));
}
```

```sh
GOOGLE_GENERATIVE_AI_API_KEY=... npx tsx agent.ts
```

The tools go to `generate` as `toolsets`, per request, and not to the agent
when it is constructed. An agent is usually built once and shared, and tools
given to it would carry one user's binding into another's conversation.

Keep the check on `toolsets.submilli`. When the server refuses the
connection, Mastra logs the error and returns no tools, and the model would
answer without them. `maxSteps` bounds the loop: the agent stops after twenty
rounds of tool calls whether or not it has an answer.

Project: [`examples/harnesses/mastra/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/mastra).

## LangChain deepagents

```sh
pip install deepagents langchain-google-genai langchain-mcp-adapters
```

```python title="agent.py"
"""A LangChain deepagents agent that runs its programs on submilli-server, over MCP."""

import asyncio
import os

from deepagents import create_deep_agent
from deepagents.middleware.filesystem import FilesystemPermission
from langchain_mcp_adapters.sessions import create_session
from langchain_mcp_adapters.tools import load_mcp_tools

SUBMILLI_SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
BLUEPRINT = "research"


def instructions(user_id: str) -> str:
    return (
        "You are a research assistant. Search the web and read pages by writing programs for Submilli. "
        "Do the whole job in one program where you can, and return only what you need to answer. "
        f"Keep a note of what you learn, with its sources, under /{user_id}/notes. "
        "Read your earlier notes before you search again."
    )


async def answer(question: str, user_id: str, model="google_genai:gemini-3.8-flash") -> str:
    submilli = {
        "transport": "streamable_http",
        "url": f"{SUBMILLI_SERVER}/mcp/{BLUEPRINT}",
        "headers": {"submilli-variables": f"userId={user_id}"},
    }

    # One session per user: the binding is fixed when the session opens.
    async with create_session(submilli) as session:
        await session.initialize()
        agent = create_deep_agent(
            model=model,
            tools=await load_mcp_tools(session),
            system_prompt=instructions(user_id),
            # deepagents has file tools of its own, which keep files in the
            # conversation. Deny them, so that notes go through Submilli.
            permissions=[FilesystemPermission(operations=["read", "write"], paths=["/**"], mode="deny")],
        )
        result = await agent.ainvoke({"messages": [{"role": "user", "content": question}]})
        return result["messages"][-1].text


if __name__ == "__main__":
    # In a real application the user comes from the signed-in session.
    print(asyncio.run(answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada")))
```

```sh
GOOGLE_API_KEY=... python agent.py
```

`create_session` opens one connection, which the tools share for as long as
the block runs, so the agent is built and run inside it. Tools loaded without
a session, as `MultiServerMCPClient.get_tools()` does, open a new connection
for every call, and each program would run in a session of its own and find
none of the state the last one left.

deepagents gives its agent a planning tool and file tools of its own, which
keep files in the conversation's state. Those files are not the notebook:
they never reach Submilli and are gone when the conversation ends. Run
without the `permissions` line, a model asked to save a note used those tools,
and the note was never written. The line denies them. An agent built
with LangChain's `create_agent` or with LangGraph takes the same tools.

Project: [`examples/harnesses/deepagents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/deepagents).

## OpenAI Agents SDK

```sh
pip install openai-agents
```

```python title="agent.py"
"""An OpenAI Agents SDK agent that runs its programs on submilli-server, over MCP."""

import asyncio
import os

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp

SUBMILLI_SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
BLUEPRINT = "research"


def instructions(user_id: str) -> str:
    return (
        "You are a research assistant. Search the web and read pages by writing programs for Submilli. "
        "Do the whole job in one program where you can, and return only what you need to answer. "
        f"Keep a note of what you learn, with its sources, under /{user_id}/notes. "
        "Read your earlier notes before you search again."
    )


async def answer(question: str, user_id: str, model=None) -> str:
    # One connection per user: the binding is fixed when it opens.
    async with MCPServerStreamableHttp(
        name="submilli",
        params={
            "url": f"{SUBMILLI_SERVER}/mcp/{BLUEPRINT}",
            "headers": {"submilli-variables": f"userId={user_id}"},
        },
    ) as submilli:
        agent = Agent(
            name="researcher",
            instructions=instructions(user_id),
            model=model,
            mcp_servers=[submilli],
        )
        result = await Runner.run(agent, question, max_turns=20)
        return result.final_output


if __name__ == "__main__":
    # In a real application the user comes from the signed-in session.
    print(asyncio.run(answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada")))
```

```sh
OPENAI_API_KEY=... python agent.py
```

The SDK connects when the `async with` block opens and ends the session when
it closes. `model=None` leaves the choice to the SDK's default; pass a model
name to choose one. `max_turns` bounds the loop.

Project: [`examples/harnesses/openai-agents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/openai-agents).

## Claude Agent SDK

```sh
npm install @anthropic-ai/claude-agent-sdk
```

```typescript title="agent.ts"
// A Claude Agent SDK agent that runs its programs on submilli-server, over MCP.

import { query, type Options } from "@anthropic-ai/claude-agent-sdk";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

function instructions(userId: string): string {
  return [
    "You are a research assistant. Search the web and read pages by writing programs for Submilli.",
    "Do the whole job in one program where you can, and return only what you need to answer.",
    `Keep a note of what you learn, with its sources, under /${userId}/notes.`,
    "Read your earlier notes before you search again.",
  ].join(" ");
}

export function options(userId: string): Options {
  return {
    systemPrompt: instructions(userId),
    mcpServers: {
      // One entry per user: the binding is fixed when the agent connects.
      submilli: {
        type: "http",
        url: `${SUBMILLI_SERVER}/mcp/${BLUEPRINT}`,
        headers: { "submilli-variables": `userId=${userId}` },
      },
    },
    // Only the server above, whatever else the account or machine has configured.
    strictMcpConfig: true,
    allowedTools: ["mcp__submilli__*"],
    // No shell, no file tools, no web fetch: every action goes through Submilli.
    tools: [],
    settingSources: [],
    maxTurns: 20,
  };
}

export async function answer(question: string, userId: string): Promise<string> {
  for await (const message of query({ prompt: question, options: options(userId) })) {
    if (message.type === "result") {
      return message.subtype === "success" ? message.result : `stopped: ${message.subtype}`;
    }
  }
  throw new Error("the agent ended without a result");
}

if (import.meta.filename === process.argv[1]) {
  // In a real application the user comes from the signed-in session.
  console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada"));
}
```

```sh
ANTHROPIC_API_KEY=... npx tsx agent.ts
```

This SDK gives its agent a shell, file access, and web fetch unless told
otherwise, and those tools run outside Submilli, where no blueprint applies.
`tools: []` removes them, and `allowedTools` lets the agent call Submilli's
without asking.

The SDK also loads the MCP servers of whoever runs the process. Run under a
Claude login that had two connectors attached, this agent started with 84
tools, eight of them Submilli's. `strictMcpConfig` limits it to the servers
named here, and `settingSources: []` keeps that person's other settings out.

The SDK takes its credentials from `ANTHROPIC_API_KEY`, from Amazon Bedrock or
Google Vertex AI when `CLAUDE_CODE_USE_BEDROCK` or `CLAUDE_CODE_USE_VERTEX` is
set, or from a Claude Code login on the machine.

The same caution holds for every harness. A blueprint governs what programs
do. It can't govern a tool the harness offers beside Submilli's.

Project: [`examples/harnesses/claude-agent-sdk/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/claude-agent-sdk).

## Without MCP: the HTTP API

The HTTP API offers the same operations as plain requests. Use it when your
harness has no MCP client, or when you want to decide which tools the model
gets and what they are called. The cost is that you build the tools yourself.
This example does it for the Vercel AI SDK.

```sh
npm install ai @ai-sdk/google zod
```

The agent looks like the ones above, with `openSession` in place of an MCP
client:

```typescript title="agent.ts"
// A Vercel AI SDK agent that runs its programs on submilli-server, over HTTP.

import { google } from "@ai-sdk/google";
import { generateText, stepCountIs, type LanguageModel } from "ai";
import { openSession } from "./submilli.ts";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";

function instructions(userId: string): string {
  return [
    "You are a research assistant. Search the web and read pages by writing programs for Submilli.",
    "Do the whole job in one program where you can, and return only what you need to answer.",
    `Keep a note of what you learn, with its sources, under /${userId}/notes.`,
    "Read your earlier notes before you search again.",
  ].join(" ");
}

export async function answer(
  question: string,
  userId: string,
  model: LanguageModel = google("gemini-3.8-flash"),
): Promise<string> {
  const submilli = await openSession({
    server: SUBMILLI_SERVER,
    blueprint: "research",
    variables: { userId },
  });

  try {
    const { text } = await generateText({
      model,
      system: instructions(userId),
      tools: submilli.tools,
      prompt: question,
      stopWhen: stepCountIs(20),
    });
    return text;
  } finally {
    await submilli.close();
  }
}

if (import.meta.filename === process.argv[1]) {
  // In a real application the user comes from the signed-in session.
  console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada"));
}
```

```sh
GOOGLE_GENERATIVE_AI_API_KEY=... npx tsx agent.ts
```

`stopWhen` matters: the SDK stops after one step by default, which would end
the run on the tool call, before the model has seen the result. The example
is written for version 7 of the SDK.

`openSession` is the part you write. It makes two requests, then builds six
tools, one for each of the MCP tools the HTTP API can serve:

```typescript title="submilli.ts"
// The tools a model needs to write and run programs on submilli-server, built
// on the server's HTTP API for the Vercel AI SDK.

import { tool, type ToolSet } from "ai";
import { z } from "zod";

export interface SessionOptions {
  /** Base URL of submilli-server. */
  server: string;
  /** The registered blueprint every program in this session runs under. */
  blueprint: string;
  /** Values for the blueprint's variables, from your application's own state. */
  variables?: Record<string, string>;
}

export interface Session {
  /** Hand these to `generateText` or `streamText`. */
  tools: ToolSet;
  /** Ends the session and wipes its files. */
  close(): Promise<void>;
}

/** The descriptions the server publishes for its MCP tools, fetched over HTTP. */
interface Prompt {
  prompt: string;
  tools: {
    search: string;
    docs: string;
    builtins_list: string;
    builtins_docs: string;
    last_run: string;
  };
}

export async function openSession(options: SessionOptions): Promise<Session> {
  const blueprint = `${options.server}/v1/blueprints/${encodeURIComponent(options.blueprint)}`;
  const describe: Prompt = await json(await fetch(`${blueprint}/prompt`));

  const { session_id } = await json(
    await fetch(`${options.server}/v1/sessions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ blueprint: options.blueprint, variables: options.variables ?? {} }),
    }),
  );
  const session = `${options.server}/v1/sessions/${session_id}`;

  return {
    tools: {
      // The model supplies the code and nothing else. The blueprint and the
      // variables were fixed above, where the model cannot reach them.
      submilli__typescript__execute: tool({
        description: describe.prompt,
        inputSchema: z.object({ code: z.string() }),
        execute: async ({ code }) => {
          const run = await json(
            await fetch(`${session}/execute`, {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({ code }),
            }),
          );
          // A denial or a compile error arrives here as `error`, for the model to read.
          return { result: run.result, console: run.console, error: run.error };
        },
      }),
      submilli__typescript__last_run: tool({
        description: describe.tools.last_run,
        inputSchema: z.object({}),
        execute: async () => json(await fetch(`${session}/last-run`)),
      }),
      submilli__typescript__packages__search: tool({
        description: describe.tools.search,
        inputSchema: z.object({ query: z.string().default("") }),
        execute: async ({ query }) =>
          json(await fetch(`${blueprint}/packages/search?${new URLSearchParams({ q: query })}`)),
      }),
      submilli__typescript__packages__docs: tool({
        description: describe.tools.docs,
        inputSchema: z.object({ name: z.string() }),
        execute: async ({ name }) => {
          const docs = await fetch(`${blueprint}/packages/docs?${new URLSearchParams({ name })}`);
          return docs.text();
        },
      }),
      submilli__typescript__builtins__list: tool({
        description: describe.tools.builtins_list,
        inputSchema: z.object({}),
        execute: async () => json(await fetch(`${blueprint}/builtins`)),
      }),
      submilli__typescript__builtins__docs: tool({
        description: describe.tools.builtins_docs,
        inputSchema: z.object({ names: z.array(z.string()) }),
        execute: async ({ names }) => {
          const query = new URLSearchParams(names.map((name) => ["name", name]));
          return json(await fetch(`${blueprint}/builtins/docs?${query}`));
        },
      }),
    },
    close: async () => {
      await fetch(session, { method: "DELETE" });
    },
  };
}

async function json(response: Response): Promise<any> {
  const body = await response.json();
  if (!response.ok) {
    throw new Error(`submilli-server answered ${response.status}: ${body.message ?? body.error}`);
  }
  return body;
}
```

The first request fetches the text the MCP tools would have carried:
`prompt`, the execute tool's description, and `tools`, the descriptions of
the others. The second opens the session. That is where the blueprint and
the variables are fixed, the step the address and the header performed over
MCP. A missing variable is refused there with HTTP 400, and an unknown
blueprint with 404.

The execute tool's schema decides what the model can choose. The model fills
in the arguments of a tool, so an argument named `blueprint` or `userId`
would hand the model that choice. Keep both in your own code, as here.

A failed program still answers HTTP 200, with `error` set as it is over MCP
and the session's id beside it. Only a request the server can't act on, such
as an unknown session, gets an error status.

| Request | Serves |
| --- | --- |
| `POST /v1/sessions` | Opens a session; takes `blueprint`, `variables`, `secrets` |
| `POST /v1/sessions/{id}/execute` | Runs `code` in the session |
| `GET /v1/sessions/{id}/last-run` | The last run, with its logs |
| `DELETE /v1/sessions/{id}` | Ends the session |
| `GET /v1/blueprints/{name}/prompt` | The tool descriptions |
| `GET /v1/blueprints/{name}/packages/search?q=` | Package search |
| `GET /v1/blueprints/{name}/packages/docs?name=` | One package's documentation |
| `GET /v1/blueprints/{name}/builtins` | The list of built-ins |
| `GET /v1/blueprints/{name}/builtins/docs?name=&name=` | Declarations of the built-ins named |

The HTTP API has no counterpart to the two file tools. `POST /v1/execute`,
which the quickstart used, takes the blueprint and variables with the code and
runs the program in a session that ends when the program returns. It suits a
single run. An agent needs the session endpoints, so that one program can
build on the state of the last.

Project: [`examples/harnesses/vercel-ai-sdk-http/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/vercel-ai-sdk-http).

## Credentials that belong to the session

Some credentials belong to the user, not to the server: the user's own
access token for a service, say. The blueprint declares such a secret with a
`harness` source, and the harness supplies the value when it opens the
session.

```sh
submilli blueprint secret add USER_TOKEN --harness --required
```

Over HTTP the value goes in the request that opens the session:

```json
{ "blueprint": "research", "variables": { "userId": "u_ada" }, "secrets": { "USER_TOKEN": "tok_…" } }
```

Over MCP it goes in a second header, `submilli-secrets`, holding the same JSON
object encoded as base64url. A session opened without a required secret is
refused with HTTP 400. The server keeps these values in memory for the life of
the session and never writes them to disk. A session therefore outlives a
server restart but its secrets don't, and running a program in it answers
HTTP 409 with `session_requires_secrets` until the harness supplies them
again. `POST /v1/sessions/{id}/rebind` does that, and replaces a token that
has expired; over MCP, open a new connection.

## Giving the agent an MCP server

Your harness may already call tools on MCP servers. Left in the harness,
those tools stay outside the blueprint. Declared in the blueprint, a server
becomes a package that programs import, and the blueprint decides which of
its tools may run. [Using MCP servers](/docs/mcp-servers) covers the
declaration, credentials, and logins. Here the agent gets a browser, through
Playwright's MCP server:

```sh
submilli blueprint add-mcp playwright http://localhost:8931/mcp
submilli blueprint capability remove mcp.playwright
submilli blueprint capability add mcp.playwright
submilli server blueprint apply blueprint.yaml
```

Nothing changes in the harness. This is the Mastra agent from above, the
same file, asked for something that takes a browser:

```text
Open https://example.com in the browser, follow its link, and tell me the title of the page you land on.
```

The model reads the documentation of `@mcp/playwright`, takes a snapshot of
the page to find the link, and then writes this:

```typescript
import * as fs from "submilli:fs";
import playwright from "@mcp/playwright";
function main(): string {
  playwright.browser_navigate({ url: "https://example.com" });
  
  // Click the link with target "e6" (from snapshot)
  const clickRes = playwright.browser_click({ target: "e6" });
  
  // Snapshot after click
  const snapRes = playwright.browser_snapshot({});
  // Let's also evaluate document.title
  const evalRes = playwright.browser_evaluate({ function: "() => document.title" });
  return JSON.stringify({
    clickRes,
    snapRes,
    evalRes
  });
}
```

It saves a note, closes the browser, and answers:

```text
After opening `https://example.com` and following its "Learn more" link (which directs to
`https://iana.org/domains/example`, redirecting to `https://www.iana.org/help/example-domains`),
the title of the page you land on is:
**"Example Domains"**
```

Four browser actions ran in one program and one round trip to the model. A
harness calling the MCP server directly would have gone back to the model
after each.

The blueprint above allows every tool the server has, including
`browser_evaluate`, which runs any JavaScript in the page. In an earlier run
the blueprint allowed only `browser_navigate`, `browser_snapshot`, and
`browser_click`. The model's program called `browser_evaluate`, was denied,
and the model reported the denial and stopped. Allow the tools the task
needs, and expect a model to reach for the others.

Next: [deploying](/docs/deploying), which puts the server in a container.
