# Mastra

Read [harnesses](harnesses.md) first. Keep the application's existing Mastra
agent, model, memory, and workflow. Submilli is the policy and execution
boundary; Mastra remains the model loop.

Connect with `@submilli/mastra`: `createSubmilliTools()` gives a Mastra agent
Submilli's tools and runs every program on submilli-server over the HTTP API,
with one server session per memory thread. Use the MCP route ("Alternative:
MCP with @mastra/mcp", below) only for an application that already loads
Submilli through `@mastra/mcp`.

## Before writing the harness

For a new project, follow [setup](setup.md), then [packages](packages.md),
then [blueprints](blueprints.md); the "Submilli setup" section below lists what
the `support-read` fixture needs. The package needs Node.js 22 or later,
`@mastra/core` 1.79 or later, and submilli-server 0.4.1 or later. Inspect the
installed versions first (`npm ls @mastra/core @mastra/memory @submilli/mastra`).
These examples were verified on 2026-11-01 with `@submilli/mastra 0.1.0`,
`@mastra/core 1.81.0`, `@mastra/memory 1.40.0`, and submilli-server 0.5.1; if
an export or signature differs, the installed declarations win over this text.
The package README is the API reference:
[@submilli/mastra](https://github.com/submilli/submilli-typescript/tree/main/packages/mastra#readme).

## Complete minimal project

```sh
npm install @submilli/mastra @mastra/core @mastra/memory
npm install -D typescript tsx @types/node
export OPENAI_API_KEY='...'
export SUBMILLI_SERVER='http://127.0.0.1:8128'
# SUBMILLI_SERVER_TOKEN must already hold the token the server was started with.
```

The package reads `SUBMILLI_SERVER`, not the `SUBMILLI_SERVER_URL` the MCP
recipe and the fixture's export line use; set it to the fixture's URL when
testing against it.

```ts
// src/mastra.ts
import { Mastra } from '@mastra/core';
import { Agent } from '@mastra/core/agent';
import { InMemoryStore } from '@mastra/core/storage';
import { Memory } from '@mastra/memory';
import { createSubmilliTools } from '@submilli/mastra';

// Once per process. Server and token come from SUBMILLI_SERVER and
// SUBMILLI_SERVER_TOKEN. The customer is the memory resource of each agent
// run, which the application sets from its authenticated session.
export const submilli = createSubmilliTools({
  blueprint: process.env.SUBMILLI_BLUEPRINT ?? 'support-read',
  variables: ({ resourceId }) => {
    // Replace this with the application's canonical customer-id validator.
    if (!resourceId || !/^[A-Za-z0-9_-]+$/.test(resourceId)) {
      throw new Error('run the agent with an authenticated memory resource');
    }
    return { customerId: resourceId };
  },
});

export const memory = new Memory();

export const mastra = new Mastra({
  agents: {
    assistant: new Agent({
      id: 'support-assistant',
      name: 'Support assistant',
      instructions: 'Use the Submilli tools for business data; never bypass them with shell, HTTP, or SDK calls.',
      // Configure the host provider here. Example: OPENAI_API_KEY plus this model.
      model: process.env.MASTRA_MODEL ?? 'openai/gpt-5.5',
      memory,
      tools: () => submilli.tools(),
    }),
  },
  // A real deployment uses persistent storage (LibSQLStore, PostgresStore), so
  // threads, and the Submilli session each one keeps, survive restarts.
  storage: new InMemoryStore(),
});
```

```ts
// src/main.ts
import { mastra, memory, submilli } from './mastra.ts';

const customerId = 'pied_piper'; // demo only: take it from the signed-in session
const threadId = `customer:${customerId}`;

try {
  const result = await mastra.getAgent('assistant').generate(
    `Read the balance for customer ${customerId} and report the amount.`,
    { memory: { thread: threadId, resource: customerId }, maxSteps: 4 },
  );
  for (const { payload } of result.toolResults) {
    const executionId = (payload.result as { execution_id?: string } | undefined)?.execution_id;
    if (executionId) console.error('program run:', executionId);
  }
  console.log(result.text);
} finally {
  // When the thread is deleted or archived, not after every run.
  await submilli.close({ threadId, memory });
}
```

Run it with `npx tsx src/main.ts`. The model should call
`submilli__typescript__execute` with a program that imports
`@pied-piper/billing`, and report `6150`. That is evidence of a live model run
only when a provider key is configured; a scripted model proves wiring only.

What the package does, and the rules that follow from it:

- **Identity comes from the agent run.** `variables` (and `secrets` for
  `harness` secrets) are fixed values or functions of the run's
  `{ resourceId, threadId, requestContext }`, set by the application, never by
  the model. Build the tools and the agent once and share them; do not build
  one per user or put the identity in a header.
- **One session per memory thread.** The first program on a thread opens a
  server session and keeps it in the thread's metadata, so later runs, other
  instances on the same storage, and restarts find it. Bind variables to
  something the thread belongs to, such as its resource: Mastra already refuses
  a run on a thread owned by another resource, and a run whose variables differ
  from the thread's session gets `SubmilliSessionMismatchError` as the tool's
  error, and nothing runs. End the session with
  `submilli.close({ threadId, memory })` when the thread is deleted or archived,
  or the Blueprint's `idle_timeout` ends it. A run without a memory thread gets
  a session that no later run can find.
- **Six tools.** The model gets `submilli__typescript__execute`, `__last_run`,
  `__packages__search`, `__packages__docs`, `__builtins__list` and
  `__builtins__docs`, with the descriptions the server writes for the Blueprint.
  Mastra tools added to the same agent are called directly and bypass Submilli.
- **Retries are safe.** Each execute sends an `Idempotency-Key` derived from
  Mastra's `toolCallId`. The output carries `execution_id` and `session_id` for
  the audit trail; the model is not shown them.
- **Policy denials are results.** A denied capability returns `denied: true`
  and `error.kind` `permission_denied`, naming the capability. A wrong token,
  an unknown Blueprint, invalid variables, and an unreachable server throw, and
  Mastra reports them to the model as tool errors.
- **Application tools.** `ptc: { ... }` lets programs call the application's
  own Mastra tools as the `@tools` Package; the Blueprint must grant
  `tools.call`. A call that needs approval (`requireApproval` on the tool, or
  `requireToolApproval`) suspends the agent run, and `resumeGenerate` carries
  it on; `onApprovalRequired` decides in code instead. See the README's "Your
  tools in programs" and "Approvals".

For `stream()`, pass the same `memory` option. There is no client to
disconnect per request: the session belongs to the thread, so never close it
while a stream is still running. Use an abort signal and a small `maxSteps`
bound for request limits.

## Deterministic verification with a scripted model

Before a live run, keep Mastra, its memory, `@submilli/mastra` and the local
server real, and script only the model with `MockLanguageModelV4` from
`ai/test` (pass it as the agent's `model`): one `submilli__typescript__execute`
call, then a final text step that quotes the tool result. Use a fresh thread per
case and `close()` each thread at the end. Assert:

1. On a thread whose resource is `pied_piper`, a program reading `pied_piper`
   returns `result` `"6150"`. The offered execute tool's input schema has only
   `code`; no Blueprint or identity argument reaches the model.
2. The same thread reading `hooli` returns a tool result with `"denied":true`
   and `pied-piper.example/balance.read`; nothing throws. The thread still has
   one session (`submilli.session({ threadId, memory })`).
3. Two concurrent runs on threads of different customers each get their own
   result.
4. Tools built without `variables` make the first execute a tool error naming
   `customerId`. Tools built with a `token` the server does not know reject
   `tools()` with `AuthenticationError` (HTTP 401).
5. `close({ threadId, memory })` returns `true` for each thread that ran a
   program.

`skills/evals/harnesses/mastra/verify-package.ts` runs this matrix against the
fixture. A scripted run is adapter and policy evidence, not a live model result.

## Submilli setup

Follow [installation and projects](setup.md), [packages](packages.md), and
[blueprints](blueprints.md) in order. The [shared harness setup](harnesses.md)
contains the tested package, blueprint, and server commands;
reuse it when validating this recipe rather than copying a partial scaffold.
The blueprint must declare required `customerId`, allow
`pied-piper.example/balance.read` only when `customerId == ${vars.customerId}`, and use
`default: deny`. Over MCP, the URL is
`http://127.0.0.1:8128/mcp/support-read`; send `SUBMILLI_SERVER_TOKEN` as
`Authorization: Bearer …` and bind the trusted value with
`submilli-variables: customerId=pied_piper`.
Variables constrain generated code; they are not authentication. Put the
server behind trusted ingress before exposing it outside the host application.

## Alternative: MCP with @mastra/mcp

An application that already loads Submilli through `@mastra/mcp` can keep
doing so; the package's session, idempotency, and binding rules then become the
application's job.

### MCP integration shape

For fixed, shared MCP identity, `await client.listTools()` can be supplied as
`tools` when constructing an agent. For an authenticated request or tenant,
create a fresh `MCPClient`, put the token and the trusted identity in `requestInit.headers`,
call `await client.listToolsets()`, and pass that result as `toolsets` to the
existing agent's `generate()` or `stream()` call. Do not cache customer tool
closures in a global agent or mutate headers on a shared client. Current
official API names are `listTools()` and `listToolsets()`; older examples may
use `getTools()` or use `listTools()` for runtime credentials.

```ts
import { MCPClient } from '@mastra/mcp';
import { mastra } from './mastra';

const agent = mastra.getAgent('assistant');
// The API token the application was given for the server.
const token = process.env.SUBMILLI_SERVER_TOKEN;
if (!token) throw new Error('SUBMILLI_SERVER_TOKEN is not set');

export async function handleRequest(prompt: string, customerId: string) {
  // Derive this value from authenticated, authorized application state.
  if (!/^cus_[a-z0-9_]+$/.test(customerId) || /[;\r\n]/.test(customerId)) {
    throw new Error('invalid customer identity');
  }

  const client = new MCPClient({
    id: `submilli-${customerId}`,
    servers: {
      submilli: {
        url: new URL('http://127.0.0.1:8128/mcp/support-read'),
        requestInit: {
          headers: {
            Authorization: `Bearer ${token}`,
            'submilli-variables': `customerId=${customerId}`,
          },
        },
      },
    },
  });

  try {
    const toolsets = await client.listToolsets();
    const result = await agent.generate(prompt, { toolsets, maxSteps: 4 });
    return result.text;
  } finally {
    await client.disconnect();
  }
}
```

For `stream()`, keep the `finally` around consumption of the returned stream,
not around stream creation. Cleanup must happen after completion or
cancellation. Use an abort signal and a small `maxSteps` bound for request
limits. Mastra tool approval can add an approval UX, but it does not replace
Submilli package checks or blueprint filters.

### Complete minimal MCP project

This host-side skeleton uses no business SDK: the only remote tool server is
Submilli. Use the application's existing `@mastra/core` and `@mastra/mcp`,
or the current releases for a new project; `@mastra/mcp` declares a peer
range on `@mastra/core`, so install both together and let the package
manager resolve the pair. The skeleton below was verified on 2026-09-17
with `@mastra/core 1.67.0` and `@mastra/mcp 1.18.0`; the versions in it are
what was tested, not a recommendation.

```json
{
  "type": "module",
  "scripts": { "start": "tsx src/main.ts", "typecheck": "tsc --noEmit" },
  "dependencies": {
    "@mastra/core": "1.67.0",
    "@mastra/mcp": "1.18.0",
    "@types/node": "22.15.21",
    "tsx": "4.19.2",
    "typescript": "5.8.3"
  }
}
```

```json
{ "compilerOptions": { "target": "ES2022", "module": "NodeNext", "moduleResolution": "NodeNext", "strict": true, "skipLibCheck": true } }
```

```ts
// src/mastra.ts
import { Agent } from '@mastra/core/agent';
import { Mastra } from '@mastra/core/mastra';

export const mastra = new Mastra({
  agents: {
    assistant: new Agent({
      id: 'support-assistant',
      name: 'Support assistant',
      instructions: 'Use the available Submilli tools. Answer with the returned value.',
      // Configure the host provider here. Example: OPENAI_API_KEY plus this model.
      model: 'openai/gpt-5.5',
    }),
  },
});
```

```ts
// src/main.ts
import { MCPClient } from '@mastra/mcp';
import { mastra } from './mastra.js';

const serverUrl = process.env.SUBMILLI_SERVER_URL ?? 'http://127.0.0.1:8128';
const token = process.env.SUBMILLI_SERVER_TOKEN;
if (!token) throw new Error('SUBMILLI_SERVER_TOKEN is not set');
const customerId = process.env.DEMO_CUSTOMER_ID ?? 'pied_piper';
if (!/^cus_[a-z0-9_]+$/.test(customerId) || /[;\r\n]/.test(customerId)) {
  throw new Error('invalid DEMO_CUSTOMER_ID');
}

const client = new MCPClient({
  id: `demo-${customerId}`,
  servers: {
    submilli: {
      url: new URL(`${serverUrl}/mcp/support-read`),
      requestInit: {
        headers: { Authorization: `Bearer ${token}`, 'submilli-variables': `customerId=${customerId}` },
      },
    },
  },
});

try {
  const toolsets = await client.listToolsets();
  const agent = mastra.getAgent('assistant');
  const result = await agent.generate(`Read the balance for authorized customer ${customerId}.`, {
    toolsets,
    maxSteps: 4,
  });
  console.log(result.text);
} finally {
  await client.disconnect();
}
```

Install and run from that project directory:

```sh
npm install
# Set the provider key only for a real model run; this guide does not perform one.
export OPENAI_API_KEY=...
export SUBMILLI_SERVER_URL=http://127.0.0.1:8128
# SUBMILLI_SERVER_TOKEN must already hold the token the server was started with.
npm run typecheck
npm start
```

The deterministic setup can run without a model key by replacing the model
with the application's typed mock model. Do not replace `MCPClient`, toolsets,
or the Submilli server with a fake SDK interface: mocks are appropriate only at
the model boundary. Expected output from the support-read fixture is `6150`
(or a short model response containing that value). A model response is not a
policy test; inspect the executed tool and run the deterministic matrix below.

### MCP identity, cleanup, and failure checks

The demo identity is a fixture value supplied by an environment variable.
Production code must derive the ID after authentication and authorization,
validate its format, and create one client per request or identity. Never
accept a customer ID from the prompt as authority.

Run these deterministic checks before a live model task:

1. `pied_piper` discovers the expected tool and reads `6150`.
2. The same binding passing `hooli` is denied by the package capability
   filter: `tool.execute` resolves with `result: null` and an `error.message`
   naming `PermissionDeniedError` and `pied-piper.example/balance.read`.
3. Omitting `submilli-variables` is rejected at connect: the server answers
   `initialize` with HTTP 400, `@mastra/mcp` then tries its SSE fallback, and
   `listToolsets()` logs the failure and returns no `submilli` toolset. Treat
   an absent toolset as the failure; do not expect a tool-level error.
4. An unlisted capability or raw `http.get` from `main` is denied.
5. A second client for another customer has an independent header and tool
   closure; no global tool cache is reused.
6. `disconnect()` runs on success, discovery failure, model failure, and stream
   cancellation. For a stream, await completion/cancellation before calling it.

Common failures are an old sample using `listTools()` for per-user
credentials, a shared client whose headers were mutated between requests, a
missing required variable, applying the blueprint to a different server, and
closing the client before a stream finishes. Current `@mastra/mcp` also has
`listToolsetsWithErrors()` for discovery diagnostics.

Official references: [@submilli/mastra README](https://github.com/submilli/submilli-typescript/tree/main/packages/mastra#readme),
[Mastra MCP integration](https://mastra.ai/docs/connections/mcp),
[`MCPClient` API](https://mastra.ai/reference/tools/mcp-client), and
[Submilli quickstart](https://submilli.ai/docs/quickstart).
