// A Claude Agent SDK agent that runs its programs on submilli-server, over MCP.

import { query, type Options } from "@anthropic-ai/claude-agent-sdk";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

function instructions(userId: string): string {
  return [
    "You are a research assistant. Search the web and read pages by writing programs for Submilli.",
    "Do the whole job in one program where you can, and return only what you need to answer.",
    `Keep a note of what you learn, with its sources, under /${userId}/notes.`,
    "Read your earlier notes before you search again.",
  ].join(" ");
}

export function options(userId: string): Options {
  return {
    systemPrompt: instructions(userId),
    mcpServers: {
      // One entry per user: the binding is fixed when the agent connects.
      submilli: {
        type: "http",
        url: `${SUBMILLI_SERVER}/mcp/${BLUEPRINT}`,
        headers: {
          Authorization: `Bearer ${userToken()}`,
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

/** The application's token for the server: the `user` role, never the admin one. */
function userToken(): string {
  const token = process.env.SUBMILLI_USER_TOKEN;
  if (!token) throw new Error("SUBMILLI_USER_TOKEN is not set: export the server's user token");
  return token;
}

if (import.meta.filename === process.argv[1]) {
  // In a real application the user comes from the signed-in session.
  console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada"));
}
