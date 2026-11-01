# Vercel AI SDK

Read [harnesses](harnesses.md) first. Submilli is the governed execution boundary; the Vercel AI SDK remains the model and agent loop. Keep the application's provider, authentication, and framework choices.

Connect with `@submilli/ai-sdk`: `submilliTools()` returns Submilli's tools as an AI SDK `ToolSet` for `generateText`, `streamText`, and agents built on them, and runs every program on submilli-server over the HTTP API. Use the MCP route ("Alternative: MCP with @ai-sdk/mcp", below) only for an application that already loads Submilli through `@ai-sdk/mcp`.

## Before writing the harness

For a new project, follow [setup](setup.md), then [packages](packages.md), then [blueprints](blueprints.md). The [shared harness setup](harnesses.md) builds the `@pied-piper/billing` / `support-read` fixture these examples use. Run `submilli build check`, `submilli build test`, and `submilli blueprint lint` before involving a model.

The package needs Node.js 22 or later, AI SDK 7 (`ai` 7.x), and submilli-server 0.4.1 or later. Inspect the installed versions first (`npm ls ai @submilli/ai-sdk`); for an application on AI SDK 6 or earlier, upgrade `ai` deliberately or use the MCP route. These examples were verified on 2026-11-01 with `@submilli/ai-sdk 0.1.0`, `ai 7.0.159`, and submilli-server 0.5.1; if an export or signature differs, the installed declarations win over this text. The package README is the API reference: [@submilli/ai-sdk](https://github.com/submilli/submilli-typescript/tree/main/packages/ai-sdk#readme).

## Runnable server-side agent

```sh
npm install @submilli/ai-sdk ai @ai-sdk/openai
npm install -D typescript tsx @types/node
export OPENAI_API_KEY='...'
export SUBMILLI_SERVER='http://127.0.0.1:8128'
# SUBMILLI_SERVER_TOKEN must already hold the token the server was started with.
```

The package reads `SUBMILLI_SERVER`, not the `SUBMILLI_SERVER_URL` the MCP recipe and the fixture's export line use; set it to the fixture's URL when testing against it. `agent.ts` (server code; never browser code):

```typescript
import { openai } from "@ai-sdk/openai";
import { submilliTools } from "@submilli/ai-sdk";
import { generateText, isStepCount } from "ai";

const BLUEPRINT = process.env.SUBMILLI_BLUEPRINT ?? "support-read";
const MAX_STEPS = 8;

function customerBinding(customerId: string): Record<string, string> {
  // Replace this with the application's canonical customer-id validator.
  if (!/^[A-Za-z0-9_-]+$/.test(customerId)) throw new Error("invalid authenticated customer identity");
  return { customerId };
}

// Once per request. Server and token come from SUBMILLI_SERVER and
// SUBMILLI_SERVER_TOKEN; requests with the same conversationId share a session.
function tools(customerId: string, conversationId: string) {
  return submilliTools({ blueprint: BLUEPRINT, conversationId, variables: customerBinding(customerId) });
}

export async function answerForRequest(request: Request, customerId: string, conversationId: string): Promise<string> {
  const submilli = await tools(customerId, conversationId);
  const result = await generateText({
    model: openai(process.env.OPENAI_MODEL ?? "gpt-5.5"),
    system: "Use the Submilli tools for business data; never bypass them with shell, HTTP, or SDK calls.",
    tools: submilli.tools,
    prompt: await request.text(),
    stopWhen: isStepCount(MAX_STEPS),
    abortSignal: request.signal,
  });
  for (const step of result.steps) {
    for (const output of step.toolResults) {
      const executionId = (output.output as { execution_id?: string } | undefined)?.execution_id;
      if (executionId) console.error("program run:", executionId);
    }
  }
  const lastStep = result.steps.at(-1);
  if (result.steps.length >= MAX_STEPS && lastStep !== undefined && lastStep.toolCalls.length > 0) {
    throw new Error("agent stopped after its tool-step budget");
  }
  return result.text;
}

/** End the conversation's session when the conversation is over, not after every request. */
export async function endConversation(customerId: string, conversationId: string): Promise<void> {
  await (await tools(customerId, conversationId)).close();
}

async function main(): Promise<void> {
  const customerId = "pied_piper"; // demo only: take it from the signed-in session
  const conversationId = `customer:${customerId}`;
  const request = new Request("http://localhost/agent", {
    method: "POST",
    body: `Read the balance for customer ${customerId} and report the amount.`,
  });
  try {
    console.log(await answerForRequest(request, customerId, conversationId));
  } finally {
    await endConversation(customerId, conversationId);
  }
}

if (import.meta.filename === process.argv[1]) {
  main().catch((error: unknown) => {
    console.error(error);
    process.exitCode = 1;
  });
}
```

Run it with `npx tsx agent.ts`. The model should call `submilli__typescript__execute` with a program that imports `@pied-piper/billing`, and report `6150`. That is evidence of a live model run only when a provider key is configured; a scripted model proves wiring only. The SDK default is one step, so keep a finite `stopWhen` and treat budget exhaustion as an explicit outcome.

What the package does, and the rules that follow from it:

- **Identity comes from the request.** `variables` (and `secrets` for `harness` secrets) are sent when the session opens, from the application's authenticated state; the model never sees or sets them. Values travel in the JSON body, not a header, but still validate their format.
- **One session per conversation.** Call `submilliTools()` per request with the conversation's id. The first program opens a server session; later requests with the same `conversationId` find it in `store` (default: this process's memory). For several processes or serverless functions, pass a `SessionStore` over the application's database or cache. Different variables for an existing conversation throw `SubmilliSessionMismatchError` from `submilliTools()`, before the model is called; a different user gets a different conversation. Call `close()` when the conversation ends, or the Blueprint's `idle_timeout` ends it. Without `conversationId`, the session belongs to that one `SubmilliTools`; call its `close()` in `finally`.
- **Six tools.** The model gets `submilli__typescript__execute`, `__last_run`, `__packages__search`, `__packages__docs`, `__builtins__list` and `__builtins__docs`, with the descriptions the server writes for the Blueprint. Pass them unchanged.
- **Retries are safe.** Each execute sends an `Idempotency-Key` derived from the AI SDK's `toolCallId`, so a repeated call replays its recorded result. The output carries `execution_id` and `session_id` for the audit trail; the model is not shown them.
- **Policy denials are results.** A denied capability returns `denied: true` and `error.kind` `permission_denied`, naming the capability. Program failures are results too. A wrong token, an unknown Blueprint, invalid variables, and an unreachable server throw.
- **Application tools.** `ptc: { ... }` lets programs call the application's own AI SDK tools as the `@tools` Package; the Blueprint must grant `tools.call`. `needsApproval` or `toolApproval` asks a person through the AI SDK's approval flow, or `onApprovalRequired` decides in code. See the README's "Your tools in programs" and "Approvals". A tool given both to `ptc` and to the model directly bypasses the Blueprint on the direct path.

