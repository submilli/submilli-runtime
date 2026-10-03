// A Mastra agent that runs its programs on submilli-server, over MCP.

import { readFileSync } from "node:fs";
import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

type Model = ConstructorParameters<typeof Agent>[0]["model"];

function instructions(userId: string): string {
  // The agent's brief, kept beside the blueprint; `{userId}` names the user.
  const brief = readFileSync(new URL("../prompt.txt", import.meta.url), "utf8");
  return brief.replaceAll("{userId}", userId);
}

export async function answer(
  question: string,
  userId: string,
  model: Model = "anthropic/claude-haiku-4-5",
): Promise<string> {
  const agent = new Agent({
    id: "researcher",
    name: "Researcher",
    instructions: instructions(userId),
    model,
  });

  // One client per user: the binding is fixed when the client connects.
  const submilli = new MCPClient({
    id: `submilli-${userId}`,
    servers: {
      submilli: {
        url: new URL(`${SUBMILLI_SERVER}/mcp/${BLUEPRINT}`),
        requestInit: {
          headers: {
            Authorization: `Bearer ${serverToken()}`,
            "submilli-variables": `userId=${userId}`,
          },
        },
      },
    },
  });

  try {
    const toolsets = await submilli.listToolsets();
    // Mastra logs a refused connection and carries on with no tools.
    if (toolsets.submilli === undefined) throw new Error("submilli-server refused the connection");

    const result = await agent.generate(question, { toolsets, maxSteps: 20 });
    return result.text;
  } finally {
    await submilli.disconnect();
  }
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
