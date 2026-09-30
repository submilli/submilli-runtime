// A Mastra agent that runs its programs on submilli-server, over MCP.

import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const BLUEPRINT = "research";

type Model = ConstructorParameters<typeof Agent>[0]["model"];

function instructions(userId: string): string {
  return [
    "You are a research assistant. Search the web and read pages by writing programs for Submilli.",
    "Do the whole job in one program where you can, and return only what you need to answer.",
    `Keep a note of what you learn, with its sources, under /${userId}/notes.`,
    "Read your earlier notes before you search again.",
  ].join(" ");
}

export async function answer(
  question: string,
  userId: string,
  model: Model = "google/gemini-3.8-flash",
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
            Authorization: `Bearer ${userToken()}`,
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