## Authenticated streaming route

```typescript
import { openai } from "@ai-sdk/openai";
import { submilliTools } from "@submilli/ai-sdk";
import { isStepCount, streamText } from "ai";

export async function handleStream(request: Request, customerId: string, conversationId: string): Promise<Response> {
  if (!/^[A-Za-z0-9_-]+$/.test(customerId)) throw new Error("invalid authenticated customer identity");
  const submilli = await submilliTools({ blueprint: "support-read", conversationId, variables: { customerId } });
  const result = streamText({
    model: openai("gpt-5.5"),
    tools: submilli.tools,
    prompt: await request.text(),
    stopWhen: isStepCount(8),
    abortSignal: request.signal,
    onError: ({ error }) => console.error("agent stream failed", error),
  });
  return result.toTextStreamResponse();
}
```

There is no client to close per request: the session belongs to the conversation and stays open for its next request, so never close it in the handler's `finally` while the stream is still running. End it with `close()` when the conversation is deleted or archived. In a UI-message route use `toUIMessageStreamResponse()`, and pass the conversation's messages instead of `prompt`.

## Deterministic verification with a scripted model

Before a live run, keep `@submilli/ai-sdk`, `generateText` and the local server real, and script only the model with `MockLanguageModelV4` from `ai/test`: one `submilli__typescript__execute` call, then a final text step that quotes the tool result. Use a fresh `conversationId` per case and `close()` each conversation at the end. Assert:

