// @submilli/mastra against the local support-read fixture. Only the model is
// scripted (MockLanguageModelV4 from ai/test); Mastra, its memory, the package
// and the server are real.
import assert from "node:assert/strict";
import { Mastra } from "@mastra/core";
import { Agent } from "@mastra/core/agent";
import { InMemoryStore } from "@mastra/core/storage";
import { Memory } from "@mastra/memory";
import { createSubmilliTools, type SubmilliTools } from "@submilli/mastra";
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

// The customer is the memory resource the thread belongs to.
const submilli = createSubmilliTools({
  blueprint: "support-read",
  variables: ({ resourceId }) => {
    if (!resourceId) throw new Error("run the agent with memory: { thread, resource }");
    return { customerId: resourceId };
  },
});
const memory = new Memory();
const storage = new InMemoryStore();

function agent(code: string, tools: SubmilliTools = submilli): Agent {
  const built = new Agent({ id: "assistant", name: "Assistant", instructions: "", model: scripted(code) as never, memory, tools: () => tools.tools() });
  return new Mastra({ agents: { assistant: built }, storage, logger: false }).getAgent("assistant");
}

async function run(customerId: string, threadId: string, code: string): Promise<string> {
  const result = await agent(code).generate("balance", { memory: { thread: threadId, resource: customerId }, maxSteps: 4 });
  return result.text;
}

const id = Date.now();
// Two customers at once, each on its own thread, reading pied_piper.
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
// The thread kept one session across both runs.
assert.ok(await submilli.session({ threadId: `a-${id}`, memory }), "the thread keeps its session");
// Without the binding the server refuses the session: a tool error, not a result.
const unbound = createSubmilliTools({ blueprint: "support-read" });
const refused = await agent(read("pied_piper"), unbound).generate("balance", { maxSteps: 4 });
assert.match(refused.text, /"error-text".*customerId/);
// A token the server does not know cannot even load the tools.
const stranger = createSubmilliTools({ blueprint: "support-read", token: "a-token-the-server-does-not-know" });
await assert.rejects(stranger.tools(), (error: Error) => error.name === "AuthenticationError");
// Closing each thread ends its session.
for (const threadId of [`a-${id}`, `b-${id}`]) assert.equal(await submilli.close({ threadId, memory }), true);
console.log("mastra package + mock model validation passed");
