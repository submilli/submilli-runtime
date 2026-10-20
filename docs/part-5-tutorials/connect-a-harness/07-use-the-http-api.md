---
title: "Use the HTTP API"
description: "Build Submilli tools for a Vercel AI SDK agent directly on the server's HTTP API."
slug: tutorials/use-the-http-api
docType: how-to
sidebar:
  order: 7
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "c4f10a455982ed9696b30916bf26279d92ad2aaf849f200263295c30dffbb2d8"
  confirmedAt: "2026-10-20T09:28:54Z"
---

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/tutorials/use-the-http-api/ as the source for this setup.

Inspect my project and follow this guide to create agent.ts and submilli.ts.
Reuse valid local Submilli connection settings and provider credentials. If
SUBMILLI_SERVER, SUBMILLI_SERVER_TOKEN, or SUBMILLI_BLUEPRINT is missing, ask
for that value or its local configuration source. Do not invent values,
expose secrets, create a Blueprint, or change the server. Ask for missing
provider credentials without printing them. Install only the
missing dependencies shown in this guide. Preserve its tool restrictions and
session cleanup. Run the file with --check to verify the connection without
a model request.
Leave the files ready for the exact Try it out command.
```

After setup, [try it out](#try-it-out).

## Set up manually

The files read this terminal's environment; they do not automatically load
`.env` files. Use your running server's base URL and registered Blueprint.
Replace these values before running the files:

```sh
export SUBMILLI_SERVER='your-server-base-url'
export SUBMILLI_SERVER_TOKEN='your-server-token'
export SUBMILLI_BLUEPRINT='your-registered-blueprint'
export OPENAI_API_KEY='your-openai-api-key'
```

Use the HTTP API when a harness has no MCP client or when your application
needs to build and name the model's tools itself. This guide uses the Vercel AI
SDK. The application fetches Submilli's tool descriptions, opens a session,
and closes it after the conversation.

Before you start:

- [Run `submilli-server`](/docs/server/run-the-server) and keep
  `SUBMILLI_SERVER_TOKEN` exported.
- [Register a Blueprint](/docs/server/register-a-blueprint), then set
  `SUBMILLI_BLUEPRINT` to its registered name. The calculation below assumes
  the Blueprint has no required variables. The sample calls no external tools.
- Install Node.js 22 or later and set the API key for your model provider.

### Start the project

```sh
mkdir vercel-ai-submilli && cd vercel-ai-submilli
npm init -y
npm pkg set type=module
npm install ai @ai-sdk/openai zod
npm install --save-dev tsx typescript @types/node
```

### Connect the agent

Create `agent.ts` in the project directory and paste the code below:

```typescript title="agent.ts"
import { openai } from "@ai-sdk/openai";
import { generateText, stepCountIs } from "ai";
import { openSession } from "./submilli.ts";

const args = process.argv.slice(2);
const checking = args.length === 1 && args[0] === "--check";
const prompt = args.join(" ");
if (!checking && !prompt.trim()) {
  throw new Error("Usage: npx tsx agent.ts '<message>' (or --check)");
}

const submilli = await openSession({
  server: process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128",
  token: required("SUBMILLI_SERVER_TOKEN"),
  blueprint: required("SUBMILLI_BLUEPRINT"),
});

try {
  if (checking) {
    console.log("Connected to Submilli.");
  } else {
    const result = await generateText({
      // Change this model ID to use another OpenAI model.
      model: openai("gpt-5-mini"),
      system: "Use Submilli to do calculations in TypeScript. Return the execution result.",
      tools: submilli.tools,
      prompt,
      stopWhen: stepCountIs(10),
      onStepFinish({ toolCalls, toolResults }) {
        if (toolCalls.length > 0) console.dir({ toolCalls, toolResults }, { depth: null });
      },
    });
    console.log(result.text);
  }
} finally {
  await submilli.close();
}

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
}
```

`stopWhen` lets the SDK continue after a tool result. The default stops after
one step, before the model can use that result. `onStepFinish` prints the tool
calls and results so you can verify execution separately from the final answer.

### Build the tools

Create `submilli.ts` beside `agent.ts` and paste the code below:

```typescript title="submilli.ts"
import { tool, type ToolSet } from "ai";
import { z } from "zod";

