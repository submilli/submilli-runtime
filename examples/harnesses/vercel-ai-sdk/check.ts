// Runs agent.ts against a local submilli-server with a scripted model in place
// of a real one, so it needs no API key. Start the server as the README says.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { SubmilliSessionMismatchError, submilliTools } from "@submilli/ai-sdk";
import { MockLanguageModelV4 } from "ai/test";
import { answer, submilli } from "./agent.ts";

const PROGRAM = readFileSync(new URL("../note.ts", import.meta.url), "utf8");
const EXECUTE = "submilli__typescript__execute";
const SUBMILLI_TOOLS = [
  EXECUTE,
  "submilli__typescript__last_run",
  "submilli__typescript__packages__search",
  "submilli__typescript__packages__docs",
  "submilli__typescript__builtins__list",
  "submilli__typescript__builtins__docs",
];
const usage = {
  inputTokens: { total: 1, noCache: 1, cacheRead: 0, cacheWrite: 0 },
  outputTokens: { total: 1, text: 1, reasoning: 0 },
};

/** Calls the execute tool with note.ts once, then answers with what the tool returned. */
function scripted(): MockLanguageModelV4 {
  return new MockLanguageModelV4({
    doGenerate: async ({ prompt, tools }) => {
      assert.deepEqual(tools?.map((tool) => tool.name).sort(), [...SUBMILLI_TOOLS].sort());
      const execute = tools?.find((tool) => tool.name === EXECUTE);
      assert.ok(execute?.type === "function", "the execute tool is offered");
      assert.match(execute.description ?? "", /strict TypeScript subset/);
      const result = prompt.find((message) => message.role === "tool");
      return {
        content: result
          ? [{ type: "text", text: JSON.stringify(result.content) }]
          : [{
              type: "tool-call",
              toolCallId: `call-${crypto.randomUUID()}`,
              toolName: EXECUTE,
              input: JSON.stringify({ code: PROGRAM }),
            }],
        finishReason: { unified: result ? "stop" : "tool-calls", raw: undefined },
        usage,
        warnings: [],
      };
    },
  });
}

async function sessionId(userId: string, conversationId: string): Promise<string | undefined> {
  return (await (await submilli(userId, conversationId)).session())?.id;
}

const run = Date.now();
const [ada, grace, alan] = [`ada_${run}`, `grace_${run}`, `alan_${run}`];
const conversations: [string, string][] = [[ada, `ada-${run}`], [grace, `grace-${run}`], [alan, `alan-${run}`]];

// Two users write the same note; each sees only their own.
assert.match(await answer("note", ada, `ada-${run}`, scripted()), /notes before: none/);
assert.match(await answer("note", grace, `grace-${run}`, scripted()), /notes before: none/);

// A conversation keeps one session across requests, and refuses another user.
assert.match(await answer("note", alan, `alan-${run}`, scripted()), /notes before: none/);
const first = await sessionId(alan, `alan-${run}`);
assert.ok(first, "the conversation has a session");
assert.match(await answer("note", alan, `alan-${run}`, scripted()), /notes before: check\.md/);
assert.equal(await sessionId(alan, `alan-${run}`), first, "the conversation's second request opened a new session");
await assert.rejects(submilli(ada, `alan-${run}`), SubmilliSessionMismatchError);

// Closing ends the conversation's session on the server.
for (const [userId, conversationId] of conversations) {
  assert.equal(await (await submilli(userId, conversationId)).close(), true, "the conversation's session was not ended");
}

// The server refuses a session without the user binding, and a token it does not know.
const nobody = await submilliTools({ blueprint: "research", conversationId: `nobody-${run}` });
const call = { toolCallId: `direct-${run}`, messages: [], context: {} };
await assert.rejects(
  Promise.resolve(nobody.tools[EXECUTE].execute?.({ code: PROGRAM }, call)),
  /userId/,
);
await assert.rejects(
  submilliTools({ blueprint: "research", token: "a-token-the-server-does-not-know", variables: { userId: ada } }),
  (error: Error) => error.name === "AuthenticationError",
);

// The model's other tools reach the server too.
const direct = await submilli(ada, `direct-${run}`);
const callId = () => ({ toolCallId: `direct-${crypto.randomUUID()}`, messages: [], context: {} });
try {
  const tools: any = direct.tools;
  const found = await tools.submilli__typescript__packages__search.execute({ query: "jina" }, callId());
  assert.ok(found.results.some((hit: any) => hit.name === "@submilli/jina"));
  assert.match(await tools.submilli__typescript__packages__docs.execute({ name: "@submilli/jina" }, callId()), /searchJson/);
  const builtins = await tools.submilli__typescript__builtins__docs.execute({ names: ["Map", "Temporal.Instant"] }, callId());
  assert.equal(builtins.results.length, 2);
  const listed = await tools.submilli__typescript__builtins__list.execute({}, callId());
  assert.ok(listed.types.includes("Map"));
  await tools[EXECUTE].execute({ code: 'function main(): void { console.log("seen"); }' }, callId());
  const last = await tools.submilli__typescript__last_run.execute({}, callId());
  assert.deepEqual(last.console, ["seen"]);
  // The blueprint grants `main` nothing outside Jina, the model and its notes.
  const refused = await tools[EXECUTE].execute(
    { code: 'import { check } from "submilli:security";\nfunction main(): void { check("example.com/op", {}); }' },
    callId(),
  );
  assert.equal(refused.denied, true);
  assert.equal(refused.error.kind, "permission_denied");
  const failed = await tools[EXECUTE].execute({ code: 'function main(): void { throw new Error("boom"); }' }, callId());
  assert.equal(failed.denied, false);
  assert.equal(failed.error.kind, "runtime_error");
} finally {
  await direct.close();
}
console.log("vercel-ai-sdk: ok");
