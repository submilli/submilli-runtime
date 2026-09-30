# Mastra

Read [harnesses](harnesses.md) first. Keep the application's existing Mastra
agent, model, memory, and workflow. Submilli is the policy and execution
boundary; Mastra remains the model loop.

## Safe integration shape

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

## Complete minimal project

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
      model: 'openai/gpt-4o-mini',
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
const customerId = process.env.DEMO_CUSTOMER_ID ?? 'cus_northwind';
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

## Submilli setup

Follow [installation and projects](setup.md), [packages](packages.md), and
[blueprints](blueprints.md) in order. The [shared harness setup](harnesses.md)
contains the tested package, blueprint, server, and MCP discovery commands;
reuse it when validating this recipe rather than copying a partial scaffold.
The blueprint must declare required `customerId`, allow
`acme.com/balance.read` only when `customerId == ${vars.customerId}`, and use
`default: deny`. The MCP URL is
`http://127.0.0.1:8128/mcp/support-read`; send `SUBMILLI_SERVER_TOKEN` as
`Authorization: Bearer …` and bind the trusted value with
`submilli-variables: customerId=cus_northwind`.
Variables constrain generated code; they are not authentication, and the token
authenticates the application, not its user. Add TLS and restricted ingress
before exposing the server outside the host application.

## Identity, cleanup, and failure checks

The demo identity is a fixture value supplied by an environment variable.
Production code must derive the ID after authentication and authorization,
validate its format, and create one client per request or identity. Never
accept a customer ID from the prompt as authority.

Run these deterministic checks before a live model task:

1. `cus_northwind` discovers the expected tool and reads `6150`.
2. The same binding passing `cus_initech` is denied by the package capability
   filter: `tool.execute` resolves with `result: null` and an `error.message`
   naming `PermissionDeniedError` and `acme.com/balance.read`.
3. Omitting `submilli-variables` (token still sent) is rejected at connect:
   the server answers `initialize` with HTTP 400, `@mastra/mcp` then tries its
   SSE fallback, and `listToolsets()` logs the failure and returns no
   `submilli` toolset. Omitting the token is a `401` with the same outcome and
   no OAuth flow; `listToolsetsWithErrors()` tells the two apart. Treat an
   absent toolset as the failure; do not expect a tool-level error.
4. An unlisted capability or raw `http.get` from `main` is denied.
5. A second client for another customer has an independent header and tool
   closure; no global tool cache is reused.
6. `disconnect()` runs on success, discovery failure, model failure, and stream
   cancellation. For a stream, await completion/cancellation before calling it.

Common failures are an old sample using `listTools()` for per-user
credentials, a shared client whose headers were mutated between requests, a
missing required variable, a missing or unknown token in the harness, applying the blueprint to a different server, and
closing the client before a stream finishes. Current `@mastra/mcp` also has
`listToolsetsWithErrors()` for discovery diagnostics.

Official references: [Mastra MCP integration](https://mastra.ai/docs/connections/mcp),
[`MCPClient` API](https://mastra.ai/reference/tools/mcp-client), and
[Submilli quickstart](https://submilli.ai/docs/quickstart).
