# Vercel AI SDK

Read [harnesses](harnesses.md) first. Submilli is the governed execution boundary; the Vercel AI SDK remains the model and agent loop. Keep the application's installed `ai`, `@ai-sdk/mcp`, provider, and framework versions. Do not copy an example that upgrades the whole application. Inspect installed declarations first:

```sh
npm ls ai @ai-sdk/mcp
node -p "require('ai/package.json').version"
node -p "require('@ai-sdk/mcp/package.json').version"
```

The recipe below was verified on 2026-09-17 with Node 22, TypeScript 5.9, `ai 6.0`, `@ai-sdk/mcp 2.0` and `zod 4.1`; if an export or signature differs, the installed version's declarations win over this text. A provider package is also required in a real app; the provider and model are application choices. Current SDKs export `createMCPClient` from `@ai-sdk/mcp`. Older applications may have `experimental_createMCPClient` from `ai`; follow the declarations actually installed rather than mixing generations.

Adapter versions differ in how they handle the server's optional GET-SSE stream, which Submilli answers with `400`: `@ai-sdk/mcp 1.0.0` stalled on it in validation, later 1.x and 2.x releases completed discovery and execution and only logged a warning through `onUncaughtError`. Keep that callback wired to logs, and if a run hangs after connecting, upgrade the adapter within the application's dependency range before debugging anything else.

## Minimal runnable server-side agent

Start with the [Submilli setup](setup.md), [package](packages.md), and [blueprint](blueprints.md) guides. A working agent needs a package installed in the server's package store, a blueprint applied to the local server, and a required trusted session variable. The MCP endpoint is `http://127.0.0.1:8128/mcp/<blueprint-name>` using Streamable HTTP.

Install only the integration dependencies in the application directory:

```sh
npm install ai @ai-sdk/mcp @ai-sdk/openai
npm install -D typescript tsx
```

`agent.ts` (server code; never browser code) can then be this complete entrypoint. `customerId` must come from the authenticated, authorized request context. A chat message must never choose it.

```typescript
import { createMCPClient } from "@ai-sdk/mcp";
import { generateText, stepCountIs } from "ai";
import { openai } from "@ai-sdk/openai";

const SUBMILLI_URL = process.env.SUBMILLI_SERVER_URL ?? "http://127.0.0.1:8128";
// The API token the application was given for the server.
const SUBMILLI_TOKEN = process.env.SUBMILLI_SERVER_TOKEN;
const BLUEPRINT = "support-read";
const MAX_STEPS = 8;

function bindCustomer(customerId: string): string {
  // Replace this with the application's canonical customer-id validator.
  if (!/^cus_[a-z0-9_]+$/.test(customerId) || /[;\r\n]/.test(customerId)) {
    throw new Error("invalid authenticated customer identity");
  }
  return "customerId=" + customerId;
}

export async function answerForRequest(request: Request, customerId: string): Promise<string> {
  if (SUBMILLI_TOKEN === undefined) throw new Error("SUBMILLI_SERVER_TOKEN is required");
  const client = await createMCPClient({
    transport: {
      type: "http",
      url: SUBMILLI_URL + "/mcp/" + BLUEPRINT,
      headers: {
        Authorization: "Bearer " + SUBMILLI_TOKEN,
        "submilli-variables": bindCustomer(customerId),
      },
    },
  });

  try {
    const tools = await client.tools();
    const result = await generateText({
      model: openai("gpt-4o-mini"),
      tools, // Preserve every discovered name, schema, and description.
      prompt: "Answer the user's request using the governed customer tools: " +
        (await request.text()),
      stopWhen: stepCountIs(MAX_STEPS),
      abortSignal: request.signal,
    });

    const lastStep = result.steps[result.steps.length - 1];
    if (result.steps.length >= MAX_STEPS && lastStep !== undefined && lastStep.toolCalls.length > 0) {
      throw new Error("agent stopped after its tool-step budget");
    }
    return result.text;
  } finally {
    await client.close();
  }
}

async function readStdin(): Promise<string> {
  let body = "";
  for await (const chunk of process.stdin) body += chunk.toString();
  return body;
}

async function main(): Promise<void> {
  const customerId = process.env.AUTHENTICATED_CUSTOMER_ID;
  if (customerId === undefined) throw new Error("AUTHENTICATED_CUSTOMER_ID is required");
  const request = new Request("http://localhost/agent", { method: "POST", body: await readStdin() });
  console.log(await answerForRequest(request, customerId));
}

if (process.argv[1] !== undefined && import.meta.url.endsWith(process.argv[1])) {
  void main().catch((error: unknown) => {
    console.error(error);
    process.exitCode = 1;
  });
}
```

The SDK default is one step; a tool call can therefore be returned without a final answer unless `stopWhen` enables more steps. Keep the bound finite and treat budget exhaustion as an explicit outcome. `stopWhen: stepCountIs(n)` is the AI SDK 5/6 replacement for the old `maxSteps` option. A raw HTTP, shell, or provider tool registered alongside these tools can bypass this boundary; remove or separately govern those tools when all business actions must pass through Submilli.

Each identity needs its own MCP client and tool closures. Never mutate headers on a shared client or reuse tools created for another tenant. The server trusts the binding as caller input from any holder of the token, so keep `SUBMILLI_SERVER_TOKEN` server-side (never in browser code, prompts, or tool schemas), and add TLS and restricted ingress when the server is reachable beyond a trusted local network.

## Authenticated streaming route

