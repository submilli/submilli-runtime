---
title: "Use the HTTP API"
description: "Run the research agent on the Vercel AI SDK with no MCP client: the tools built by hand on the server's HTTP API, the blueprint and variables fixed in your code, then a real conversation."
slug: next/tutorials/use-the-http-api
pagefind: false
sidebar:
  order: 7
  hidden: true
---

The HTTP API offers the same operations as plain requests. Use it when
your harness has no MCP client, or when you want to decide which tools
the model gets and what they are called. The cost is that you build the
tools yourself, and one thing about them is not yours to write: the
execute tool's description. It is the text that teaches the model the
language, with this blueprint's packages and rules filled in, and the
server publishes it for you to fetch.

In this tutorial we will run the research agent on the Vercel AI SDK
with no MCP client: the tools built on the server's HTTP API, the
blueprint and the signed-in user, `u_ada` in the examples, fixed in our
own code, then one real conversation. You need the server and the `research`
blueprint from [Connect a harness](/docs/next/tutorials/connect-a-harness),
with `SUBMILLI_SERVER_TOKEN` still exported, Node.js 20 or later, and a
key from your model provider for the conversation. The agent file
imports `@ai-sdk/anthropic`; for Google or OpenAI, `@ai-sdk/google` or
`@ai-sdk/openai` takes its place, with that provider's key in the
environment.

## Start the project

