// A Mastra agent that runs its programs on submilli-server, through @submilli/mastra.

import { readFileSync } from "node:fs";
import { Mastra } from "@mastra/core";
import { Agent } from "@mastra/core/agent";
import { InMemoryStore } from "@mastra/core/storage";
import { Memory } from "@mastra/memory";
import { createSubmilliTools } from "@submilli/mastra";

type Model = ConstructorParameters<typeof Agent>[0]["model"];

// The agent's brief, kept beside the blueprint.
const INSTRUCTIONS = readFileSync(new URL("../prompt.txt", import.meta.url), "utf8");

// One per process. Server and token come from SUBMILLI_SERVER and
// SUBMILLI_SERVER_TOKEN; the user comes from each agent run's memory resource.
export const submilli = createSubmilliTools({
  blueprint: "research",
  variables: ({ resourceId }) => {
    if (!resourceId) throw new Error("run the agent with memory: { thread, resource }");
    return { userId: resourceId };
  },
});

// Each memory thread keeps its Submilli session in its metadata. A real
// deployment uses persistent storage (LibSQLStore, PostgresStore), so threads
// and their sessions survive restarts.
export const memory = new Memory();
const storage = new InMemoryStore();

export function researcher(model: Model = "anthropic/claude-sonnet-5"): Agent {
  // Only Submilli's tools are given to the model.
  const agent = new Agent({ id: "researcher", name: "Researcher", instructions: INSTRUCTIONS, model, memory, tools: () => submilli.tools() });
  return new Mastra({ agents: { researcher: agent }, storage, logger: false }).getAgent("researcher");
}

/** Answer one message of a conversation: a memory thread of the user's. */
export async function answer(question: string, userId: string, threadId: string, agent = researcher()): Promise<string> {
  const result = await agent.generate(question, { memory: { thread: threadId, resource: userId }, maxSteps: 20 });
  for (const part of result.toolResults) {
    // Where each program's run is in the server's audit trail.
    const executionId = (part.payload.result as { execution_id?: string } | undefined)?.execution_id;
    if (executionId) console.error(`[submilli] execution ${executionId}`);
  }
  return result.text;
}

if (import.meta.filename === process.argv[1]) {
  try {
    // In a real application the user comes from the signed-in session.
    console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada", "example"));
  } finally {
    // The session belongs to the thread. This one is done with it.
    await submilli.close({ threadId: "example", memory });
  }
}