1. Bound to `pied_piper`, a program reading `pied_piper` returns `result` `"6150"`. The offered execute tool's input schema has only `code`; no Blueprint or identity argument reaches the model.
2. The same binding reading `hooli` returns a tool result with `"denied":true`, `permission_denied` and `pied-piper.example/balance.read`; nothing throws.
3. Two concurrent conversations bound to different customers each get their own result.
4. `submilliTools()` for an existing conversation with another customer rejects with `SubmilliSessionMismatchError`.
5. Without `variables`, the first execute throws the server's refusal naming `customerId`. With a `token` the server does not know, `submilliTools()` rejects with `AuthenticationError` (HTTP 401).
6. `close()` returns `true` for each conversation that ran a program.

`skills/evals/harnesses/vercel/validate-package.ts` runs this matrix against the fixture. A scripted run is adapter and policy evidence, not a live model result.

## Alternative: MCP with @ai-sdk/mcp

An application that already loads Submilli through `@ai-sdk/mcp` can keep doing so; the package's session, idempotency, and binding rules then become the application's job. Keep the application's installed `ai`, `@ai-sdk/mcp`, and provider versions. Inspect installed declarations first:

```sh
npm ls ai @ai-sdk/mcp
node -p "require('ai/package.json').version"
node -p "require('@ai-sdk/mcp/package.json').version"
```

The recipe below was verified on 2026-09-17 with Node 22, TypeScript 5.9, `ai 6.0`, `@ai-sdk/mcp 2.0` and `zod 4.1`; if an export or signature differs, the installed version's declarations win over this text. Current SDKs export `createMCPClient` from `@ai-sdk/mcp`. Older applications may have `experimental_createMCPClient` from `ai`; follow the declarations actually installed rather than mixing generations.

Adapter versions differ in how they handle the server's optional GET-SSE stream, which Submilli answers with `400`: `@ai-sdk/mcp 1.0.0` stalled on it in validation, later 1.x and 2.x releases completed discovery and execution and only logged a warning through `onUncaughtError`. Keep that callback wired to logs, and if a run hangs after connecting, upgrade the adapter within the application's dependency range before debugging anything else.

### Minimal MCP agent

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
      model: openai("gpt-5.5"),
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

### MCP streaming route

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
      model: openai("gpt-5.5"),
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

### MCP validation before a live model

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

- [@submilli/ai-sdk README](https://github.com/submilli/submilli-typescript/tree/main/packages/ai-sdk#readme) — options, sessions, results, application tools, and approvals.
- [MCP tools integration](https://ai-sdk.dev/docs/ai-sdk-core/mcp-tools) — HTTP transport, tool conversion, and when to close a client.
- [`createMCPClient`](https://ai-sdk.dev/docs/reference/ai-sdk-core/create-mcp-client) — current `@ai-sdk/mcp` import and `tools()` / `close()` surface.
- [`streamText`](https://ai-sdk.dev/docs/reference/ai-sdk-core/stream-text) — `stopWhen`, `abortSignal`, `onFinish`, `onAbort`, `onError`, and response methods.
- [`stepCountIs`](https://ai-sdk.dev/docs/reference/ai-sdk-core/step-count-is) — bounded multi-step tool loop.
- [Tool calling](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling) — forwarding abort signals and step callbacks.
- [Error handling](https://ai-sdk.dev/docs/ai-sdk-core/error-handling) — abort semantics and stream errors.
- [Submilli harness rules](harnesses.md), [setup](setup.md), [packages](packages.md), and [blueprints](blueprints.md).