export interface SessionOptions {
  server: string;
  token: string;
  /** Name of a Blueprint already registered on this server. */
  blueprint: string;
  /** Trusted application values required by the Blueprint. */
  variables?: Record<string, string>;
  /** Harness-supplied secrets required by the Blueprint. */
  secrets?: Record<string, string>;
}

export interface Session {
  tools: ToolSet;
  close(): Promise<void>;
}

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
  const call = (url: string, method = "GET", body?: unknown): Promise<Response> =>
    fetch(url, {
      method,
      headers: {
        Authorization: `Bearer ${options.token}`,
        ...(body === undefined ? {} : { "content-type": "application/json" }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });

  const blueprint =
    `${options.server}/v1/blueprints/${encodeURIComponent(options.blueprint)}`;
  const describe: Prompt = await json(await call(`${blueprint}/prompt`));

  const { session_id } = await json(
    await call(`${options.server}/v1/sessions`, "POST", {
      blueprint: options.blueprint,
      variables: options.variables ?? {},
      secrets: options.secrets ?? {},
    }),
  );
  const session = `${options.server}/v1/sessions/${session_id}`;

  return {
    tools: {
      submilli__typescript__execute: tool({
        description: describe.prompt,
        inputSchema: z.object({ code: z.string() }),
        execute: async ({ code }) => {
          const run = await json(await call(`${session}/execute`, "POST", { code }));
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
          const response = await call(
            `${blueprint}/packages/docs?${new URLSearchParams({ name })}`,
          );
          if (!response.ok) throw new Error(await response.text());
          return response.text();
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
      const response = await call(session, "DELETE");
      if (!response.ok) throw new Error(`could not close session: HTTP ${response.status}`);
    },
  };
}

async function json(response: Response): Promise<any> {
  const body = await response.json();
  if (!response.ok) {
    throw new Error(
      `submilli-server answered ${response.status}: ${body.message ?? body.error}`,
    );
  }
  return body;
}
```

The execute tool accepts only `code`. The application fixes the Blueprint,
variables, and secrets when it opens the session, outside the model's control.
Fetch the execute description from the server: it describes the supported
TypeScript, Packages, built-ins, and selected Blueprint. The `finally` block
closes the session even when generation fails.

This wrapper passes only the six `submilli__typescript__*` tools it builds to
`generateText`; it does not declare an application-tool catalog in the `tools`
field of `POST /v1/sessions`, and it does not handle `suspended` responses or
`POST /v1/runs/{run_id}/resume`. Direct Vercel AI SDK tools can still be added
when the application needs them, but those calls bypass Submilli and execute
through the harness's own path. For a governed migration,
declare the application tools in `POST /v1/sessions`, grant `tools.call` in the
Blueprint, execute the callback in the harness, and resume the run with
`POST /v1/runs/{run_id}/resume`. See the [application tools section](/docs/reference/http-api#application-tools)
for the suspended-run and result contract.

If your Blueprint has required variables or harness secrets, pass them in
`openSession({ variables, secrets })` from trusted application state. The
[HTTP API reference](/docs/reference/http-api#post-v1sessions) defines
validation, rebinding after a restart, failures, and the complete session
contract.

### Check the connection

With the setup environment active, run this without a model request:

```sh
npx tsx agent.ts --check
```

Look for `Connected to Submilli.`. This checks the server and registered
Blueprint; the model runs only when you supply a message.

## Try it out

```sh
npx tsx agent.ts \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
```

In the printed step data, confirm a call to
`submilli__typescript__execute` and a tool result containing `4`. The
final assistant text alone does not verify where the calculation ran. This
sample uses the model provider for the Vercel AI SDK conversation. The
TypeScript program itself only computes a value and calls no Package,
application tool, or external service.

See the [HTTP API reference](/docs/reference/http-api) for endpoint schemas,
execute results, idempotency, errors, and application-tool callbacks. See [How
Submilli works with harnesses](/docs/tutorials/connect-a-harness) for the
boundary between the harness, Submilli, and the selected Blueprint.
