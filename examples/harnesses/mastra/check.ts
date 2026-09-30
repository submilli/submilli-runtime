// Runs agent.ts against a local submilli-server with a scripted model in place
// of a real one, so it needs no API key. Start the server as the README says.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { MastraLanguageModelV2Mock } from "@mastra/core/test-utils/llm-mock";
import { answer } from "./agent.ts";

const program = readFileSync(new URL("../note.ts", import.meta.url), "utf8");
const usage = { inputTokens: 1, outputTokens: 1, totalTokens: 2 };

/** Calls the execute tool once, then answers with what the tool returned. */
function scripted(code: string): MastraLanguageModelV2Mock {
  return new MastraLanguageModelV2Mock({
    doGenerate: async ({ prompt, tools }: any) => {
      const execute = tools?.find((tool: any) => tool.name.endsWith("submilli__typescript__execute"));
      assert.ok(execute, "the execute tool was discovered");
      assert.match(execute.description ?? "", /strict TypeScript subset/);
      const result = prompt.find((message: any) => message.role === "tool");
      return {
        content: result
          ? [{ type: "text", text: JSON.stringify(result.content) }]
          : [{
              type: "tool-call",
              toolCallId: "call-1",
              toolName: execute.name,
              input: JSON.stringify({ code }),
            }],
        finishReason: result ? "stop" : "tool-calls",
        usage,
        warnings: [],
      };
    },
  } as any);
}

assert.match(await answer("total", "u_ada", scripted(program)), /notes: .*check\.md/);
assert.match(
  await answer("total", "u_ada", scripted(program.replaceAll("u_ada", "u_grace"))),
  /permission denied/,
);
await assert.rejects(answer("total", "", scripted(program)), /refused the connection/);

// A token the server does not know gets no tools either.
process.env.SUBMILLI_SERVER_TOKEN = "a-token-the-server-does-not-know";
await assert.rejects(answer("total", "u_ada", scripted(program)), /refused the connection/);
console.log("mastra: ok");
