// Checks that the agent in agent.ts connects to a local submilli-server and is
// offered its tools, and that a connection without a user, or with a token the
// server does not know, is refused. It
// sends the model nothing, so it costs nothing. The Claude Agent SDK has no
// scripted model to stand in for a real one; run agent.ts to see a full answer.

import assert from "node:assert/strict";
import { query, type McpServerStatus, type SDKUserMessage } from "@anthropic-ai/claude-agent-sdk";
import { options } from "./agent.ts";

async function connect(userId: string): Promise<McpServerStatus> {
  let release = (): void => {};
  const held = new Promise<void>((resolve) => (release = resolve));
  async function* silent(): AsyncGenerator<SDKUserMessage> {
    await held;
  }

  const agent = query({ prompt: silent(), options: options(userId) });
  try {
    for (let attempt = 0; attempt < 50; attempt++) {
      const [status] = await agent.mcpServerStatus();
      if (status !== undefined && status.status !== "pending") return status;
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    throw new Error("the connection to submilli-server stayed pending");
  } finally {
    release();
    agent.close();
  }
}

const bound = await connect("u_ada");
assert.equal(bound.status, "connected", bound.error);
const offered = bound.tools?.map((tool) => tool.name) ?? [];
assert.ok(offered.includes("submilli__typescript__execute"), offered.join(", "));

const unbound = await connect("");
assert.equal(unbound.status, "failed");

process.env.SUBMILLI_USER_TOKEN = "a-token-the-server-does-not-know";
const unknown = await connect("u_ada");
assert.equal(unknown.status, "failed");
console.log("claude-agent-sdk: ok");