In `harnesses`, make a directory for this harness and install the
dependencies; the files use top-level `await`, hence `type=module`. The
full project is
[`examples/harnesses/vercel-ai-sdk-http/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/vercel-ai-sdk-http).

```sh
mkdir vercel-ai-sdk-http && cd vercel-ai-sdk-http
npm init -y
npm pkg set type=module
npm install ai @ai-sdk/anthropic zod
npm install --save-dev tsx typescript @types/node
```

## The agent

The agent looks like the ones on the other pages, with `openSession` in
place of an MCP client. Save it as `agent.ts`; it reads the brief from
`../prompt.txt`:

```typescript title="agent.ts"
// A Vercel AI SDK agent that runs its programs on submilli-server, over HTTP.

import { readFileSync } from "node:fs";
import { anthropic } from "@ai-sdk/anthropic";
import { generateText, stepCountIs, type LanguageModel } from "ai";
import { openSession } from "./submilli.ts";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";

function instructions(userId: string): string {
  // The agent's brief, kept beside the blueprint; `{userId}` names the user.
  const brief = readFileSync(new URL("../prompt.txt", import.meta.url), "utf8");
  return brief.replaceAll("{userId}", userId);
}

export async function answer(
  question: string,
  userId: string,
  model: LanguageModel = anthropic("claude-haiku-4-5"),
): Promise<string> {
  const submilli = await openSession({
    server: SUBMILLI_SERVER,
    token: serverToken(),
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

`stopWhen` matters: the SDK stops after one step by default, which would
end the run on the tool call, before the model has seen the result. The
example is written for version 7 of the SDK.

## The tools

`openSession` is the part you write. It makes two requests, then builds
six tools, one for each of the MCP tools the HTTP API can serve. Watch
the first request, `GET /v1/blueprints/research/prompt`: its answer is
the execute tool's description, and the descriptions of the other five.
Over MCP the server hands these to the harness; over HTTP you fetch them
and pass them through unchanged:

```typescript title="submilli.ts"
// The tools a model needs to write and run programs on submilli-server, built
// on the server's HTTP API for the Vercel AI SDK.

import { tool, type ToolSet } from "ai";
import { z } from "zod";

export interface SessionOptions {
  /** Base URL of submilli-server. */
  server: string;
  /** An API token the server accepts. It stays in your application. */
  token: string;
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
  // Every request carries the token, and a request with a body sends it as JSON.
  const call = (url: string, method = "GET", body?: unknown): Promise<Response> =>
    fetch(url, {
      method,
      headers: {
        Authorization: `Bearer ${options.token}`,
        ...(body === undefined ? {} : { "content-type": "application/json" }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });

  const blueprint = `${options.server}/v1/blueprints/${encodeURIComponent(options.blueprint)}`;
  const describe: Prompt = await json(await call(`${blueprint}/prompt`));

  const { session_id } = await json(
    await call(`${options.server}/v1/sessions`, "POST", {
      blueprint: options.blueprint,
      variables: options.variables ?? {},
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
          const run = await json(await call(`${session}/execute`, "POST", { code }));
          // A denial or a compile error arrives here as `error`, for the model to read.
          return { result: run.result, console: run.console, error: run.error };
        },
      }),
      submilli__typescript__last_run: tool({
        description: describe.tools.last_run,
        inputSchema: z.object({}),
        execute: async () => json(await call(`${session}/last-run`)),
      }),
      submilli__typescript__packages__search: tool({
        description: describe.tools.search,
        inputSchema: z.object({ query: z.string().default("") }),
        execute: async ({ query }) =>
          json(await call(`${blueprint}/packages/search?${new URLSearchParams({ q: query })}`)),
      }),
      submilli__typescript__packages__docs: tool({
        description: describe.tools.docs,
        inputSchema: z.object({ name: z.string() }),
        execute: async ({ name }) => {
          const docs = await call(`${blueprint}/packages/docs?${new URLSearchParams({ name })}`);
          return docs.text();
        },
      }),
      submilli__typescript__builtins__list: tool({
        description: describe.tools.builtins_list,
        inputSchema: z.object({}),
        execute: async () => json(await call(`${blueprint}/builtins`)),
      }),
      submilli__typescript__builtins__docs: tool({
        description: describe.tools.builtins_docs,
        inputSchema: z.object({ names: z.array(z.string()) }),
        execute: async ({ names }) => {
          const query = new URLSearchParams(names.map((name) => ["name", name]));
          return json(await call(`${blueprint}/builtins/docs?${query}`));
        },
      }),
    },
    close: async () => {
      await call(session, "DELETE");
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

Notice `description: describe.prompt` on the execute tool. That text is
the model's whole instruction in the language: the subset of TypeScript
it may write, the modules this blueprint lets it import, its filesystem
and hosts, and what to do with a denial. Write your own description
there and the model writes Node.js, imports packages the blueprint
doesn't list, and retries denials. Fetch it from the server, per
blueprint, and never cache it across blueprint changes. The second
request opens the session. That is where the blueprint
and the variables are fixed, the step the address and the header perform
over MCP. A missing variable is refused there with HTTP 400, and an
unknown blueprint with 404.

Notice the execute tool's schema: `code`, and nothing else. The model
fills in the arguments of a tool, so an argument named `blueprint` or
`userId` would hand the model that choice. Keep both in your own code, as
here.

A failed program still answers HTTP 200, with `error` set as it is over
MCP and the session's id beside it. Only a request the server can't act
on, such as an unknown session, gets an error status.

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

A secret the blueprint declares with a `harness` source goes in the
request that opens the session, beside the variables:

```json
{ "blueprint": "research", "variables": { "userId": "u_ada" }, "secrets": { "USER_TOKEN": "tok_…" } }
```

Over MCP the same object goes base64url-encoded in a `submilli-secrets`
header. A session opened without a required secret is refused with HTTP
400. The server keeps these values in memory only, so a session outlives
a server restart but its secrets don't: a program then answers HTTP 409
with `session_requires_secrets` until the harness supplies them again,
with `POST /v1/sessions/{id}/rebind`, or over MCP a new connection.

The HTTP API has no counterpart to the two file tools. `POST
/v1/execute`, which the quickstart used, takes the blueprint and
variables with the code and runs the program in a session that ends when
the program returns. It suits a single run. An agent needs the session
endpoints, so that one program can build on the state of the last.

## One conversation

```sh
ANTHROPIC_API_KEY=... npx tsx agent.ts
```

This is one real run, with Claude Haiku 4.5 as the model. The model's
programs are its own, and another run writes different ones; the answer
ended:

```text
**Source:** I've saved a complete note with citations to `/u_ada/notes/rust_latest.md` containing all details and the official Rust release documentation link: https://doc.rust-lang.org/beta/releases.html (published October 3, 2026)
```

The note is a file on the server's volume, there for the next
conversation `u_ada` opens, on this harness or any other.

You have the research agent running on the Vercel AI SDK with tools of
your own on the HTTP API, the blueprint and the user fixed where the
model can't reach them, and the binding proved on the index before
any model was involved. Project:
[`examples/harnesses/vercel-ai-sdk-http/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/vercel-ai-sdk-http).