Streaming changes cleanup timing. Do not put `client.close()` in a handler `finally` immediately after returning the stream: the SDK may still be executing MCP tools. Close after normal completion, cancellation, or failure. This framework-neutral handler uses the SDK's `onFinish`, `onAbort`, and `onError` callbacks; `onFinish` is not called for an abort.

```typescript
import { createMCPClient } from "@ai-sdk/mcp";
import { stepCountIs, streamText } from "ai";
import { openai } from "@ai-sdk/openai";

function bindCustomer(customerId: string): string {
  if (!/^cus_[a-z0-9_]+$/.test(customerId) || /[;\r\n]/.test(customerId)) {
    throw new Error("invalid authenticated customer identity");
  }
  return "customerId=" + customerId;
}

export async function handleStream(request: Request, customerId: string): Promise<Response> {
  const client = await createMCPClient({
    transport: {
      type: "http",
      url: (process.env.SUBMILLI_SERVER_URL ?? "http://127.0.0.1:8128") + "/mcp/support-read",
      headers: {
        Authorization: "Bearer " + process.env.SUBMILLI_SERVER_TOKEN,
        "submilli-variables": bindCustomer(customerId),
      },
    },
    onUncaughtError: (error: unknown) => console.error("MCP transport error", error),
  });

  let closed = false;
  const closeOnce = async (): Promise<void> => {
    if (closed) return;
    closed = true;
    await client.close();
  };

  try {
    const tools = await client.tools();
    const result = streamText({
      model: openai("gpt-4o-mini"),
      tools,
      prompt: await request.text(),
      stopWhen: stepCountIs(8),
      abortSignal: request.signal,
      onFinish: async ({ steps, finishReason }) => {
        console.info("agent finished", { steps: steps.length, finishReason });
        await closeOnce();
      },
      onAbort: async ({ steps }) => {
        console.info("agent cancelled", { steps: steps.length });
        await closeOnce();
      },
      onError: async ({ error }) => {
        console.error("agent stream failed", error);
        await closeOnce();
      },
    });
    return result.toTextStreamResponse();
  } catch (error) {
    await closeOnce();
    throw error;
  }
}
```

The authenticated framework should pass its canonical identity to `handleStream`; the example's `bindCustomer` is the same validator used by the non-streaming route. If the framework exposes a disconnect signal separately, connect it to an `AbortController` and pass that signal to `streamText`. In a UI-message route use the corresponding `toUIMessageStreamResponse()` method, keeping the same callback cleanup. The AI SDK documents that `onAbort` runs for `AbortSignal` cancellation while `onFinish` does not, and that stream errors are reported through `onError`.

## Deterministic validation before a live model

Use the real `@ai-sdk/mcp` client against a local Submilli server and the SDK's test-only `MockLanguageModelV3` from `ai/test`; do not replace the MCP client or its HTTP transport with a fake. The mock should emit one tool call followed by a final text step. Run this in a temporary directory with the app's exact versions and no provider key:

```sh
tmp_dir=$(mktemp -d)
cd "$tmp_dir"
npm init -y
npm install ai @ai-sdk/mcp typescript tsx zod   # or the app's exact versions
# write validate.ts asserting the list below, then:
SUBMILLI_SERVER_URL=http://127.0.0.1:8128 npx tsx validate.ts   # SUBMILLI_SERVER_TOKEN exported
```

The validation must assert all of the following against the local fixture:

1. `createMCPClient` initializes and `client.tools()` discovers the execution tool with its code schema and description intact; the serialized tool-call schema contains no model-controlled blueprint or identity binding.
2. The mock model calls the discovered tool with code that reads the bound customer and receives `6150`; the bounded loop then produces final text.
3. Calling the same tool with another customer's id is denied by Submilli, and omitting the required binding is rejected at initialize: `createMCPClient` itself rejects with `MCPClientError` (HTTP 400, `required variable 'customerId' was not supplied`), so assert with `assert.rejects` rather than expecting a tool result. Send the token in that case; omitting the token instead rejects with HTTP 401 and starts no OAuth flow. The denial, by contrast, is a normal tool result containing `permission denied`.
4. Two concurrent clients bound to different customers cannot cross-use each other's tool closures. Close each client in `finally` and assert cleanup on normal completion, cancellation, and an MCP/tool failure.

This proves the adapter and policy boundary without a live model or business service. Only after it passes should an application run one live model task; report that separately from deterministic mock results. No live model or business call belongs in the validation command.

## Version and API references

- [MCP tools integration](https://ai-sdk.dev/docs/ai-sdk-core/mcp-tools) — HTTP transport, tool conversion, and when to close a client.
- [`createMCPClient`](https://ai-sdk.dev/docs/reference/ai-sdk-core/create-mcp-client) — current `@ai-sdk/mcp` import and `tools()` / `close()` surface.
- [`streamText`](https://ai-sdk.dev/docs/reference/ai-sdk-core/stream-text) — `stopWhen`, `abortSignal`, `onFinish`, `onAbort`, `onError`, and response methods.
- [`stepCountIs`](https://ai-sdk.dev/docs/reference/ai-sdk-core/step-count-is) — bounded multi-step tool loop.
- [Tool calling](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling) — forwarding abort signals and step callbacks.
- [Error handling](https://ai-sdk.dev/docs/ai-sdk-core/error-handling) — abort semantics and stream errors.
- [Submilli harness rules](harnesses.md), [setup](setup.md), [packages](packages.md), and [blueprints](blueprints.md).
