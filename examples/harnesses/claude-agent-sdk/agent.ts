// A Claude Agent SDK agent that runs its programs on submilli-server, over MCP.

import { readFileSync } from "node:fs";
import { query, type Options } from "@anthropic-ai/claude-agent-sdk";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

// The agent's brief, kept beside the blueprint.
const INSTRUCTIONS = readFileSync(new URL("../prompt.txt", import.meta.url), "utf8");

export function options(userId: string): Options {
  return {
    model: "claude-sonnet-5",
    systemPrompt: INSTRUCTIONS,
    mcpServers: {
      // One entry per user: the binding is fixed when the agent connects.
      submilli: {
        type: "http",
        url: `${SUBMILLI_SERVER}/mcp/${BLUEPRINT}`,
        headers: {
          Authorization: `Bearer ${serverToken()}`,
          "submilli-variables": `userId=${userId}`,
        },
      },
    },
    // Only the server above, whatever else the account or machine has configured.
    strictMcpConfig: true,
    allowedTools: ["mcp__submilli__*"],
    // No shell, no file tools, no web fetch: every action goes through Submilli.
    tools: [],
    settingSources: [],
    maxTurns: 20,
  };
}

export async function answer(question: string, userId: string): Promise<string> {
  for await (const message of query({ prompt: question, options: options(userId) })) {
    if (message.type === "result") {
      return message.subtype === "success" ? message.result : `stopped: ${message.subtype}`;
    }
  }
  throw new Error("the agent ended without a result");
}

/** The API token this application was given for the server. */
function serverToken(): string {
  const token = process.env.SUBMILLI_SERVER_TOKEN;
  if (!token) throw new Error("SUBMILLI_SERVER_TOKEN is not set: export the token the server was started with");
  return token;
}

if (import.meta.filename === process.argv[1]) {
  // In a real application the user comes from the signed-in session.
  console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada"));
}
