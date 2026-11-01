// Runs agent.ts against a local submilli-server with a scripted model in place
// of a real one, so it needs no API key. Start the server as the README says.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Agent } from "@mastra/core/agent";
import { createSubmilliTools } from "@submilli/mastra";
import { MockLanguageModelV4 } from "ai/test";
import { answer, memory, researcher, submilli } from "./agent.ts";

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
      const last = prompt.at(-1);
      const result = last?.role === "tool" ? last : undefined;
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

const note = (userId: string, threadId: string, agent = researcher(scripted() as never)) =>
  answer("note", userId, threadId, agent);
const sessionId = async (threadId: string) => (await submilli.session({ threadId, memory }))?.id;

const run = Date.now();
const [ada, grace, alan] = [`ada_${run}`, `grace_${run}`, `alan_${run}`];

// Two users write the same note; each sees only their own.
assert.match(await note(ada, `ada-${run}`), /notes before: none/);
assert.match(await note(grace, `grace-${run}`), /notes before: none/);

// A thread keeps one session across agent runs, and refuses another user.
assert.match(await note(alan, `alan-${run}`), /notes before: none/);
const first = await sessionId(`alan-${run}`);
assert.ok(first, "the thread has a session");
assert.match(await note(alan, `alan-${run}`), /notes before: check\.md/);
assert.equal(await sessionId(`alan-${run}`), first, "the thread's second run opened a new session");
// Mastra itself refuses a run on a thread that belongs to another resource.
await assert.rejects(note(ada, `alan-${run}`), /resource/i);

// Closing ends the thread's session on the server.
for (const threadId of [`ada-${run}`, `grace-${run}`, `alan-${run}`]) {
  assert.equal(await submilli.close({ threadId, memory }), true, "the thread's session was not ended");
}

// The server refuses a session without the user binding, and a token it does not know.
const unbound = createSubmilliTools({ blueprint: "research" });
const nobody = new Agent({ id: "nobody", name: "Nobody", instructions: "", model: scripted() as never, tools: () => unbound.tools() });
assert.match((await nobody.generate("note", { maxSteps: 4 })).text, /"error-text".*required variable 'userId'/);
const stranger = createSubmilliTools({ blueprint: "research", token: "a-token-the-server-does-not-know" });
await assert.rejects(stranger.tools(), (error: Error) => error.name === "AuthenticationError");
console.log("mastra: ok");
