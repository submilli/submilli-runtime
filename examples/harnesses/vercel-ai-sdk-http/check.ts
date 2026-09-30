// Runs agent.ts against a local submilli-server with a scripted model in place
// of a real one, so it needs no API key. Start the server as the README says.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { MockLanguageModelV4 } from "ai/test";
import { answer } from "./agent.ts";
import { openSession } from "./submilli.ts";

const server = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const token = process.env.SUBMILLI_SERVER_TOKEN ?? "";
const program = readFileSync(new URL("../note.ts", import.meta.url), "utf8");
const usage = {
  inputTokens: { total: 1, noCache: 1, cacheRead: 0, cacheWrite: 0 },
  outputTokens: { total: 1, text: 1, reasoning: 0 },
};

function scripted(code: string): MockLanguageModelV4 {
  return new MockLanguageModelV4({
    doGenerate: async ({ prompt, tools }) => {
      const execute = tools?.find((tool) => tool.name === "submilli__typescript__execute");
      assert.ok(execute?.type === "function", "the execute tool is offered");
      assert.match(execute.description ?? "", /strict TypeScript subset/);
      const result = prompt.find((message) => message.role === "tool");
      return {
        content: result
          ? [{ type: "text", text: JSON.stringify(result.content) }]
          : [{
              type: "tool-call",
              toolCallId: "call-1",
              toolName: "submilli__typescript__execute",
              input: JSON.stringify({ code }),
            }],
        finishReason: { unified: result ? "stop" : "tool-calls", raw: undefined },
        usage,
        warnings: [],
      };
    },
  });
}

assert.match(await answer("total", "u_ada", scripted(program)), /notes: .*check\.md/);
assert.match(
  await answer("total", "u_ada", scripted(program.replaceAll("u_ada", "u_grace"))),
  /permission denied/,
);
await assert.rejects(
  openSession({ server, token, blueprint: "research" }),
  /required variable 'userId'/,
);
await assert.rejects(
  openSession({
    server,
    token: "a-token-the-server-does-not-know",
    blueprint: "research",
    variables: { userId: "u_ada" },
  }),
  /answered 401/,
);

const session = await openSession({
  server,
  token,
  blueprint: "research",
  variables: { userId: "u_ada" },
});
const call = { toolCallId: "direct", messages: [], context: {} };
try {
  const tools = session.tools;
  const found: any = await tools.submilli__typescript__packages__search.execute?.({ query: "jina" }, call);
  assert.ok(found.results.some((hit: any) => hit.name === "@submilli/jina"));
  const docs: any = await tools.submilli__typescript__packages__docs.execute?.({ name: "@submilli/jina" }, call);
  assert.match(docs, /searchJson/);
  const builtins: any = await tools.submilli__typescript__builtins__docs.execute?.({ names: ["Map", "Temporal.Instant"] }, call);
  assert.equal(builtins.results.length, 2);
  const listed: any = await tools.submilli__typescript__builtins__list.execute?.({}, call);
  assert.ok(listed.types.includes("Map"));
  await tools.submilli__typescript__execute.execute?.(
    { code: 'function main(): void { console.log("seen"); }' },
    call,
  );
  const last: any = await tools.submilli__typescript__last_run.execute?.({}, call);
  assert.deepEqual(last.console, ["seen"]);
} finally {
  await session.close();
}
console.log("vercel-ai-sdk-http: ok");
