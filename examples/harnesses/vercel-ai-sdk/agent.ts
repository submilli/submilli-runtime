// A Vercel AI SDK agent that runs its programs on submilli-server, through @submilli/ai-sdk.

import { readFileSync } from "node:fs";
import { anthropic } from "@ai-sdk/anthropic";
import { submilliTools, type SubmilliTools } from "@submilli/ai-sdk";
import { generateText, isStepCount, type LanguageModel } from "ai";

// The agent's brief, kept beside the blueprint.
const INSTRUCTIONS = readFileSync(new URL("../prompt.txt", import.meta.url), "utf8");

/**
 * Submilli's tools for one request. The user comes with the request, and the
 * conversation's session is found again by its id. Server and token come
 * from SUBMILLI_SERVER and SUBMILLI_SERVER_TOKEN.
 */
export function submilli(userId: string, conversationId: string): Promise<SubmilliTools> {
  return submilliTools({ blueprint: "research", conversationId, variables: { userId } });
}

export async function answer(
  question: string,
  userId: string,
  conversationId: string,
  model: LanguageModel = anthropic("claude-sonnet-5"),
): Promise<string> {
  const { tools } = await submilli(userId, conversationId);
  // Only Submilli's tools are given to the model.
  const result = await generateText({
    model,
    system: INSTRUCTIONS,
    tools,
    prompt: question,
    stopWhen: isStepCount(20),
  });
  for (const step of result.steps) {
    for (const output of step.toolResults) {
      // Where each program's run is in the server's audit trail.
      const executionId = (output.output as { execution_id?: string } | undefined)?.execution_id;
      if (executionId) console.error(`[submilli] execution ${executionId}`);
    }
  }
  return result.text;
}

if (import.meta.filename === process.argv[1]) {
  try {
    // In a real application the user comes from the signed-in session.
    console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada", "example"));
  } finally {
    // The session belongs to the conversation. This one is done with it.
    await (await submilli("u_ada", "example")).close();
  }
}
