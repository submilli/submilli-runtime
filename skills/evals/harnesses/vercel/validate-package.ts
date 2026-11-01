// @submilli/ai-sdk against the local support-read fixture. Only the model is
// scripted (MockLanguageModelV4); the package, its HTTP calls and the server are real.
import assert from "node:assert/strict";
import { SubmilliSessionMismatchError, submilliTools } from "@submilli/ai-sdk";
import { generateText, isStepCount } from "ai";
import { MockLanguageModelV4 } from "ai/test";

const EXECUTE = "submilli__typescript__execute";
const read = (customer: string) =>
  `import { readBalance } from "@pied-piper/billing";\nfunction main(): number { return readBalance("${customer}"); }`;
const usage = {
  inputTokens: { total: 1, noCache: 1, cacheRead: 0, cacheWrite: 0 },
  outputTokens: { total: 1, text: 1, reasoning: 0 },
};

// One execute call with `code`, then a final answer quoting the tool result.
function scripted(code: string): MockLanguageModelV4 {
  return new MockLanguageModelV4({
    doGenerate: async ({ prompt, tools }) => {
      const execute = tools?.find((tool) => tool.name === EXECUTE);
      assert.ok(execute?.type === "function", "the execute tool is offered");
      assert.match(execute.description ?? "", /strict TypeScript subset/);
      assert.deepEqual(Object.keys((execute.inputSchema as { properties: object }).properties), ["code"]);
      const last = prompt.at(-1);
      const result = last?.role === "tool" ? last : undefined;
      return {
        content: result
          ? [{ type: "text", text: JSON.stringify(result.content) }]
          : [{ type: "tool-call", toolCallId: `call-${crypto.randomUUID()}`, toolName: EXECUTE, input: JSON.stringify({ code }) }],
        finishReason: { unified: result ? "stop" : "tool-calls", raw: undefined },
        usage,
        warnings: [],
      };
    },
  });
}

async function run(customerId: string, conversationId: string, code: string): Promise<string> {
  const submilli = await submilliTools({ blueprint: "support-read", conversationId, variables: { customerId } });
  const result = await generateText({ model: scripted(code), tools: submilli.tools, prompt: "balance", stopWhen: isStepCount(4) });
  return result.text;
}

const id = Date.now();
// Two customers at once, each in its own conversation, reading pied_piper.
const [own, other] = await Promise.all([
  run("pied_piper", `a-${id}`, read("pied_piper")),
  run("hooli", `b-${id}`, read("pied_piper")),
]);
assert.match(own, /"result":"6150"/);
assert.match(other, /"denied":true/);
// The bound customer reading another customer is a denial result, not a throw.
const denied = await run("pied_piper", `a-${id}`, read("hooli"));
assert.match(denied, /"denied":true/);
assert.match(denied, /pied-piper\.example\/balance\.read/);
// A conversation is bound to its first customer.
await assert.rejects(
  submilliTools({ blueprint: "support-read", conversationId: `a-${id}`, variables: { customerId: "hooli" } }),
  SubmilliSessionMismatchError,
);
// Without the binding the server refuses the session; without a known token, the client.
const unbound = await submilliTools({ blueprint: "support-read" });
const call = { toolCallId: `direct-${id}`, messages: [], context: {} };
await assert.rejects(Promise.resolve(unbound.tools[EXECUTE].execute?.({ code: read("pied_piper") }, call)), /customerId/);
await assert.rejects(
  submilliTools({ blueprint: "support-read", token: "a-token-the-server-does-not-know", variables: { customerId: "pied_piper" } }),
  (error: Error) => error.name === "AuthenticationError",
);
// Ending each conversation closes its session.
for (const [customerId, conversationId] of [["pied_piper", `a-${id}`], ["hooli", `b-${id}`]]) {
  const submilli = await submilliTools({ blueprint: "support-read", conversationId, variables: { customerId } });
  assert.equal(await submilli.close(), true);
}
console.log("vercel package + mock model validation passed");
