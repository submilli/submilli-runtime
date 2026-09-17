import assert from "node:assert/strict";
import { createMCPClient, type MCPClient } from "@ai-sdk/mcp";
import { generateText, stepCountIs } from "ai";
import { MockLanguageModelV3 } from "ai/test";

const base = process.env.SUBMILLI_SERVER_URL ?? "http://127.0.0.1:18128";
const endpoint = base + "/mcp/support-read";
const code = 'import { readBalance } from "@acme/billing"; function main(): number { return readBalance("cus_northwind"); }';

function usage(): object {
  return {
    inputTokens: { total: 1, noCache: 1, cacheRead: 0, cacheWrite: 0 },
    outputTokens: { total: 1, text: 1, reasoning: 0 },
  };
}

async function open(customerId?: string): Promise<MCPClient> {
  const headers: Record<string, string> = {};
  if (customerId !== undefined) headers["submilli-variables"] = "customerId=" + customerId;
  return createMCPClient({
    transport: { type: "http", url: endpoint, headers },
    onUncaughtError: (error: unknown) => console.error("transport warning", error),
  });
}

async function main(): Promise<void> {
  const client = await open("cus_northwind");
  try {
    const tools = await client.tools();
    const execute = tools["submilli__typescript__execute"];
    assert.ok(execute, "execution tool was discovered");
    assert.match(execute.description, /strict TypeScript subset/);
    assert.deepEqual(execute.inputSchema.jsonSchema.required, ["code"]);

    const allowed = await execute.execute({ code }, {
      toolCallId: "direct-allowed",
      messages: [],
      abortSignal: new AbortController().signal,
    });
    assert.equal(allowed.structuredContent?.result, "6150");

    const model = new MockLanguageModelV3({
      doGenerate: async (options) => {
        const hasToolResult = options.prompt.some((message) => message.role === "tool");
        return {
          content: hasToolResult
            ? [{ type: "text", text: "balance 6150" }]
            : [{ type: "tool-call", toolCallId: "mock-call", toolName: "submilli__typescript__execute", input: { code } }],
          finishReason: { unified: hasToolResult ? "stop" : "tool-calls", raw: undefined },
          usage: usage(),
        };
      },
    });
    const generated = await generateText({ model, tools, prompt: "Read my balance", stopWhen: stepCountIs(3) });
    assert.equal(generated.text, "balance 6150");
    assert.equal(generated.steps.length, 2);
  } finally {
    await client.close();
  }

  const deniedClient = await open("cus_northwind");
  try {
    const tools = await deniedClient.tools();
    const denied = await tools["submilli__typescript__execute"].execute({ code: code.replace("cus_northwind", "cus_initech") }, {
        toolCallId: "cross-tenant",
        messages: [],
        abortSignal: new AbortController().signal,
      });
    assert.match(JSON.stringify(denied), /permission denied/i);
  } finally {
    await deniedClient.close();
  }

  // The server rejects a session without the required binding at initialize.
  await assert.rejects(open(), /required variable 'customerId'/);
  console.log("vercel MCP + mock model validation passed");
}

void main().catch((error: unknown) => {
  console.error(error);
  process.exitCode = 1;
});
